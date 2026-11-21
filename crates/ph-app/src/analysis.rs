//! Non-AI image measurements used for automatic matching: noise level, fine-detail energy
//! (to suggest grain and blur so a layer matches its background) and a simple light estimate.

use ph_media::Frame;
use ph_media::color::linear_to_srgb;

/// Perceptual luma image with per-pixel weight (alpha × mask), resampled to `scale` × the frame size.
pub struct Luma {
    pub w: usize,
    pub h: usize,
    pub v: Vec<f32>,
    pub wt: Vec<f32>,
}

impl Luma {
    /// Resample `frame` by `scale` (nearest; capped at 640 px on the long side) to perceptual luma.
    pub fn from_frame(frame: &Frame, scale: f32, mask: &dyn Fn(f32, f32) -> f32) -> Self {
        let (fw, fh) = (frame.width as f32, frame.height as f32);
        let s = scale.min(640.0 / fw.max(fh)).max(1e-3);
        let (w, h) = (((fw * s).round() as usize).max(4), ((fh * s).round() as usize).max(4));
        let mut v = vec![0.0; w * h];
        let mut wt = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let (u, vv) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                let sx = ((u * fw) as usize).min(frame.width as usize - 1);
                let sy = ((vv * fh) as usize).min(frame.height as usize - 1);
                let i = (sy * frame.width as usize + sx) * 4;
                let a = frame.pixels[i + 3].to_f32();
                let inv = if a > 1e-4 { 1.0 / a } else { 0.0 };
                let [r, g, b] = [0, 1, 2].map(|c| linear_to_srgb((frame.pixels[i + c].to_f32() * inv).max(0.0)));
                v[y * w + x] = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                wt[y * w + x] = a * mask(u, vv);
            }
        }
        Self { w, h, v, wt }
    }

    fn box_blur(&self, r: usize) -> Vec<f32> {
        let (w, h) = (self.w, self.h);
        let mut tmp = vec![0.0; w * h];
        let mut out = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
                tmp[y * w + x] = self.v[y * w + a..=y * w + b].iter().sum::<f32>() / (b - a + 1) as f32;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
                out[y * w + x] = (a..=b).map(|yy| tmp[yy * w + x]).sum::<f32>() / (b - a + 1) as f32;
            }
        }
        out
    }

    fn blurred(&self, radius: f32) -> Luma {
        if radius < 0.5 {
            return Luma { w: self.w, h: self.h, v: self.v.clone(), wt: self.wt.clone() };
        }
        // Three box passes ≈ Gaussian.
        let r = (radius / 1.7).round().max(1.0) as usize;
        let mut cur = Luma { w: self.w, h: self.h, v: self.v.clone(), wt: self.wt.clone() };
        for _ in 0..3 {
            cur.v = cur.box_blur(r);
        }
        cur
    }

    /// Mean absolute high-pass (|L − box₂(L)|) over solid pixels: overall fine-detail energy.
    pub fn detail(&self) -> f32 {
        let lo = self.box_blur(2);
        let (mut s, mut n) = (0.0, 0.0);
        for i in 0..self.v.len() {
            if self.wt[i] > 0.9 {
                s += (self.v[i] - lo[i]).abs();
                n += 1.0;
            }
        }
        if n > 0.0 { s / n } else { 0.0 }
    }

    /// Noise σ estimated from the median absolute 3×3 high-pass in the flattest third of pixels.
    pub fn noise(&self) -> f32 {
        let lo = self.box_blur(1);
        let wide = self.box_blur(4);
        let mut cand: Vec<(f32, f32)> =
            (0..self.v.len()).filter(|&i| self.wt[i] > 0.9).map(|i| ((lo[i] - wide[i]).abs(), (self.v[i] - lo[i]).abs())).collect();
        if cand.len() < 16 {
            return 0.0;
        }
        cand.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut flat: Vec<f32> = cand[..cand.len() / 3].iter().map(|c| c.1).collect();
        flat.sort_by(f32::total_cmp);
        flat[flat.len() / 2] * 1.4826
    }
}

