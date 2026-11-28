//! 2D camera tracking without AI: textured patches around a region (a window's frame, the wall
//! around it) are followed through the clip by normalised cross-correlation, and a similarity
//! transform (move, zoom, roll) is fitted to them per frame with outliers dropped, so things that
//! move on their own (a head turning, a hand typing) don't pull the track.

use glam::{Affine2, Mat2, Vec2};
use ph_media::Frame;

/// Greyscale image (perceptual-ish: sqrt of linear luma).
pub struct Luma {
    pub w: usize,
    pub h: usize,
    pub px: Vec<f32>,
}

impl Luma {
    pub fn from_frame(f: &Frame) -> Self {
        let (w, h) = (f.width as usize, f.height as usize);
        let px = f
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| (0.2126 * p[0].to_f32() + 0.7152 * p[1].to_f32() + 0.0722 * p[2].to_f32()).max(0.0).sqrt())
            .collect();
        Self { w, h, px }
    }

    fn at(&self, x: usize, y: usize) -> f32 {
        self.px[y * self.w + x]
    }

    fn sample(&self, p: Vec2) -> f32 {
        let x = p.x.clamp(0.0, (self.w - 1) as f32 - 1e-3);
        let y = p.y.clamp(0.0, (self.h - 1) as f32 - 1e-3);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let top = self.at(x0, y0) * (1.0 - fx) + self.at(x1, y0) * fx;
        let bot = self.at(x0, y1) * (1.0 - fx) + self.at(x1, y1) * fx;
        top * (1.0 - fy) + bot * fy
    }
}

/// Patch half-size and search radius (pixels of the tracking images).
const R: i32 = 7;
const SEARCH: i32 = 12;

struct Feature {
    at: Vec2,
    /// Zero-mean, unit-norm template from the reference frame.
    tpl: Vec<f32>,
}

fn template(img: &Luma, c: Vec2) -> Option<Vec<f32>> {
    let mut v = Vec::with_capacity(((2 * R + 1) * (2 * R + 1)) as usize);
    for dy in -R..=R {
        for dx in -R..=R {
            v.push(img.sample(c + Vec2::new(dx as f32, dy as f32)));
        }
    }
    let mean = v.iter().sum::<f32>() / v.len() as f32;
    v.iter_mut().for_each(|x| *x -= mean);
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    (norm > 0.05).then(|| v.into_iter().map(|x| x / norm).collect())
}

/// Corner strength (smaller eigenvalue of the structure tensor) at integer pixel (x, y).
fn corner_score(img: &Luma, x: usize, y: usize) -> f32 {
    let (mut a, mut b, mut c) = (0.0f32, 0.0f32, 0.0f32);
    for yy in y - 3..=y + 3 {
        for xx in x - 3..=x + 3 {
            let gx = img.at(xx + 1, yy) - img.at(xx - 1, yy);
            let gy = img.at(xx, yy + 1) - img.at(xx, yy - 1);
            a += gx * gx;
            b += gx * gy;
            c += gy * gy;
        }
    }
    let tr = a + c;
    let det = a * c - b * b;
    tr * 0.5 - ((tr * tr * 0.25 - det).max(0.0)).sqrt()
}

fn pick_features(img: &Luma, lo: Vec2, hi: Vec2, cell: usize, max: usize) -> Vec<Feature> {
    let margin = (R + SEARCH + 4) as usize;
    let x0 = (lo.x.max(0.0) as usize).max(margin);
    let y0 = (lo.y.max(0.0) as usize).max(margin);
    let x1 = (hi.x as usize).min(img.w.saturating_sub(margin));
    let y1 = (hi.y as usize).min(img.h.saturating_sub(margin));
    let mut best: Vec<(f32, usize, usize)> = Vec::new();
    let mut cy = y0;
    while cy < y1 {
        let mut cx = x0;
        while cx < x1 {
            let mut top = (0.0f32, 0, 0);
            for y in (cy..(cy + cell).min(y1)).step_by(2) {
                for x in (cx..(cx + cell).min(x1)).step_by(2) {
                    let s = corner_score(img, x, y);
                    if s > top.0 {
                        top = (s, x, y);
                    }
                }
            }
            if top.0 > 0.0 {
                best.push(top);
            }
            cx += cell;
        }
        cy += cell;
    }
    best.sort_by(|a, b| b.0.total_cmp(&a.0));
    // Keep strong corners only (relative to the best ones): flat glass and wall make poor patches.
    let floor = best.get(best.len() / 8).map_or(0.0, |b| b.0) * 0.15;
    best.into_iter()
        .filter(|b| b.0 > floor)
        .take(max)
        .filter_map(|(_, x, y)| {
            let at = Vec2::new(x as f32, y as f32);
            template(img, at).map(|tpl| Feature { at, tpl })
        })
        .collect()
}

