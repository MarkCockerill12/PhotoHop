//! "Match to scene": fits the six everyday grade sliders (exposure, temperature, tint, contrast,
//! saturation, shadow level) so a layer sits in its scene, for objects of any colour.
//!
//! Why sliders and not per-channel curves: a strongly coloured object (a deep blue alien, a red
//! car) has channel statistics dominated by its own colour. Matching channel means or fitting
//! R/G/B curves then "corrects" the object's colour itself (blue armour turns teal or grey). Here
//! the hue of the object is never targeted. The match only changes:
//! - the colour of the light: measured on near-neutral pixels (greys, whites) of the object and of
//!   the scene, never on coloured ones; an object without neutral pixels is assumed neutral-lit;
//! - the black and white levels (how hazy / contrasty the scene is);
//! - how saturated it is, capped by how saturated the scene ever gets.
//!
//! The AI path fits the same sliders to an AI harmonisation result, pixel by pixel.

use ph_core::look::{LookSliders, from_perceptual, grade_perceptual};
use ph_media::Frame;

/// A linear-RGB pixel sample with a weight.
#[derive(Clone, Copy, Debug)]
pub struct Px {
    pub rgb: [f32; 3],
    pub w: f32,
}

/// Straight-alpha linear pixels of `frame` where `weight(x, y, alpha)` > 0, about `max` of them.
pub fn sample(frame: &Frame, max: usize, weight: impl Fn(usize, usize, f32) -> f32) -> Vec<Px> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let step = (((w * h) as f32 / max as f32).sqrt().ceil() as usize).max(1);
    let mut out = Vec::new();
    for y in (0..h).step_by(step) {
        for x in (0..w).step_by(step) {
            let i = (y * w + x) * 4;
            let a = frame.pixels[i + 3].to_f32();
            if a <= 0.02 {
                continue;
            }
            let wt = weight(x, y, a);
            if wt <= 0.01 {
                continue;
            }
            let rgb = std::array::from_fn(|c| (frame.pixels[i + c].to_f32() / a).max(0.0));
            out.push(Px { rgb, w: wt });
        }
    }
    out
}

