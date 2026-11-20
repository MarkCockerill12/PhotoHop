//! Per-layer colour grade (Resolve-style primaries) and colour-match parameters.
//!
//! Order of operations (implemented in ph-gpu `grade()`):
//! 1. linear light: exposure (stops), white balance (temperature / tint)
//! 2. perceptual (sRGB-encoded) values: colour match → lift/gamma/gain → contrast about pivot →
//!    saturation → offset
//!
//! Lift/gamma/gain/offset are `[r, g, b, master]`. Identity: lift 0, gamma 0, gain 1, offset 0.

use crate::anim::Animated;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grade {
    pub enabled: bool,
    pub exposure: Animated<f32>,
    /// -1 (cooler/blue) .. 1 (warmer/orange).
    pub temperature: Animated<f32>,
    /// -1 (green) .. 1 (magenta).
    pub tint: Animated<f32>,
    /// Added to shadows: `p + lift * (1 - p)`.
    pub lift: Animated<[f32; 4]>,
    /// Exponent offsets: `p^(1 / exp(gamma))`; 0 = identity.
    pub gamma: Animated<[f32; 4]>,
    /// Multipliers; 1 = identity.
    pub gain: Animated<[f32; 4]>,
    pub offset: Animated<[f32; 4]>,
    pub contrast: Animated<f32>,
    pub pivot: Animated<f32>,
    pub saturation: Animated<f32>,
    /// Per-channel scale/offset computed by "Match colour" (perceptual space).
    pub match_scale: [f32; 3],
    pub match_offset: [f32; 3],
    /// 0 = match ignored, 1 = full match.
    pub match_amount: Animated<f32>,
    pub curves: Curves,
    pub qualifier: Qualifier,
}

impl Default for Grade {
    fn default() -> Self {
        Self {
            enabled: true,
            exposure: Animated::new(0.0),
            temperature: Animated::new(0.0),
            tint: Animated::new(0.0),
            lift: Animated::new([0.0; 4]),
            gamma: Animated::new([0.0; 4]),
            gain: Animated::new([1.0; 4]),
            offset: Animated::new([0.0; 4]),
            contrast: Animated::new(1.0),
            pivot: Animated::new(0.435),
            saturation: Animated::new(1.0),
            match_scale: [1.0; 3],
            match_offset: [0.0; 3],
            match_amount: Animated::new(0.0),
            curves: Curves::default(),
            qualifier: Qualifier::default(),
        }
    }
}

/// A grade evaluated at one point in time, laid out for the GPU (std140-friendly vec4s).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradeSample {
    /// exposure, temperature, tint, enabled(0/1)
    pub basic: [f32; 4],
    pub lift: [f32; 4],
    pub gamma: [f32; 4],
    pub gain: [f32; 4],
    pub offset: [f32; 4],
    /// contrast, pivot, saturation, match_amount
    pub tone: [f32; 4],
    /// rgb scale, unused
    pub match_scale: [f32; 4],
    /// rgb offset, unused
    pub match_offset: [f32; 4],
    /// hue_center, hue_width, softness, enabled(0/1)
    pub qual0: [f32; 4],
    /// sat lo, sat hi, lum lo, lum hi
    pub qual1: [f32; 4],
    /// hue shift, saturation, gain, flags (1 = invert, 2 = show matte)
    pub qual2: [f32; 4],
}

impl Grade {
    pub fn is_identity(&self) -> bool {
        *self == Grade { enabled: self.enabled, ..Grade::default() }
    }

    pub fn sample(&self, t: f64) -> GradeSample {
        let s = self.match_scale;
        let o = self.match_offset;
        let q = &self.qualifier;
        GradeSample {
            basic: [self.exposure.at(t), self.temperature.at(t), self.tint.at(t), f32::from(u8::from(self.enabled))],
            lift: self.lift.at(t),
            gamma: self.gamma.at(t),
            gain: self.gain.at(t),
            offset: self.offset.at(t),
            tone: [self.contrast.at(t), self.pivot.at(t), self.saturation.at(t), self.match_amount.at(t)],
            match_scale: [s[0], s[1], s[2], 0.0],
            match_offset: [o[0], o[1], o[2], 0.0],
            qual0: [q.hue_center, q.hue_width, q.softness, f32::from(u8::from(q.enabled))],
            qual1: [q.sat_range[0], q.sat_range[1], q.lum_range[0], q.lum_range[1]],
            qual2: [q.hue_shift, q.saturation, q.gain, f32::from(u8::from(q.invert)) + 2.0 * f32::from(u8::from(q.show_matte))],
        }
    }

    pub fn key_times(&self) -> impl Iterator<Item = f64> + '_ {
        self.exposure
            .key_times()
            .chain(self.temperature.key_times())
            .chain(self.tint.key_times())
            .chain(self.lift.key_times())
            .chain(self.gamma.key_times())
            .chain(self.gain.key_times())
            .chain(self.offset.key_times())
            .chain(self.contrast.key_times())
            .chain(self.pivot.key_times())
            .chain(self.saturation.key_times())
            .chain(self.match_amount.key_times())
    }
}

/// A greyscale layer mask (white = visible). Pixels live in a sidecar file like paint layers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerMask {
    pub enabled: bool,
    pub invert: bool,
    pub width: u32,
    pub height: u32,
    /// Per-frame mask from a greyscale video (AI tracking/matting); overrides the painted raster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<crate::doc::MediaId>,
}