/// Best match of `f` near `guess` in `img`: (position, NCC score).
fn search(img: &Luma, f: &Feature, guess: Vec2) -> Option<(Vec2, f32)> {
    let n = ((2 * R + 1) * (2 * R + 1)) as f32;
    let g = guess.round();
    let (gx, gy) = (g.x as i32, g.y as i32);
    if gx - SEARCH - R < 1 || gy - SEARCH - R < 1 || gx + SEARCH + R >= img.w as i32 - 1 || gy + SEARCH + R >= img.h as i32 - 1 {
        return None;
    }
    let side = (2 * SEARCH + 1) as usize;
    let mut scores = vec![-1.0f32; side * side];
    for sy in -SEARCH..=SEARCH {
        for sx in -SEARCH..=SEARCH {
            let (cx, cy) = (gx + sx, gy + sy);
            let (mut st, mut s1, mut s2) = (0.0f32, 0.0f32, 0.0f32);
            let mut i = 0;
            for dy in -R..=R {
                let row = ((cy + dy) as usize) * img.w;
                for dx in -R..=R {
                    let v = img.px[row + (cx + dx) as usize];
                    st += f.tpl[i] * v;
                    s1 += v;
                    s2 += v * v;
                    i += 1;
                }
            }
            let var = (s2 - s1 * s1 / n).max(1e-9);
            scores[((sy + SEARCH) as usize) * side + (sx + SEARCH) as usize] = st / var.sqrt();
        }
    }
    let (bi, &bs) = scores.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
    let (bx, by) = (bi % side, bi / side);
    // Sub-pixel: parabola through the neighbours.
    let sub = |m: f32, c: f32, p: f32| {
        let d = m - 2.0 * c + p;
        if d.abs() > 1e-6 { (0.5 * (m - p) / d).clamp(-0.5, 0.5) } else { 0.0 }
    };
    let ox = if bx > 0 && bx + 1 < side { sub(scores[by * side + bx - 1], bs, scores[by * side + bx + 1]) } else { 0.0 };
    let oy = if by > 0 && by + 1 < side { sub(scores[(by - 1) * side + bx], bs, scores[(by + 1) * side + bx]) } else { 0.0 };
    Some((Vec2::new((gx + bx as i32 - SEARCH) as f32 + ox, (gy + by as i32 - SEARCH) as f32 + oy), bs))
}

/// Least-squares similarity (scale, rotation, translation) mapping `a[i]` onto `b[i]`.
fn fit_similarity(a: &[Vec2], b: &[Vec2]) -> Option<Affine2> {
    let n = a.len();
    if n < 3 {
        return None;
    }
    let ca = a.iter().copied().sum::<Vec2>() / n as f32;
    let cb = b.iter().copied().sum::<Vec2>() / n as f32;
    let (mut sxx, mut sxy, mut var) = (0.0f32, 0.0f32, 0.0f32);
    for (p, q) in a.iter().zip(b) {
        let (p, q) = (*p - ca, *q - cb);
        sxx += p.dot(q);
        sxy += p.perp_dot(q);
        var += p.length_squared();
    }
    if var < 1e-6 {
        return None;
    }
    let (c, s) = (sxx / var, sxy / var);
    let m = Mat2::from_cols(Vec2::new(c, s), Vec2::new(-s, c));
    Some(Affine2::from_mat2_translation(m, cb - m * ca))
}

