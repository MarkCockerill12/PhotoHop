//! "Match to scene" result stored as ordinary slider values.
//!
//! A colour match (smart or AI) is expressed in the six everyday grade sliders. The grade keeps
//! the slider values from before the match (`original`) and the fitted values (`matched`); the
//! Match strength slider moves every slider between the two, so the user sees exactly what the
//! match changed and can fine-tune any slider afterwards.

use crate::grade::Grade;
use serde::{Deserialize, Serialize};

/// The sliders a scene match sets.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LookSliders {
    pub exposure: f32,
    pub temperature: f32,
    pub tint: f32,
    pub contrast: f32,
    pub saturation: f32,
    /// Shadow level: the master value of Lift (raises or lowers the blacks).
    pub black: f32,
}

impl Default for LookSliders {
    fn default() -> Self {
        Self { exposure: 0.0, temperature: 0.0, tint: 0.0, contrast: 1.0, saturation: 1.0, black: 0.0 }
    }
}

impl LookSliders {
    pub const NAMES: [&'static str; 6] = ["Exposure", "Temperature", "Tint", "Contrast", "Saturation", "Shadow level"];

    pub fn to_array(self) -> [f32; 6] {
        [self.exposure, self.temperature, self.tint, self.contrast, self.saturation, self.black]
    }

    pub fn from_array(a: [f32; 6]) -> Self {
        Self { exposure: a[0], temperature: a[1], tint: a[2], contrast: a[3], saturation: a[4], black: a[5] }
    }

    pub fn read(g: &Grade, t: f64) -> Self {
        Self {
            exposure: g.exposure.at(t),
            temperature: g.temperature.at(t),
            tint: g.tint.at(t),
            contrast: g.contrast.at(t),
            saturation: g.saturation.at(t),
            black: g.lift.at(t)[3],
        }
    }

    /// Write the sliders into the grade at time `t` (`key` = add keyframes, as with auto-key).
    pub fn write(self, g: &mut Grade, t: f64, key: bool) {
        g.exposure.set(t, self.exposure, key);
        g.temperature.set(t, self.temperature, key);
        g.tint.set(t, self.tint, key);
        g.contrast.set(t, self.contrast, key);
        g.saturation.set(t, self.saturation, key);
        let mut lift = g.lift.at(t);
        lift[3] = self.black;
        g.lift.set(t, lift, key);
    }

    pub fn lerp(a: Self, b: Self, k: f32) -> Self {
        let (a, b) = (a.to_array(), b.to_array());
        Self::from_array(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * k))
    }

    fn close(a: Self, b: Self) -> bool {
        a.to_array().iter().zip(b.to_array()).all(|(x, y)| (x - y).abs() < 1e-4)
    }
}

/// How a scene match was made (shown to the user).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MatchMethod {
    /// Measured from the scene around the layer (instant, no AI).
    #[default]
    Smart,
    /// Fitted to an AI harmonisation suggestion.
    Ai,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneMatch {
    pub original: LookSliders,
    pub matched: LookSliders,
    /// 0 = original, 1 = fully matched.
    pub strength: f32,
    #[serde(default)]
    pub method: MatchMethod,
}

impl SceneMatch {
    pub fn expected(&self) -> LookSliders {
        LookSliders::lerp(self.original, self.matched, self.strength)
    }
}

impl Grade {
    /// Keep hand edits made after a match: if the sliders no longer sit where the strength puts
    /// them, shift both ends by the difference (so the strength slider keeps the user's tweak).
    pub fn sync_scene_match(&mut self, t: f64) {
        if self.scene_match.is_none() {
            return;
        }
        let cur = LookSliders::read(self, t);
        let Some(m) = &mut self.scene_match else { return };
        let exp = m.expected();
        if LookSliders::close(cur, exp) {
            return;
        }
        let (c, e) = (cur.to_array(), exp.to_array());
        let (o, mt) = (m.original.to_array(), m.matched.to_array());
        m.original = LookSliders::from_array(std::array::from_fn(|i| o[i] + c[i] - e[i]));
        m.matched = LookSliders::from_array(std::array::from_fn(|i| mt[i] + c[i] - e[i]));
    }

    /// Move the sliders to `strength` between original (0) and matched (1).
    pub fn set_match_strength(&mut self, strength: f32, t: f64, key: bool) {
        self.sync_scene_match(t);
        let Some(m) = &mut self.scene_match else { return };
        m.strength = strength.clamp(0.0, 1.0);
        let v = m.expected();
        v.write(self, t, key);
    }

    /// Store a new match: the current sliders become "original", `matched` is applied at full strength.
    pub fn apply_scene_match(&mut self, matched: LookSliders, method: MatchMethod, t: f64, key: bool) {
        // A new match starts from what the user had before any previous match.
        self.sync_scene_match(t);
        let original = self.scene_match.map_or_else(|| LookSliders::read(self, t), |m| m.original);
        self.scene_match = Some(SceneMatch { original, matched, strength: 1.0, method });
        matched.write(self, t, key);
        self.enabled = true;
    }
}

/// CPU copy of the GPU grade for the six look sliders (pivot as given; everything else identity).
/// `lin` is linear RGB; returns perceptual (sRGB-encoded) RGB, unclamped above 1.
pub fn grade_perceptual(lin: [f32; 3], s: &LookSliders, pivot: f32) -> [f32; 3] {
    let ex = s.exposure.exp2();
    let wb = [(0.25 * s.temperature).exp(), (-0.25 * s.tint).exp(), (-0.25 * s.temperature).exp()];
    let wl = 0.2126 * wb[0] + 0.7152 * wb[1] + 0.0722 * wb[2];
    let mut p: [f32; 3] = std::array::from_fn(|c| to_perceptual(lin[c] * ex * wb[c] / wl));
    for v in &mut p {
        *v += s.black * (1.0 - *v);
        *v = (*v - pivot) * s.contrast + pivot;
    }
    let y = 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
    p.map(|v| (y + (v - y) * s.saturation).max(0.0))
}

pub fn to_perceptual(x: f32) -> f32 {
    let x = x.max(0.0);
    if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

pub fn from_perceptual(c: f32) -> f32 {
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_moves_sliders_and_keeps_hand_edits() {
        let mut g = Grade::default();
        let m = LookSliders { exposure: 1.0, saturation: 0.5, ..LookSliders::default() };
        g.apply_scene_match(m, MatchMethod::Smart, 0.0, false);
        assert_eq!(g.exposure.at(0.0), 1.0);
        g.set_match_strength(0.5, 0.0, false);
        assert!((g.exposure.at(0.0) - 0.5).abs() < 1e-6);
        assert!((g.saturation.at(0.0) - 0.75).abs() < 1e-6);
        // Hand edit: +0.2 contrast at half strength survives moving the strength.
        g.contrast.set(0.0, 1.2, false);
        g.set_match_strength(0.0, 0.0, false);
        assert!((g.contrast.at(0.0) - 1.2).abs() < 1e-6);
        assert!(g.exposure.at(0.0).abs() < 1e-6);
        g.set_match_strength(1.0, 0.0, false);
        assert!((g.exposure.at(0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cpu_grade_identity() {
        let p = grade_perceptual([0.2, 0.5, 0.8], &LookSliders::default(), 0.435);
        for (c, v) in p.iter().zip([0.2f32, 0.5, 0.8]) {
            assert!((c - to_perceptual(v)).abs() < 1e-5);
        }
    }
}