impl Luma {
    fn sample(&self, x: f32, y: f32) -> Option<f32> {
        if x < 0.0 || y < 0.0 || x > (self.w - 1) as f32 || y > (self.h - 1) as f32 {
            return None;
        }
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let i = |xx: usize, yy: usize| yy * self.w + xx;
        if self.wt[i(x0, y0)] < 0.9 || self.wt[i(x1, y1)] < 0.9 {
            return None;
        }
        let top = self.v[i(x0, y0)] * (1.0 - fx) + self.v[i(x1, y0)] * fx;
        let bot = self.v[i(x0, y1)] * (1.0 - fx) + self.v[i(x1, y1)] * fx;
        Some(top * (1.0 - fy) + bot * fy)
    }
}

/// Motion blur in `l`: (direction in degrees, 0 = horizontal, y down; length in `l` pixels).
///
/// A streak of length L along θ turns the derivative along θ into a pair of opposite spikes L
/// apart, so its autocorrelation along θ dips strongly negative at lag L. The angle with the
/// deepest dip wins; no dip below −0.15 means no motion blur.
pub fn estimate_motion(l: &Luma) -> Option<(f32, f32)> {
    let max_lag = 48usize.min(l.w.min(l.h) / 4);
    if max_lag < 4 {
        return None;
    }
    let mut scores = [(0.0f32, 0.0f32); 36]; // (score, length) per 5°
    for (step, slot) in scores.iter_mut().enumerate() {
        let ang = step as f32 * 5.0;
        let (dx, dy) = (ang.to_radians().cos(), ang.to_radians().sin());
        // Directional derivative on a sparse grid.
        let deriv = |x: f32, y: f32| -> Option<f32> { Some(l.sample(x + dx * 0.5, y + dy * 0.5)? - l.sample(x - dx * 0.5, y - dy * 0.5)?) };
        let mut r = vec![0.0f64; max_lag + 1];
        let mut n = vec![0u32; max_lag + 1];
        for y in (max_lag..l.h.saturating_sub(max_lag)).step_by(2) {
            for x in (max_lag..l.w.saturating_sub(max_lag)).step_by(2) {
                let (fx, fy) = (x as f32, y as f32);
                let Some(d0) = deriv(fx, fy) else { continue };
                for (k, (rk, nk)) in r.iter_mut().zip(n.iter_mut()).enumerate() {
                    if let Some(dk) = deriv(fx + dx * k as f32, fy + dy * k as f32) {
                        *rk += (d0 * dk) as f64;
                        *nk += 1;
                    }
                }
            }
        }
        if n[0] < 200 || r[0] <= 1e-12 {
            *slot = (0.0, 0.0);
            continue;
        }
        let r0 = r[0] / n[0] as f64;
        let (mut score, mut lag) = (0.0f64, 0usize);
        for k in 3..=max_lag {
            if n[k] > 50 {
                let v = r[k] / n[k] as f64 / r0;
                if v < score {
                    (score, lag) = (v, k);
                }
            }
        }
        *slot = (score as f32, lag as f32);
    }
    // Deepest dip, which must be directional: clearly deeper than at right angles.
    let (i, &(score, len)) = scores.iter().enumerate().min_by(|a, b| a.1.0.total_cmp(&b.1.0))?;
    let perp = scores[(i + 18) % 36].0;
    (score < -0.15 && perp > score * 0.5).then_some((i as f32 * 5.0, len))
}

/// Blur radius (in the luma image's pixels) that brings `subject`'s detail down to `target`.
pub fn blur_to_match(subject: &Luma, target_detail: f32) -> f32 {
    let base = subject.detail();
    if base <= target_detail * 1.05 {
        return 0.0;
    }
    let mut best = (0.0, (base - target_detail).abs());
    for i in 1..=24 {
        let r = i as f32 * 0.5;
        let d = subject.blurred(r).detail();
        let err = (d - target_detail).abs();
        if err < best.1 {
            best = (r, err);
        }
        if d < target_detail {
            break;
        }
    }
    best.0
}