/// Tone curves in perceptual space: per-channel then master. Points are `[x, y]` in 0..1, sorted by x.
/// Interpolation follows GIMP's curve spline (cubic Bézier per segment with inner x at 1/3, 2/3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Curves {
    pub master: Vec<[f32; 2]>,
    pub red: Vec<[f32; 2]>,
    pub green: Vec<[f32; 2]>,
    pub blue: Vec<[f32; 2]>,
}

pub const IDENTITY_CURVE: [[f32; 2]; 2] = [[0.0, 0.0], [1.0, 1.0]];
/// Entries in a baked curve LUT.
pub const CURVE_LUT_SIZE: usize = 256;

impl Default for Curves {
    fn default() -> Self {
        let id = IDENTITY_CURVE.to_vec();
        Self { master: id.clone(), red: id.clone(), green: id.clone(), blue: id }
    }
}

impl Curves {
    pub fn is_identity(&self) -> bool {
        [&self.master, &self.red, &self.green, &self.blue].iter().all(|c| c.as_slice() == IDENTITY_CURVE)
    }

    /// `[r, g, b, master]` per entry, for x = i / (N - 1).
    pub fn bake(&self) -> Vec<[f32; 4]> {
        (0..CURVE_LUT_SIZE)
            .map(|i| {
                let x = i as f32 / (CURVE_LUT_SIZE - 1) as f32;
                [eval_curve(&self.red, x), eval_curve(&self.green, x), eval_curve(&self.blue, x), eval_curve(&self.master, x)]
            })
            .collect()
    }
}

/// Evaluate a GIMP-style curve spline at `x` (flat outside the end points, clamped to 0..1).
pub fn eval_curve(pts: &[[f32; 2]], x: f32) -> f32 {
    match pts.len() {
        0 => return x,
        1 => return pts[0][1],
        _ => {}
    }
    if x <= pts[0][0] {
        return pts[0][1];
    }
    let last = pts.len() - 1;
    if x >= pts[last][0] {
        return pts[last][1];
    }
    let i = pts.partition_point(|p| p[0] <= x).saturating_sub(1).min(last - 1);
    let ([x1, y1], [x2, y2]) = (pts[i], pts[i + 1]);
    let dx = (x2 - x1).max(1e-6);
    let dy = y2 - y1;
    let left = (i > 0).then(|| pts[i - 1]);
    let right = (i + 2 <= last).then(|| pts[i + 2]);
    let (c1, c2) = match (left, right) {
        (None, None) => (y1 + dy / 3.0, y1 + 2.0 * dy / 3.0),
        (None, Some([x3, y3])) => {
            let slope = (y3 - y1) / (x3 - x1).max(1e-6);
            let c2 = y2 - slope * dx / 3.0;
            (y1 + (c2 - y1) / 2.0, c2)
        }
        (Some([x0, y0]), None) => {
            let slope = (y2 - y0) / (x2 - x0).max(1e-6);
            let c1 = y1 + slope * dx / 3.0;
            (c1, y2 + (c1 - y2) / 2.0)
        }
        (Some([x0, y0]), Some([x3, y3])) => (
            y1 + (y2 - y0) / (x2 - x0).max(1e-6) * dx / 3.0,
            y2 - (y3 - y1) / (x3 - x1).max(1e-6) * dx / 3.0,
        ),
    };
    let t = (x - x1) / dx;
    let u = 1.0 - t;
    (u * u * u * y1 + 3.0 * u * u * t * c1 + 3.0 * u * t * t * c2 + t * t * t * y2).clamp(0.0, 1.0)
}

/// HSL qualifier (Resolve-style secondary): selects a colour range with soft edges and adjusts it.
/// Hue is 0..1 (red = 0), ranges are on the layer's input colour so adjustments never change the selection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Qualifier {
    pub enabled: bool,
    pub hue_center: f32,
    /// Half-width of the fully selected hue band (0..0.5).
    pub hue_width: f32,
    pub sat_range: [f32; 2],
    pub lum_range: [f32; 2],
    /// Edge softness for all three ranges.
    pub softness: f32,
    pub invert: bool,
    /// Show the selection as a greyscale matte instead of the image (for checking).
    pub show_matte: bool,
    /// Hue rotation applied inside the selection (-0.5..0.5 of a turn).
    pub hue_shift: f32,
    pub saturation: f32,
    /// Brightness multiplier inside the selection.
    pub gain: f32,
}

impl Default for Qualifier {
    fn default() -> Self {
        Self {
            enabled: false,
            hue_center: 0.0,
            hue_width: 0.08,
            sat_range: [0.1, 1.0],
            lum_range: [0.0, 1.0],
            softness: 0.05,
            invert: false,
            show_matte: false,
            hue_shift: 0.0,
            saturation: 1.0,
            gain: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_curve_is_identity() {
        for i in 0..=10 {
            let x = i as f32 / 10.0;
            assert!((eval_curve(&IDENTITY_CURVE, x) - x).abs() < 1e-5);
        }
        assert!(Curves::default().is_identity());
    }

    #[test]
    fn curve_passes_through_points() {
        let pts = [[0.0, 0.0], [0.25, 0.4], [0.75, 0.6], [1.0, 1.0]];
        for p in pts {
            assert!((eval_curve(&pts, p[0]) - p[1]).abs() < 1e-4, "{p:?}");
        }
        let lut = Curves { master: pts.to_vec(), ..Default::default() }.bake();
        assert_eq!(lut.len(), CURVE_LUT_SIZE);
        assert!(lut.windows(2).all(|w| w[1][3] >= w[0][3] - 1e-4), "monotone input stays monotone");
    }
}