/// Robust fit: refit without the points that disagree with the majority.
fn robust_fit(a: &[Vec2], b: &[Vec2]) -> Option<(Affine2, usize)> {
    let mut keep: Vec<usize> = (0..a.len()).collect();
    let mut model = None;
    for _ in 0..4 {
        let (pa, pb): (Vec<Vec2>, Vec<Vec2>) = keep.iter().map(|&i| (a[i], b[i])).unzip();
        let m = fit_similarity(&pa, &pb)?;
        let mut res: Vec<(usize, f32)> = (0..a.len()).map(|i| (i, m.transform_point2(a[i]).distance(b[i]))).collect();
        let mut sorted: Vec<f32> = res.iter().map(|r| r.1).collect();
        sorted.sort_by(f32::total_cmp);
        let limit = (sorted[sorted.len() / 2] * 2.5).max(1.0);
        res.retain(|r| r.1 <= limit);
        keep = res.into_iter().map(|r| r.0).collect();
        model = Some(m);
        if keep.len() < 3 {
            break;
        }
    }
    Some((model?, keep.len()))
}

/// Track the camera through `count` frames. `frame(i)` gives frame i as a tracking image (all the
/// same size); `region` is the area to lock onto in the reference frame `reference`. Returns, per
/// frame, the map from reference-frame pixels to that frame's pixels (None where tracking failed).
pub fn track(mut frame: impl FnMut(usize) -> Option<Luma>, count: usize, reference: usize, region: (Vec2, Vec2)) -> Vec<Option<Affine2>> {
    let mut out = vec![None; count];
    let Some(img0) = frame(reference) else { return out };
    let feats = pick_features(&img0, region.0, region.1, 24, 80);
    if feats.len() < 6 {
        return out;
    }
    out[reference] = Some(Affine2::IDENTITY);
    let refs: Vec<Vec2> = feats.iter().map(|f| f.at).collect();
    // Forward from the reference, then backward; each frame starts from its neighbour's motion.
    for dir in [1i64, -1] {
        let mut prev = Affine2::IDENTITY;
        let mut i = reference as i64 + dir;
        while (0..count as i64).contains(&i) {
            let Some(img) = frame(i as usize) else {
                i += dir;
                continue;
            };
            let matches: Vec<Option<(Vec2, f32)>> = std::thread::scope(|s| {
                let chunks: Vec<_> = feats
                    .chunks(feats.len().div_ceil(8))
                    .map(|chunk| {
                        let img = &img;
                        s.spawn(move || chunk.iter().map(|f| search(img, f, prev.transform_point2(f.at))).collect::<Vec<_>>())
                    })
                    .collect();
                chunks.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
            });
            let (a, b): (Vec<Vec2>, Vec<Vec2>) =
                matches.iter().zip(&refs).filter_map(|(m, r)| m.filter(|m| m.1 > 0.75).map(|m| (*r, m.0))).unzip();
            if let Some((m, inliers)) = robust_fit(&a, &b)
                && inliers >= 6
            {
                prev = m;
                out[i as usize] = Some(m);
            }
            i += dir;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A textured image shifted and slightly zoomed is recovered.
    #[test]
    fn recovers_shift_and_zoom() {
        let (w, h) = (240usize, 200usize);
        let tex = |p: Vec2| {
            let (x, y) = (p.x * 0.11, p.y * 0.13);
            ((x.sin() * (y * 1.7).cos() + (x * 0.37 + y * 0.53).sin() * 0.6 + ((x * 2.3).sin() * (y * 2.9).sin()) * 0.4) * 0.25 + 0.5).max(0.0)
        };
        let truth = |i: usize| {
            let k = 1.0 + i as f32 * 0.004;
            Affine2::from_scale_angle_translation(Vec2::splat(k), 0.0, Vec2::new(i as f32 * 1.5, -(i as f32) * 0.8))
        };
        let render = |i: usize| {
            let inv = truth(i).inverse();
            let px = (0..w * h).map(|j| tex(inv.transform_point2(Vec2::new((j % w) as f32, (j / w) as f32)))).collect();
            Luma { w, h, px }
        };
        let got = track(|i| Some(render(i)), 8, 0, (Vec2::new(40.0, 40.0), Vec2::new(200.0, 160.0)));
        for (i, m) in got.iter().enumerate() {
            let m = m.expect("tracked");
            let p = Vec2::new(120.0, 100.0);
            let err = m.transform_point2(p).distance(truth(i).transform_point2(p));
            assert!(err < 0.6, "frame {i}: error {err}");
        }
    }
}