/// A rough light estimate from a background image (no AI).
#[derive(Debug, Clone, Copy)]
pub struct LightEstimate {
    /// Normalised colour of the brightest pixels (linear RGB, max channel 1).
    pub color: [f32; 3],
    /// Shadow level relative to the highlights (0..1).
    pub ambient: f32,
    /// Direction of the bright region from the image centre (degrees, 0 = right, 90 = up).
    pub azimuth: f32,
}

pub fn estimate_light(frame: &Frame) -> LightEstimate {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let step = ((w * h) as f32 / 120_000.0).sqrt().ceil().max(1.0) as usize;
    let mut px: Vec<(f32, [f32; 3], f32, f32)> = Vec::new();
    for y in (0..h).step_by(step) {
        for x in (0..w).step_by(step) {
            let i = (y * w + x) * 4;
            let a = frame.pixels[i + 3].to_f32();
            if a < 0.5 {
                continue;
            }
            let c = [0, 1, 2].map(|k| frame.pixels[i + k].to_f32() / a);
            let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            px.push((l, c, x as f32 / w as f32 - 0.5, 0.5 - y as f32 / h as f32));
        }
    }
    if px.is_empty() {
        return LightEstimate { color: [1.0; 3], ambient: 0.5, azimuth: 90.0 };
    }
    px.sort_by(|a, b| a.0.total_cmp(&b.0));
    let top = &px[px.len() * 97 / 100..];
    let mut col = [0.0f32; 3];
    let (mut dx, mut dy) = (0.0, 0.0);
    for (_, c, x, y) in top {
        for k in 0..3 {
            col[k] += c[k];
        }
        dx += x;
        dy += y;
    }
    let m = col.iter().cloned().fold(1e-6, f32::max);
    let bright = top.iter().map(|p| p.0).sum::<f32>() / top.len() as f32;
    let median = px[px.len() / 2].0;
    LightEstimate {
        color: col.map(|v| v / m),
        // Square root: bright skies dominate the highlight sample, so the raw ratio is too harsh.
        ambient: (median / bright.max(1e-4)).sqrt().clamp(0.25, 0.9),
        azimuth: dy.atan2(dx).to_degrees(),
    }
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    fn texture(w: usize, h: usize) -> Vec<f32> {
        // Deterministic blobby texture with edges in every direction.
        let mut v = vec![0.0f32; w * h];
        let mut seed = 12345u32;
        for _ in 0..400 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let (cx, cy) = ((seed >> 8) as usize % w, (seed >> 4) as usize % h);
            let r = 3 + (seed >> 20) as usize % 9;
            let val = ((seed >> 12) % 100) as f32 / 100.0;
            for y in cy.saturating_sub(r)..(cy + r).min(h) {
                for x in cx.saturating_sub(r)..(cx + r).min(w) {
                    v[y * w + x] = val;
                }
            }
        }
        v
    }

    fn motion_blur(v: &[f32], w: usize, h: usize, ang: f32, len: usize) -> Vec<f32> {
        let (dx, dy) = (ang.to_radians().cos(), ang.to_radians().sin());
        let mut out = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut s = 0.0;
                for k in 0..len {
                    let t = k as f32 - (len as f32 - 1.0) / 2.0;
                    let (sx, sy) = ((x as f32 + dx * t).round().clamp(0.0, (w - 1) as f32), (y as f32 + dy * t).round().clamp(0.0, (h - 1) as f32));
                    s += v[sy as usize * w + sx as usize];
                }
                out[y * w + x] = s / len as f32;
            }
        }
        out
    }

    #[test]
    fn finds_direction_and_length_of_a_motion_blur() {
        let (w, h) = (320, 240);
        let base = texture(w, h);
        let l = Luma { w, h, v: motion_blur(&base, w, h, 30.0, 14), wt: vec![1.0; w * h] };
        let (ang, len) = estimate_motion(&l).expect("motion found");
        assert!((ang - 30.0).abs() <= 10.0, "angle {ang}");
        assert!((len - 14.0).abs() <= 3.0, "length {len}");
        let sharp = Luma { w, h, v: base, wt: vec![1.0; w * h] };
        assert!(estimate_motion(&sharp).is_none(), "no motion in a sharp image");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use half::f16;

    fn frame_from(w: usize, h: usize, f: impl Fn(usize, usize) -> f32) -> Frame {
        let px: Vec<f16> = (0..w * h)
            .flat_map(|i| {
                let v = f(i % w, i / w);
                [v, v, v, 1.0].map(f16::from_f32)
            })
            .collect();
        Frame::new(w as u32, h as u32, px)
    }

    #[test]
    fn noise_and_blur_estimates() {
        // Deterministic pseudo-noise on mid grey.
        let noisy = frame_from(128, 128, |x, y| 0.2 + (((x * 7919 + y * 104729) % 97) as f32 / 97.0 - 0.5) * 0.06);
        let clean = frame_from(128, 128, |_, _| 0.2);
        let one = |_: f32, _: f32| 1.0;
        let (ln, lc) = (Luma::from_frame(&noisy, 1.0, &one), Luma::from_frame(&clean, 1.0, &one));
        assert!(ln.noise() > 0.01 && lc.noise() < 1e-4, "{} {}", ln.noise(), lc.noise());
        // A sharp checkerboard needs blur to match a soft target.
        let sharp = frame_from(96, 96, |x, y| if (x / 3 + y / 3) % 2 == 0 { 0.05 } else { 0.6 });
        let ls = Luma::from_frame(&sharp, 1.0, &one);
        let r = blur_to_match(&ls, ls.detail() * 0.3);
        assert!(r >= 0.5, "radius {r}");
    }

    #[test]
    fn light_from_bright_top_left() {
        let f = frame_from(64, 64, |x, y| if x < 16 && y < 16 { 1.0 } else { 0.1 });
        let e = estimate_light(&f);
        assert!(e.azimuth > 100.0 && e.azimuth < 170.0, "azimuth {}", e.azimuth);
        assert!(e.ambient < 0.5);
    }
}

