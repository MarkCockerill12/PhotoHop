//! Keyframed parameters. Every animatable value in a document is an [`Animated<T>`]:
//! a constant when it has no keyframes, otherwise interpolated over time (seconds).

use serde::{Deserialize, Serialize};

/// How the value travels from one keyframe to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Interp {
    /// Jump at the next key (Resolve "Step").
    Hold,
    #[default]
    Linear,
    /// Smooth ease in/out (cubic smoothstep between keys).
    Smooth,
}

/// Values that can be interpolated.
pub trait Lerp: Clone {
    fn lerp(&self, other: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

impl Lerp for [f32; 2] {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        [self[0].lerp(&other[0], t), self[1].lerp(&other[1], t)]
    }
}

impl Lerp for [f32; 4] {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        std::array::from_fn(|i| self[i].lerp(&other[i], t))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key<T> {
    pub time: f64,
    pub value: T,
    pub interp: Interp,
}

/// A parameter that is either constant or keyframed. Keys are kept sorted by time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animated<T> {
    pub base: T,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<Key<T>>,
}

/// Two keys closer than this (seconds) are treated as the same key.
pub const KEY_EPS: f64 = 1e-4;

impl<T: Lerp> Animated<T> {
    pub fn new(value: T) -> Self {
        Self { base: value, keys: Vec::new() }
    }

    pub fn is_animated(&self) -> bool {
        !self.keys.is_empty()
    }

    /// Value at time `t` (seconds).
    pub fn at(&self, t: f64) -> T {
        let keys = &self.keys;
        match keys.len() {
            0 => self.base.clone(),
            1 => keys[0].value.clone(),
            _ => {
                if t <= keys[0].time {
                    return keys[0].value.clone();
                }
                let last = keys.len() - 1;
                if t >= keys[last].time {
                    return keys[last].value.clone();
                }
                let i = keys.partition_point(|k| k.time <= t) - 1;
                let (a, b) = (&keys[i], &keys[i + 1]);
                let span = (b.time - a.time).max(1e-9);
                let u = ((t - a.time) / span) as f32;
                let u = match a.interp {
                    Interp::Hold => 0.0,
                    Interp::Linear => u,
                    Interp::Smooth => u * u * (3.0 - 2.0 * u),
                };
                a.value.lerp(&b.value, u)
            }
        }
    }

    /// Index of a key at exactly `t` (within [`KEY_EPS`]).
    pub fn key_index_at(&self, t: f64) -> Option<usize> {
        self.keys.iter().position(|k| (k.time - t).abs() < KEY_EPS)
    }

    pub fn has_key_at(&self, t: f64) -> bool {
        self.key_index_at(t).is_some()
    }

    /// Set the value "at time t": updates the key there if animated, adds a key if `add_key`,
    /// otherwise changes the constant base value.
    pub fn set(&mut self, t: f64, value: T, add_key: bool) {
        if let Some(i) = self.key_index_at(t) {
            self.keys[i].value = value;
        } else if add_key || self.is_animated() {
            let i = self.keys.partition_point(|k| k.time < t);
            self.keys.insert(i, Key { time: t, value, interp: Interp::default() });
        } else {
            self.base = value;
        }
    }

    /// Toggle a key at `t`: removes it if present, otherwise stores the current value as a key.
    pub fn toggle_key(&mut self, t: f64) {
        if let Some(i) = self.key_index_at(t) {
            let removed = self.keys.remove(i);
            if self.keys.is_empty() {
                self.base = removed.value;
            }
        } else {
            let v = self.at(t);
            self.set(t, v, true);
        }
    }

    pub fn key_times(&self) -> impl Iterator<Item = f64> + '_ {
        self.keys.iter().map(|k| k.time)
    }
}

impl<T: Lerp + Default> Default for Animated<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_and_keys() {
        let mut a = Animated::new(1.0f32);
        assert_eq!(a.at(5.0), 1.0);
        a.set(0.0, 0.0, true);
        a.set(2.0, 10.0, true);
        assert_eq!(a.at(1.0), 5.0);
        assert_eq!(a.at(-1.0), 0.0);
        assert_eq!(a.at(3.0), 10.0);
        a.keys[0].interp = Interp::Hold;
        assert_eq!(a.at(1.9), 0.0);
    }

    #[test]
    fn toggle_restores_base() {
        let mut a = Animated::new(2.0f32);
        a.toggle_key(1.0);
        assert!(a.has_key_at(1.0));
        a.toggle_key(1.0);
        assert!(!a.is_animated());
        assert_eq!(a.base, 2.0);
    }
}
