//! Selections rasterised into coverage masks (0..1, one value per mask pixel).
//! Shapes are anti-aliased with 4×4 supersampling; the magic wand follows GIMP's fuzzy select
//! (max per-channel difference, fully selected up to the threshold, linear ramp to 1.5×).

use glam::Vec2;
use ph_media::Frame;
use ph_media::color::linear_to_srgb;
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SelectShape {
    #[default]
    Rect,
    Ellipse,
    Lasso,
    Wand,
}

impl SelectShape {
    pub const ALL: [SelectShape; 4] = [Self::Rect, Self::Ellipse, Self::Lasso, Self::Wand];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rect => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Lasso => "Lasso",
            Self::Wand => "Magic wand",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Combine {
    Replace,
    Add,
    Subtract,
}

const SS: usize = 4;

fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Coverage of a closed polygon (even-odd) in mask pixel coordinates.
pub fn polygon(w: u32, h: u32, poly: &[Vec2]) -> Vec<f32> {
    let mut out = vec![0.0; (w * h) as usize];
    if poly.len() < 3 {
        return out;
    }
    let lo = poly.iter().fold(Vec2::splat(f32::MAX), |a, p| a.min(*p)).max(Vec2::ZERO);
    let hi = poly.iter().fold(Vec2::splat(f32::MIN), |a, p| a.max(*p)).min(Vec2::new(w as f32, h as f32));
    let (x0, y0) = (lo.x.floor() as u32, lo.y.floor() as u32);
    let (x1, y1) = (hi.x.ceil() as u32, hi.y.ceil() as u32);
    for y in y0..y1.min(h) {
        for x in x0..x1.min(w) {
            let mut n = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let p = Vec2::new(x as f32 + (sx as f32 + 0.5) / SS as f32, y as f32 + (sy as f32 + 0.5) / SS as f32);
                    n += usize::from(point_in_polygon(p, poly));
                }
            }
            out[(y * w + x) as usize] = n as f32 / (SS * SS) as f32;
        }
    }
    out
}

#[cfg(test)]
pub fn rect(w: u32, h: u32, a: Vec2, b: Vec2) -> Vec<f32> {
    let (lo, hi) = (a.min(b), a.max(b));
    polygon(w, h, &[lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)])
}

/// Magic wand: contiguous region around `seed` (mask pixel coords) whose colour is within
/// `threshold` (0..1, perceptual, max channel difference) of the seed colour. `frame` is sampled
/// at the mask's resolution.
pub fn wand(frame: &Frame, w: u32, h: u32, seed: Vec2, threshold: f32) -> Vec<f32> {
    let sample = |x: u32, y: u32| -> [f32; 4] {
        let fx = ((x as f32 + 0.5) / w as f32 * frame.width as f32) as usize;
        let fy = ((y as f32 + 0.5) / h as f32 * frame.height as f32) as usize;
        let i = (fy.min(frame.height as usize - 1) * frame.width as usize + fx.min(frame.width as usize - 1)) * 4;
        let a = frame.pixels[i + 3].to_f32();
        let inv = if a > 1e-4 { 1.0 / a } else { 0.0 };
        let [r, g, b] = [0, 1, 2].map(|c| linear_to_srgb((frame.pixels[i + c].to_f32() * inv).max(0.0)));
        [r, g, b, a]
    };
    let mut out = vec![0.0; (w * h) as usize];
    let (sx, sy) = (seed.x.floor() as i64, seed.y.floor() as i64);
    if sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 {
        return out;
    }
    let target = sample(sx as u32, sy as u32);
    let thr = threshold.max(1e-4);
    let score = |c: [f32; 4]| -> f32 {
        let d = (0..4).map(|k| (c[k] - target[k]).abs()).fold(0.0, f32::max);
        let aa = 1.5 - d / thr;
        if aa <= 0.0 { 0.0 } else if aa < 0.5 { aa * 2.0 } else { 1.0 }
    };
    let mut seen = vec![false; (w * h) as usize];
    let mut q = VecDeque::from([(sx as u32, sy as u32)]);
    seen[(sy as u32 * w + sx as u32) as usize] = true;
    while let Some((x, y)) = q.pop_front() {
        let v = score(sample(x, y));
        if v <= 0.0 {
            continue;
        }
        out[(y * w + x) as usize] = v;
        for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                let i = (ny as u32 * w + nx as u32) as usize;
                if !seen[i] {
                    seen[i] = true;
                    q.push_back((nx as u32, ny as u32));
                }
            }
        }
    }
    out
}

/// Gaussian-ish feather (three box passes, σ ≈ radius / 3.5 like GIMP).
pub fn feather(cov: &mut [f32], w: u32, h: u32, radius: f32) {
    let r = (radius / 3.5 * 1.7).round() as usize;
    if r == 0 {
        return;
    }
    let (w, h) = (w as usize, h as usize);
    let mut tmp = vec![0.0; w * h];
    for _ in 0..3 {
        for y in 0..h {
            for x in 0..w {
                let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
                tmp[y * w + x] = cov[y * w + a..=y * w + b].iter().sum::<f32>() / (b - a + 1) as f32;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
                cov[y * w + x] = (a..=b).map(|yy| tmp[yy * w + x]).sum::<f32>() / (b - a + 1) as f32;
            }
        }
    }
}

/// Combine a selection into an existing mask (red channel of `mask`).
pub fn apply(mask: &mut [[f32; 4]], sel: &[f32], mode: Combine) {
    for (m, s) in mask.iter_mut().zip(sel) {
        let v = match mode {
            Combine::Replace => *s,
            Combine::Add => m[0].max(*s),
            Combine::Subtract => m[0].min(1.0 - s),
        };
        *m = [v, v, v, 1.0];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use half::f16;

    #[test]
    fn rect_coverage_is_antialiased() {
        let c = rect(10, 10, Vec2::new(2.5, 2.0), Vec2::new(6.0, 6.0));
        assert_eq!(c[3 * 10 + 4], 1.0);
        assert!((c[3 * 10 + 2] - 0.5).abs() < 0.01, "half-covered column: {}", c[3 * 10 + 2]);
        assert_eq!(c[0], 0.0);
    }

    #[test]
    fn wand_stops_at_edges() {
        // Left half dark, right half bright.
        let px: Vec<f16> = (0..64).flat_map(|i| { let v = if i % 8 < 4 { 0.05 } else { 0.8 }; [v, v, v, 1.0].map(f16::from_f32) }).collect();
        let f = Frame::new(8, 8, px);
        let s = wand(&f, 8, 8, Vec2::new(1.0, 1.0), 0.1);
        assert_eq!(s[2 * 8 + 1], 1.0);
        assert_eq!(s[2 * 8 + 6], 0.0);
    }

    #[test]
    fn subtract_and_add() {
        let mut m = vec![[1.0f32; 4]; 4];
        apply(&mut m, &[1.0, 0.0, 0.5, 0.0], Combine::Subtract);
        assert_eq!([m[0][0], m[1][0], m[2][0]], [0.0, 1.0, 0.5]);
        apply(&mut m, &[1.0, 0.0, 0.0, 0.0], Combine::Add);
        assert_eq!(m[0][0], 1.0);
    }
}