/// Oklab from linear sRGB.
fn oklab(c: [f32; 3]) -> [f32; 3] {
    let l = 0.412_221_47 * c[0] + 0.536_332_55 * c[1] + 0.051_445_99 * c[2];
    let m = 0.211_903_5 * c[0] + 0.680_699_5 * c[1] + 0.107_396_96 * c[2];
    let s = 0.088_302_46 * c[0] + 0.281_718_85 * c[1] + 0.629_978_7 * c[2];
    let (l, m, s) = (l.max(0.0).cbrt(), m.max(0.0).cbrt(), s.max(0.0).cbrt());
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// What a match looks at: tone levels, saturation and the colour of the light.
#[derive(Clone, Copy, Debug, Default)]
pub struct Features {
    /// Oklab lightness at the 5th, 50th and 95th percentiles.
    pub black: f32,
    pub mid: f32,
    pub white: f32,
    /// Mean and 95th-percentile Oklab chroma.
    pub chroma: f32,
    pub chroma_hi: f32,
    /// Oklab (a, b) of the light: weighted mean over near-neutral pixels.
    pub cast: [f32; 2],
    /// Share of the weight on near-neutral pixels (how reliable `cast` is).
    pub neutral: f32,
}

fn percentile(v: &mut [(f32, f32)], total: f32, p: f32) -> f32 {
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut acc = 0.0;
    for (x, w) in v.iter() {
        acc += w;
        if acc >= p * total {
            return *x;
        }
    }
    v.last().map_or(0.0, |x| x.0)
}

/// Features of perceptual (sRGB-encoded) pixels with weights.
pub fn features_perceptual(px: &[([f32; 3], f32)]) -> Features {
    let mut ls = Vec::with_capacity(px.len());
    let mut cs = Vec::with_capacity(px.len());
    let (mut total, mut csum, mut nw, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for (p, w) in px {
        let lab = oklab(p.map(from_perceptual));
        let c = (lab[1] * lab[1] + lab[2] * lab[2]).sqrt();
        ls.push((lab[0], *w));
        cs.push((c, *w));
        total += w;
        csum += c * w;
        // Near-neutral, not too dark or blown out: these show the colour of the light.
        let n = w * (-(c / 0.05).powi(2)).exp() * if (0.25..0.97).contains(&lab[0]) { 1.0 } else { 0.0 };
        nw += n;
        na += lab[1] * n;
        nb += lab[2] * n;
    }
    if total <= 0.0 {
        return Features::default();
    }
    Features {
        black: percentile(&mut ls, total, 0.05),
        mid: percentile(&mut ls, total, 0.5),
        white: percentile(&mut ls, total, 0.95),
        chroma: csum / total,
        chroma_hi: percentile(&mut cs, total, 0.95),
        cast: if nw > 0.0 { [na / nw, nb / nw] } else { [0.0; 2] },
        neutral: nw / total,
    }
}

fn graded(px: &[Px], s: &LookSliders, pivot: f32) -> Vec<([f32; 3], f32)> {
    px.iter().map(|p| (grade_perceptual(p.rgb, s, pivot), p.w)).collect()
}

/// Oklab (a, b) a neutral grey ramp gets from the white balance sliders.
fn grey_cast(s: &LookSliders, pivot: f32) -> [f32; 2] {
    let ramp: Vec<([f32; 3], f32)> = (1..8).map(|i| (grade_perceptual([i as f32 / 10.0; 3], s, pivot), 1.0)).collect();
    let (mut a, mut b) = (0.0, 0.0);
    for (p, _) in &ramp {
        let lab = oklab(p.map(from_perceptual));
        a += lab[1];
        b += lab[2];
    }
    [a / ramp.len() as f32, b / ramp.len() as f32]
}

/// The look a subject should get in a scene: targets for its features.
#[derive(Clone, Copy, Debug)]
pub struct Targets {
    /// White-balance shift for neutral pixels (Oklab a, b).
    pub cast_shift: [f32; 2],
    pub black: f32,
    pub mid: f32,
    pub white: f32,
    pub chroma: f32,
}

pub fn targets(subject: &Features, scene: &Features) -> Targets {
    // Light colour: the subject's own neutral pixels show the light it was shot in (e.g. a purple
    // LED room); replace that with the scene's light. No neutral pixels (a blue alien): assume
    // it was neutral-lit and only add the scene's light.
    let own = if subject.neutral > 0.03 { subject.cast } else { [0.0; 2] };
    let scene_cast = if scene.neutral > 0.02 { scene.cast.map(|v| v * 0.85) } else { [0.0; 2] };
    let cast_shift = [scene_cast[0] - own[0], scene_cast[1] - own[1]];
    // Tone: blacks sit at the scene's black level (haze, flare); highlights may stay darker than
    // the scene's (dark objects stay dark) but never brighter; mids move a little towards the scene.
    // Each level moves a bounded amount: a sunny scene without shadows must not wash a dark
    // object out, and a dark scene must not crush a bright one.
    let black = subject.black + ((scene.black - subject.black) * 0.9).clamp(-0.08, 0.12);
    let dw = if subject.white > scene.white { (scene.white - subject.white) * 0.75 } else { (scene.white - subject.white) * 0.25 };
    let white = (subject.white + dw.clamp(-0.2, 0.1)).max(black + 0.15);
    let mid = subject.mid + ((scene.mid - subject.mid) * 0.2).clamp(-0.08, 0.08);
    // Saturation: a gentle pull towards the scene's (a scene's saturation says more about its
    // camera and grade than about this object, so a blue object in a grey place stays blue), and
    // the most saturated parts not far beyond the most saturated things in the scene.
    let mut r = (scene.chroma.max(1e-3) / subject.chroma.max(1e-3)).powf(0.25);
    let cap = scene.chroma_hi * 1.5 / subject.chroma_hi.max(1e-3);
    if r > cap {
        r = r.min((r + cap) * 0.5);
    }
    let chroma = subject.chroma * r.clamp(0.6, 1.2);
    Targets { cast_shift, black, mid, white, chroma }
}

const RANGES: [(f32, f32); 6] = [(-3.0, 3.0), (-2.0, 2.0), (-2.0, 2.0), (0.5, 1.6), (0.3, 1.5), (-0.25, 0.45)];

fn clamp(a: [f32; 6]) -> [f32; 6] {
    std::array::from_fn(|i| a[i].clamp(RANGES[i].0, RANGES[i].1))
}

/// Solve `a x = b` (n×n, row-major) by Gaussian elimination with partial pivoting.
fn solve(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Option<Vec<f64>> {
    for col in 0..n {
        let piv = (col..n).max_by(|&i, &j| a[i * n + col].abs().total_cmp(&a[j * n + col].abs()))?;
        if a[piv * n + col].abs() < 1e-12 {
            return None;
        }
        for k in 0..n {
            a.swap(col * n + k, piv * n + k);
        }
        b.swap(col, piv);
        for row in col + 1..n {
            let f = a[row * n + col] / a[col * n + col];
            for k in col..n {
                a[row * n + k] -= f * a[col * n + k];
            }
            b[row] -= f * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|k| a[row * n + k] * x[k]).sum();
        x[row] = (b[row] - s) / a[row * n + row];
    }
    Some(x)
}

/// Levenberg–Marquardt over the slider indices in `which` (the others stay at `start`), with the
/// sliders kept inside their ranges. `resid` returns the residual vector to minimise.
fn least_squares(start: [f32; 6], which: &[usize], iters: usize, resid: impl Fn(&[f32; 6]) -> Vec<f32>) -> [f32; 6] {
    let n = which.len();
    let cost = |r: &[f32]| r.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
    let mut x = clamp(start);
    let mut r = resid(&x);
    let mut c = cost(&r);
    let mut lambda = 1e-2;
    for _ in 0..iters {
        // Numeric Jacobian (central differences).
        let h = 1e-3f32;
        let cols: Vec<Vec<f32>> = which
            .iter()
            .map(|&i| {
                let (mut a, mut b) = (x, x);
                a[i] += h;
                b[i] -= h;
                resid(&a).iter().zip(resid(&b)).map(|(p, q)| (p - q) / (2.0 * h)).collect()
            })
            .collect();
        let mut jtj = vec![0.0f64; n * n];
        let mut jtr = vec![0.0f64; n];
        for i in 0..n {
            for k in 0..n {
                jtj[i * n + k] = cols[i].iter().zip(&cols[k]).map(|(p, q)| (*p as f64) * (*q as f64)).sum();
            }
            jtr[i] = -cols[i].iter().zip(&r).map(|(p, q)| (*p as f64) * (*q as f64)).sum::<f64>();
        }
        let mut stepped = false;
        for _ in 0..8 {
            let mut a = jtj.clone();
            for i in 0..n {
                a[i * n + i] += lambda * (jtj[i * n + i] + 1e-6);
            }
            let Some(d) = solve(a, jtr.clone(), n) else {
                lambda *= 10.0;
                continue;
            };
            let mut y = x;
            for (k, &i) in which.iter().enumerate() {
                y[i] += d[k] as f32;
            }
            y = clamp(y);
            let ry = resid(&y);
            let cy = cost(&ry);
            if cy < c {
                (x, r, c) = (y, ry, cy);
                lambda = (lambda * 0.3).max(1e-7);
                stepped = true;
                break;
            }
            lambda *= 10.0;
        }
        if !stepped || c < 1e-12 {
            break;
        }
    }
    x
}

/// Fit the sliders so `subject` pixels get the `targets`. `start` = the user's current sliders.
pub fn fit_smart(subject: &[Px], tg: &Targets, start: LookSliders, pivot: f32) -> LookSliders {
    // 1. Light colour: white-balance sliders so a neutral grey gets the wanted shift.
    let g0 = grey_cast(&start, pivot);
    let want = [g0[0] + tg.cast_shift[0], g0[1] + tg.cast_shift[1]];
    let wb = least_squares(start.to_array(), &[1, 2], 30, |a| {
        let g = grey_cast(&LookSliders::from_array(*a), pivot);
        vec![(g[0] - want[0]) * 10.0, (g[1] - want[1]) * 10.0]
    });
    // 2. Tone and saturation on the real pixels, white balance held. Small pulls towards the
    //    starting sliders keep the solution unique (exposure, contrast and shadow level overlap).
    let s0 = start.to_array();
    let x = least_squares(wb, &[0, 3, 4, 5], 40, |a| {
        let f = features_perceptual(&graded(subject, &LookSliders::from_array(*a), pivot));
        vec![
            2.0 * (f.black - tg.black),
            1.4 * (f.white - tg.white),
            (f.mid - tg.mid),
            1.7 * (f.chroma - tg.chroma) * 4.0,
            0.02 * (a[0] - s0[0]),
            0.02 * (a[3] - s0[3]),
            0.05 * (a[5] - s0[5]),
        ]
    });
    LookSliders::from_array(x)
}

/// Fit the sliders to an AI suggestion: `before` linear pixels should look like `after`
/// (perceptual 0..1), pixel for pixel. Only global slider moves are possible, so the AI can never
/// add detail or change one part of the object differently from another.
pub fn fit_to_example(before: &[Px], after: &[[f32; 3]], start: LookSliders, pivot: f32) -> LookSliders {
    let x = least_squares(start.to_array(), &[0, 1, 2, 3, 4, 5], 60, |a| {
        let s = LookSliders::from_array(*a);
        let mut r = Vec::with_capacity(before.len() * 3);
        for (p, q) in before.iter().zip(after) {
            let g = grade_perceptual(p.rgb, &s, pivot);
            let w = p.w.sqrt();
            r.extend((0..3).map(|c| w * (g[c] - q[c])));
        }
        r
    });
    LookSliders::from_array(x)
}

/// One line describing what the match changed, for the status bar.
pub fn describe(from: &LookSliders, to: &LookSliders) -> String {
    let (a, b) = (from.to_array(), to.to_array());
    let mut parts = Vec::new();
    for (i, name) in LookSliders::NAMES.iter().enumerate() {
        let d = b[i] - a[i];
        if d.abs() >= 0.02 {
            parts.push(format!("{name} {d:+.2}"));
        }
    }
    if parts.is_empty() { "already matched — nothing to change".into() } else { parts.join(", ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(rgb: [f32; 3]) -> Px {
        Px { rgb, w: 1.0 }
    }

    #[test]
    fn blue_object_stays_blue() {
        // Deep blue armour with dark grey parts, into a warm hazy scene.
        let subject: Vec<Px> = (0..200)
            .map(|i| if i % 4 == 0 { px([0.02, 0.02, 0.02]) } else { px([0.02, 0.05, 0.6 * (0.5 + (i % 7) as f32 / 14.0)]) })
            .collect();
        let scene: Vec<([f32; 3], f32)> = (0..200)
            .map(|i| {
                let v = 0.25 + (i % 10) as f32 / 20.0;
                ([v * 1.05, v, v * 0.85].map(ph_core::look::to_perceptual), 1.0)
            })
            .collect();
        let fs = features_perceptual(&graded(&subject, &LookSliders::default(), 0.435));
        let ft = features_perceptual(&scene);
        let tg = targets(&fs, &ft);
        let s = fit_smart(&subject, &tg, LookSliders::default(), 0.435);
        // Blue keeps being the dominant channel of the armour.
        let g = grade_perceptual([0.02, 0.05, 0.45], &s, 0.435);
        assert!(g[2] > g[0] * 1.5 && g[2] > g[1] * 1.5, "{g:?} {s:?}");
        // Blacks lifted towards the hazy scene, saturation reduced.
        assert!(s.black > 0.0 && s.saturation < 1.0, "{s:?}");
    }

    #[test]
    fn purple_room_cast_is_removed() {
        // A grey shirt lit purple, into a neutral scene: temperature/tint must neutralise it.
        let subject: Vec<Px> = (0..100).map(|i| px([0.3 + i as f32 * 0.002, 0.2, 0.35])).collect();
        let scene: Vec<([f32; 3], f32)> = (0..100).map(|i| ([0.3 + i as f32 * 0.003; 3].map(ph_core::look::to_perceptual), 1.0)).collect();
        let fs = features_perceptual(&graded(&subject, &LookSliders::default(), 0.435));
        let ft = features_perceptual(&scene);
        let s = fit_smart(&subject, &targets(&fs, &ft), LookSliders::default(), 0.435);
        let g = grade_perceptual([0.35, 0.2, 0.35], &s, 0.435);
        let lab = oklab(g.map(from_perceptual));
        let before = oklab([0.35, 0.2, 0.35]);
        let c = |l: [f32; 3]| (l[1] * l[1] + l[2] * l[2]).sqrt();
        assert!(c(lab) < c(before) * 0.6, "{lab:?} vs {before:?} ({s:?})");
    }

    #[test]
    fn example_fit_recovers_sliders() {
        let truth = LookSliders { exposure: 0.4, temperature: 0.5, saturation: 0.7, ..LookSliders::default() };
        let before: Vec<Px> = (0..300).map(|i| px([(i % 10) as f32 / 12.0 + 0.02, (i % 7) as f32 / 9.0 + 0.02, (i % 13) as f32 / 15.0 + 0.02])).collect();
        let after: Vec<[f32; 3]> = before.iter().map(|p| grade_perceptual(p.rgb, &truth, 0.435)).collect();
        let s = fit_to_example(&before, &after, LookSliders::default(), 0.435);
        // Exposure, contrast and shadow level overlap, so compare the colours they produce.
        let worst = before.iter().zip(&after).map(|(p, q)| {
            let g = grade_perceptual(p.rgb, &s, 0.435);
            (0..3).map(|c| (g[c] - q[c]).abs()).fold(0.0f32, f32::max)
        }).fold(0.0f32, f32::max);
        assert!(worst < 0.02 && (s.temperature - 0.5).abs() < 0.1, "{worst} {s:?}");
    }
}
