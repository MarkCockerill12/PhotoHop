//! Non-AI colour matching: compares a layer's colour statistics (inside its alpha × mask) with a
//! reference layer and turns the difference into grade parameters (match scale / offset), applied
//! in perceptual space by the GPU grade. Statistics use the current preview frame.

use ph_media::Frame;
use ph_media::color::linear_to_srgb;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatchMode {
    /// Per-channel mean and spread (Reinhard-style transfer).
    Full,
    /// Only the colour balance (mean hue shift), brightness untouched.
    ColourCast,
    /// Only brightness and contrast (same change on all channels).
    Tone,
}

impl MatchMode {
    pub const ALL: [MatchMode; 3] = [Self::ColourCast, Self::Tone, Self::Full];

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full (colour + brightness + contrast)",
            Self::ColourCast => "Colour cast only",
            Self::Tone => "Brightness & contrast only",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub mean: [f32; 3],
    pub std: [f32; 3],
    pub weight: f32,
}

const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

fn luma(c: [f32; 3]) -> f32 {
    c[0] * LUMA[0] + c[1] * LUMA[1] + c[2] * LUMA[2]
}

/// Weighted perceptual-space statistics. `mask` gives an extra weight per pixel from normalised
/// coordinates (layer masks); about 250k pixels are sampled.
pub fn stats(frame: &Frame, mask: Option<&dyn Fn(f32, f32) -> f32>) -> Stats {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let step = (((w * h) as f32 / 250_000.0).sqrt().ceil() as usize).max(1);
    let (mut sum, mut sq, mut wsum) = ([0f64; 3], [0f64; 3], 0f64);
    for y in (0..h).step_by(step) {
        for x in (0..w).step_by(step) {
            let i = (y * w + x) * 4;
            let a = frame.pixels[i + 3].to_f32();
            if a <= 0.01 {
                continue;
            }
            let mw = mask.map_or(1.0, |m| m((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32));
            let wt = (a * mw) as f64;
            if wt <= 0.01 {
                continue;
            }
            for c in 0..3 {
                let v = linear_to_srgb((frame.pixels[i + c].to_f32() / a).max(0.0)) as f64;
                sum[c] += v * wt;
                sq[c] += v * v * wt;
            }
            wsum += wt;
        }
    }
    if wsum <= 0.0 {
        return Stats::default();
    }
    let mean = sum.map(|s| (s / wsum) as f32);
    let std = std::array::from_fn(|c| ((sq[c] / wsum) as f32 - mean[c] * mean[c]).max(0.0).sqrt());
    Stats { mean, std, weight: wsum as f32 }
}

/// Grade match parameters `(scale, offset)` that move `src` statistics towards `target`.
pub fn transfer(src: &Stats, target: &Stats, mode: MatchMode) -> ([f32; 3], [f32; 3]) {
    match mode {
        MatchMode::Full => {
            let mut scale = [1.0; 3];
            let mut offset = [0.0; 3];
            for c in 0..3 {
                scale[c] = (target.std[c] / src.std[c].max(1e-3)).clamp(0.25, 4.0);
                offset[c] = target.mean[c] - src.mean[c] * scale[c];
            }
            (scale, offset)
        }
        MatchMode::ColourCast => {
            let d: [f32; 3] = std::array::from_fn(|c| target.mean[c] - src.mean[c]);
            let dy = luma(d);
            ([1.0; 3], d.map(|v| v - dy))
        }
        MatchMode::Tone => {
            let (ms, mt) = (luma(src.mean), luma(target.mean));
            let (ss, st) = (luma(src.std), luma(target.std));
            let k = (st / ss.max(1e-3)).clamp(0.25, 4.0);
            ([k; 3], [mt - ms * k; 3])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use half::f16;

    fn flat(rgb: [f32; 3]) -> Frame {
        let px: Vec<f16> = (0..64).flat_map(|_| [rgb[0], rgb[1], rgb[2], 1.0].map(f16::from_f32)).collect();
        Frame::new(8, 8, px)
    }

    #[test]
    fn colour_cast_keeps_brightness() {
        let s = stats(&flat([0.2, 0.2, 0.2]), None);
        let t = stats(&flat([0.4, 0.2, 0.05]), None);
        let (scale, off) = transfer(&s, &t, MatchMode::ColourCast);
        assert_eq!(scale, [1.0; 3]);
        assert!(luma(off).abs() < 1e-5, "{off:?}");
        assert!(off[0] > 0.0 && off[2] < 0.0);
    }

    #[test]
    fn full_maps_mean() {
        let s = stats(&flat([0.1, 0.3, 0.5]), None);
        let t = stats(&flat([0.5, 0.3, 0.1]), None);
        let (scale, off) = transfer(&s, &t, MatchMode::Full);
        for c in 0..3 {
            assert!((s.mean[c] * scale[c] + off[c] - t.mean[c]).abs() < 1e-4);
        }
    }
}