#[cfg(test)]
mod real_project {
    /// Opt-in: `PHOTOHOP_TEST_PROJECT=project.phop cargo test -p photohop match_real -- --nocapture`.
    #[test]
    fn match_real() {
        let Some(path) = std::env::var_os("PHOTOHOP_TEST_PROJECT") else {
            return;
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut app = crate::app::App::new(&egui::Context::default(), &device, &queue);
        app.open(std::path::Path::new(&path)).unwrap();
        app.selected = app.doc.layers.last().map(|l| l.id);
        eprintln!("{}", app.match_detail().unwrap());
        eprintln!("{}", app.match_light().unwrap());
        app.match_colour().unwrap();
        let g = &app.doc.layers.last().unwrap().grade;
        eprintln!("surroundings match: scale {:?} offset {:?}", g.match_scale, g.match_offset);
        if let Some(out) = std::env::var_os("PHOTOHOP_TEST_SAVE") {
            app.save_to(std::path::Path::new(&out)).unwrap();
        }
    }
}

#[cfg(test)]
mod real_match {
    /// Opt-in: `PHOTOHOP_TEST_MATCH=in.phop:out.phop` runs Match the camera on the top layer.
    #[test]
    fn match_camera_real() {
        let Some(spec) = std::env::var("PHOTOHOP_TEST_MATCH").ok() else {
            return;
        };
        let (inp, out) = spec.rsplit_once(':').unwrap();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut app = crate::app::App::new(&egui::Context::default(), &device, &queue);
        app.open(std::path::Path::new(inp)).unwrap();
        app.selected = app.doc.layers.last().map(|l| l.id);
        let _ = app.layer_frame(app.selected.unwrap());
        std::thread::sleep(std::time::Duration::from_millis(500));
        eprintln!("{}", app.match_detail().unwrap());
        app.save_to(std::path::Path::new(out)).unwrap();
    }
}
