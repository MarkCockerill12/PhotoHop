//! Bezier roto shapes: editable, keyframable masks drawn on a layer (Resolve "Power Window" /
//! GIMP "Paths"). Points live in the layer's source pixel space, so shapes follow the layer's
//! transform. The whole path is keyframed at once; in-between frames interpolate every point.

use crate::anim::{Animated, Lerp};
use serde::{Deserialize, Serialize};

/// One anchor with its two handles (offsets from the anchor, layer pixels).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct RotoPoint {
    pub p: [f32; 2],
    #[serde(default)]
    pub h_in: [f32; 2],
    #[serde(default)]
    pub h_out: [f32; 2],
}

impl RotoPoint {
    pub fn corner(p: [f32; 2]) -> Self {
        Self { p, h_in: [0.0; 2], h_out: [0.0; 2] }
    }
}

/// The anchors of one shape at one moment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct RotoPath(pub Vec<RotoPoint>);

impl Lerp for RotoPath {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        if self.0.len() != other.0.len() {
            // Point count changed between keys: no correspondence, so switch half way.
            return if t < 0.5 { self.clone() } else { other.clone() };
        }
        RotoPath(
            self.0
                .iter()
                .zip(&other.0)
                .map(|(a, b)| RotoPoint { p: a.p.lerp(&b.p, t), h_in: a.h_in.lerp(&b.h_in, t), h_out: a.h_out.lerp(&b.h_out, t) })
                .collect(),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RotoMode {
    /// Union with the shapes above it in the list.
    #[default]
    Add,
    /// Cut out of the shapes above it.
    Subtract,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RotoShape {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// False while the shape is still being drawn (open paths don't mask anything).
    pub closed: bool,
    #[serde(default)]
    pub mode: RotoMode,
    #[serde(default)]
    pub invert: bool,
    /// Feather radius in layer pixels.
    pub feather: Animated<f32>,
    pub opacity: Animated<f32>,
    pub path: Animated<RotoPath>,
}

fn yes() -> bool {
    true
}

impl RotoShape {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            enabled: true,
            closed: false,
            mode: RotoMode::Add,
            invert: false,
            feather: Animated::new(0.0),
            opacity: Animated::new(1.0),
            path: Animated::new(RotoPath::default()),
        }
    }

    /// The path at `t` flattened to a closed polygon (layer pixels). `segs` lines per curve span.
    pub fn polygon(&self, t: f64, segs: usize) -> Vec<[f32; 2]> {
        flatten(&self.path.at(t), self.closed, segs)
    }

    /// Whether this shape currently contributes to the mask.
    pub fn active(&self) -> bool {
        self.enabled && self.closed && (self.path.base.0.len() >= 3 || self.path.keys.iter().any(|k| k.value.0.len() >= 3))
    }
}

/// Cubic bezier through the anchors (handles as offsets), as a polyline.
pub fn flatten(path: &RotoPath, closed: bool, segs: usize) -> Vec<[f32; 2]> {
    let pts = &path.0;
    let n = pts.len();
    if n < 2 {
        return pts.iter().map(|p| p.p).collect();
    }
    let spans = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(spans * segs + 1);
    for i in 0..spans {
        let (a, b) = (&pts[i], &pts[(i + 1) % n]);
        let p0 = a.p;
        let p1 = [a.p[0] + a.h_out[0], a.p[1] + a.h_out[1]];
        let p2 = [b.p[0] + b.h_in[0], b.p[1] + b.h_in[1]];
        let p3 = b.p;
        let straight = a.h_out == [0.0; 2] && b.h_in == [0.0; 2];
        let steps = if straight { 1 } else { segs.max(1) };
        for s in 0..steps {
            let u = s as f32 / steps as f32;
            let v = 1.0 - u;
            let (w0, w1, w2, w3) = (v * v * v, 3.0 * v * v * u, 3.0 * v * u * u, u * u * u);
            out.push([
                w0 * p0[0] + w1 * p1[0] + w2 * p2[0] + w3 * p3[0],
                w0 * p0[1] + w1 * p1[1] + w2 * p2[1] + w3 * p3[1],
            ]);
        }
    }
    if !closed {
        out.push(pts[n - 1].p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(s: f32) -> RotoPath {
        RotoPath(vec![RotoPoint::corner([0.0, 0.0]), RotoPoint::corner([s, 0.0]), RotoPoint::corner([s, s]), RotoPoint::corner([0.0, s])])
    }

    #[test]
    fn straight_square_flattens_to_corners() {
        let poly = flatten(&square(10.0), true, 16);
        assert_eq!(poly, vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
    }

    #[test]
    fn curved_span_passes_through_ends_and_bulges() {
        let path = RotoPath(vec![
            RotoPoint { p: [0.0, 0.0], h_in: [0.0; 2], h_out: [0.0, -10.0] },
            RotoPoint { p: [10.0, 0.0], h_in: [0.0, -10.0], h_out: [0.0; 2] },
        ]);
        let poly = flatten(&path, false, 8);
        assert_eq!(poly[0], [0.0, 0.0]);
        assert_eq!(*poly.last().unwrap(), [10.0, 0.0]);
        let mid = poly[4];
        assert!((mid[0] - 5.0).abs() < 1e-4 && (mid[1] + 7.5).abs() < 1e-4, "{mid:?}");
    }

    #[test]
    fn keyed_paths_interpolate_pointwise() {
        let mut s = RotoShape::new("a");
        s.closed = true;
        s.path.set(0.0, square(10.0), true);
        s.path.set(1.0, square(20.0), true);
        let mid = s.path.at(0.5);
        assert_eq!(mid.0[2].p, [15.0, 15.0]);
        assert!(s.active());
    }
}
