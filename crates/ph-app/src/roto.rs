//! Rasterising bezier roto shapes into mask coverage. Nonzero winding (self-overlapping shapes
//! stay filled), 4 sub-scanlines per pixel with exact horizontal span coverage, so a 4K mask with
//! a few hundred edges fills in milliseconds and can be redone every frame of an animated shape.

use ph_core::{Layer, RotoMode};

const SUB: usize = 4;

/// Coverage (0..1) of a closed polygon given in mask pixel coordinates.
pub fn fill(w: u32, h: u32, poly: &[[f32; 2]]) -> Vec<f32> {
    let (wu, hu) = (w as usize, h as usize);
    let mut out = vec![0.0f32; wu * hu];
    if poly.len() < 3 || w == 0 || h == 0 {
        return out;
    }
    let edges: Vec<([f32; 2], [f32; 2], i32)> = (0..poly.len())
        .filter_map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if a[1] == b[1] {
                None
            } else if a[1] < b[1] {
                Some((a, b, 1))
            } else {
                Some((b, a, -1))
            }
        })
        .collect();
    let ymin = poly.iter().map(|p| p[1]).fold(f32::MAX, f32::min).max(0.0);
    let ymax = poly.iter().map(|p| p[1]).fold(f32::MIN, f32::max).min(h as f32);
    let mut xs: Vec<(f32, i32)> = Vec::new();
    let mut row = vec![0.0f32; wu + 1];
    let weight = 1.0 / SUB as f32;
    for y in (ymin.floor() as usize)..(ymax.ceil() as usize).min(hu) {
        row.iter_mut().for_each(|v| *v = 0.0);
        let mut any = false;
        for s in 0..SUB {
            let sy = y as f32 + (s as f32 + 0.5) / SUB as f32;
            xs.clear();
            for (a, b, dir) in &edges {
                if sy >= a[1] && sy < b[1] {
                    let x = a[0] + (sy - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                    xs.push((x, *dir));
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|p, q| p.0.total_cmp(&q.0));
            let mut wind = 0;
            for k in 0..xs.len() - 1 {
                wind += xs[k].1;
                if wind == 0 {
                    continue;
                }
                let (x0, x1) = (xs[k].0.clamp(0.0, w as f32), xs[k + 1].0.clamp(0.0, w as f32));
                if x1 <= x0 {
                    continue;
                }
                any = true;
                let (i0, i1) = (x0.floor() as usize, x1.floor() as usize);
                if i0 == i1 {
                    row[i0] += (x1 - x0) * weight;
                } else {
                    row[i0] += (i0 as f32 + 1.0 - x0) * weight;
                    for v in &mut row[i0 + 1..i1] {
                        *v += weight;
                    }
                    if i1 < wu {
                        row[i1] += (x1 - i1 as f32) * weight;
                    }
                }
            }
        }
        if any {
            for (o, v) in out[y * wu..(y + 1) * wu].iter_mut().zip(&row) {
                *o = v.min(1.0);
            }
        }
    }
    out
}

/// Combined coverage of the layer's roto shapes at `t`, at mask resolution `w`×`h` for a layer
/// whose source is `src` pixels. `None` when the layer has no active shapes.
pub fn layer_coverage(layer: &Layer, t: f64, w: u32, h: u32, src: [f32; 2]) -> Option<Vec<f32>> {
    let shapes: Vec<_> = layer.roto.iter().filter(|s| s.active()).collect();
    if shapes.is_empty() {
        return None;
    }
    let (sx, sy) = (w as f32 / src[0].max(1.0), h as f32 / src[1].max(1.0));
    let start: f32 = if shapes[0].mode == RotoMode::Subtract { 1.0 } else { 0.0 };
    let mut acc = vec![start; (w * h) as usize];
    for s in shapes {
        let poly: Vec<[f32; 2]> = s.polygon(t, 24).iter().map(|p| [p[0] * sx, p[1] * sy]).collect();
        let mut cov = fill(w, h, &poly);
        let feather = s.feather.at(t) * sx;
        if feather > 0.5 {
            crate::selection::feather(&mut cov, w, h, feather);
        }
        let op = s.opacity.at(t).clamp(0.0, 1.0);
        for (a, c) in acc.iter_mut().zip(&cov) {
            let c = if s.invert { 1.0 - c } else { *c } * op;
            *a = match s.mode {
                RotoMode::Add => (*a).max(c),
                RotoMode::Subtract => (*a).min(1.0 - c),
            };
        }
    }
    Some(acc)
}

/// A hash of everything that changes the roto coverage at `t` (cache key for the GPU mask).
pub fn signature(layer: &Layer, t: f64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hsh = std::collections::hash_map::DefaultHasher::new();
    for s in layer.roto.iter().filter(|s| s.active()) {
        for p in &s.path.at(t).0 {
            for v in p.p.iter().chain(&p.h_in).chain(&p.h_out) {
                v.to_bits().hash(&mut hsh);
            }
        }
        (s.feather.at(t).to_bits(), s.opacity.at(t).to_bits(), s.invert, s.mode as u8).hash(&mut hsh);
    }
    hsh.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_coverage_with_fractional_edges() {
        let c = fill(10, 10, &[[2.5, 2.0], [6.0, 2.0], [6.0, 6.0], [2.5, 6.0]]);
        assert_eq!(c[3 * 10 + 4], 1.0);
        assert!((c[3 * 10 + 2] - 0.5).abs() < 1e-5, "{}", c[3 * 10 + 2]);
        assert_eq!(c[3 * 10 + 6], 0.0);
        assert_eq!(c[0], 0.0);
    }

    #[test]
    fn overlapping_loops_stay_filled() {
        // The same square twice (same direction): nonzero winding keeps it filled, even-odd wouldn't.
        let sq = [[1.0, 1.0], [5.0, 1.0], [5.0, 5.0], [1.0, 5.0]];
        let poly: Vec<[f32; 2]> = sq.iter().chain(sq.iter()).copied().collect();
        assert_eq!(fill(8, 8, &poly)[3 * 8 + 3], 1.0);
    }

    #[test]
    fn matches_supersampled_reference_on_a_triangle() {
        let tri = [[0.3, 0.2], [17.7, 3.9], [6.1, 15.4]];
        let fast = fill(20, 20, &tri);
        let poly: Vec<glam::Vec2> = tri.iter().map(|p| glam::Vec2::new(p[0], p[1])).collect();
        let reference = crate::selection::polygon(20, 20, &poly);
        let err: f32 = fast.iter().zip(&reference).map(|(a, b)| (a - b).abs()).sum::<f32>() / 400.0;
        assert!(err < 0.02, "mean error {err}");
    }
}
