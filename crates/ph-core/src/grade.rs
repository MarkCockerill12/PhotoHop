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
}

impl Grade {
    pub fn is_identity(&self) -> bool {
        *self == Grade { enabled: self.enabled, ..Grade::default() }
    }

    pub fn sample(&self, t: f64) -> GradeSample {
        let s = self.match_scale;
        let o = self.match_offset;
        GradeSample {
            basic: [self.exposure.at(t), self.temperature.at(t), self.tint.at(t), f32::from(u8::from(self.enabled))],
            lift: self.lift.at(t),
            gamma: self.gamma.at(t),
            gain: self.gain.at(t),
            offset: self.offset.at(t),
            tone: [self.contrast.at(t), self.pivot.at(t), self.saturation.at(t), self.match_amount.at(t)],
            match_scale: [s[0], s[1], s[2], 0.0],
            match_offset: [o[0], o[1], o[2], 0.0],
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
}
