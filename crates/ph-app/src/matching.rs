//! Non-AI colour matching: compares a layer's colour statistics (inside its alpha × mask) with a
//! reference layer and turns the difference into grade parameters (match scale / offset), applied
//! in perceptual space by the GPU grade. Statistics use the current preview frame.

use ph_media::Frame;
use ph_media::color::linear_to_srgb;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatchMode {
    /// Colour cast + tone from the background immediately around the layer (most natural).
    Surroundings,
    /// Per-channel mean and spread (Reinhard-style transfer).
    Full,
    /// Only the colour balance (mean hue shift), brightness untouched.
    ColourCast,
    /// Only brightness and contrast (same change on all channels).
    Tone,
}

impl MatchMode {
    pub const ALL: [MatchMode; 4] = [Self::Surroundings, Self::ColourCast, Self::Tone, Self::Full];

    pub fn label(self) -> &'static str {
        match self {
            Self::Surroundings => "Surroundings (recommended)",
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
    /// Luma at the 2nd and 98th percentiles (black and white points).
    pub black: f32,
    pub white: f32,
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
    let mut hist = [0f64; 256];
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
            let mut px = [0f64; 3];
            for c in 0..3 {
                let v = linear_to_srgb((frame.pixels[i + c].to_f32() / a).max(0.0)) as f64;
                sum[c] += v * wt;
                sq[c] += v * v * wt;
                px[c] = v;
            }
            let y = 0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2];
            hist[((y * 255.0).round() as usize).min(255)] += wt;
            wsum += wt;
        }
    }
    if wsum <= 0.0 {
        return Stats::default();
    }
    let mean = sum.map(|s| (s / wsum) as f32);
    let std = std::array::from_fn(|c| ((sq[c] / wsum) as f32 - mean[c] * mean[c]).max(0.0).sqrt());
    let pct = |p: f64| -> f32 {
        let mut acc = 0.0;
        for (i, h) in hist.iter().enumerate() {
            acc += h;
            if acc >= p * wsum {
                return i as f32 / 255.0;
            }
        }
        1.0
    };
    Stats { mean, std, weight: wsum as f32, black: pct(0.02), white: pct(0.98) }
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
        MatchMode::Surroundings => {
            // Colourist-style: match the black point to the scene while keeping the subject's own
            // highlights (subjects are often darker or brighter than their surroundings by nature),
            // plus 60 % of the colour cast difference.
            let (sb, sw, tb) = (src.black, src.white.max(src.black + 0.05), target.black);
            let k = ((sw - tb) / (sw - sb)).clamp(0.7, 1.1);
            let off = tb - sb * k;
            let d: [f32; 3] = std::array::from_fn(|c| target.mean[c] - src.mean[c]);
            let dy = luma(d);
            ([k; 3], std::array::from_fn(|c| off + 0.6 * (d[c] - dy)))
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

/// Statistics of the subject (opaque pixels of `subject`) and of the background in a band around
/// it (inside the subject's bounding box grown by 50 %, where the subject is absent). Both frames
/// are canvas-sized composites.
pub fn surround_stats(background: &Frame, subject: &Frame) -> Option<(Stats, Stats)> {
    let (w, h) = (subject.width as usize, subject.height as usize);
    let alpha = |x: usize, y: usize| subject.pixels[(y * w + x) * 4 + 3].to_f32();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            if alpha(x, y) > 0.5 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (gx, gy) = ((x1 - x0) / 2, (y1 - y0) / 2);
    let (bx0, by0, bx1, by1) = (x0.saturating_sub(gx), y0.saturating_sub(gy), (x1 + gx).min(w - 1), (y1 + gy).min(h - 1));
    let ring = |u: f32, v: f32| -> f32 {
        let (x, y) = (((u * w as f32) as usize).min(w - 1), ((v * h as f32) as usize).min(h - 1));
        if x < bx0 || x > bx1 || y < by0 || y > by1 || alpha(x, y) > 0.05 { 0.0 } else { 1.0 }
    };
    let solid = |u: f32, v: f32| -> f32 {
        let (x, y) = (((u * w as f32) as usize).min(w - 1), ((v * h as f32) as usize).min(h - 1));
        if alpha(x, y) > 0.9 { 1.0 } else { 0.0 }
    };
    let s = stats(subject, Some(&solid));
    let t = stats(background, Some(&ring));
    (s.weight > 0.0 && t.weight > 0.0).then_some((s, t))
}

/// Fit per-channel tone curves (perceptual 0..1) mapping `before` to `after` over pixels where
/// `mask` > 0.9. Both images are 8-bit sRGB RGB of the same size. Used to turn an AI colour
/// suggestion into ordinary, editable curves (the AI's pixels themselves are discarded).
pub fn fit_curves(before: &[u8], after: &[u8], mask: &[u8], max_points: usize) -> [Vec<[f32; 2]>; 3] {
    const BINS: usize = 32;
    std::array::from_fn(|c| {
        let (mut sum, mut cnt) = ([0f64; BINS], [0u32; BINS]);
        for (i, m) in mask.iter().enumerate() {
            if *m < 230 {
                continue;
            }
            let x = before[i * 3 + c] as usize;
            let b = (x * BINS / 256).min(BINS - 1);
            sum[b] += after[i * 3 + c] as f64 / 255.0;
            cnt[b] += 1;
        }
        let total: u32 = cnt.iter().sum();
        let mut pts: Vec<[f32; 2]> = (0..BINS)
            .filter(|&b| cnt[b] >= (total / 500).max(3))
            .map(|b| [((b as f32 + 0.5) / BINS as f32), (sum[b] / cnt[b] as f64) as f32])
            .collect();
        if pts.is_empty() {
            return crate::matching::identity();
        }
        // Monotone, then thin to at most `max_points` evenly spaced along the data.
        for i in 1..pts.len() {
            pts[i][1] = pts[i][1].max(pts[i - 1][1]);
        }
        if pts.len() > max_points {
            let step = (pts.len() - 1) as f32 / (max_points - 1) as f32;
            pts = (0..max_points).map(|k| pts[((k as f32 * step).round() as usize).min(pts.len() - 1)]).collect();
        }
        // Extend to the ends with slope 1 so untouched tones continue naturally.
        let first = pts[0];
        let last = *pts.last().expect("non-empty");
        let mut out = vec![[0.0, (first[1] - first[0]).clamp(0.0, 1.0)]];
        out.extend(pts);
        out.push([1.0, (last[1] + 1.0 - last[0]).clamp(0.0, 1.0)]);
        out.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-3);
        out
    })
}

fn identity() -> Vec<[f32; 2]> {
    ph_core::grade::IDENTITY_CURVE.to_vec()
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
    fn fitted_curve_reproduces_a_brightening() {
        // after = before * 1.2 (clamped) on the masked pixels.
        let before: Vec<u8> = (0..300u32).flat_map(|i| [(i % 200) as u8; 3]).collect();
        let after: Vec<u8> = before.iter().map(|v| ((*v as f32) * 1.2).min(255.0) as u8).collect();
        let mask = vec![255u8; 300];
        let [r, _, _] = fit_curves(&before, &after, &mask, 8);
        let y = ph_core::grade::eval_curve(&r, 100.0 / 255.0);
        assert!((y - 120.0 / 255.0).abs() < 0.03, "{y}");
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
