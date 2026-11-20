//! Reads the scene's own shadows from a background image: a light map (1 = sunlit, 0 = in shadow)
//! plus the colour ratio between shadowed and lit surfaces. Used by "Shadows on me" to lay the
//! same sun-through-leaves pattern onto the subject, and to keep the subject's cast shadow from
//! darkening ground that was already in shadow.
//!
//! Method: luminance is compared with its local upper envelope (a sliding maximum, then a box
//! blur, over ~6 % of the image), so the ratio is ~1 on sunlit ground and drops inside shadows
//! regardless of the ground's own brightness. The shadow level is the low percentile of that
//! ratio; the lit/shadow split sits halfway between it and 1, softened by a smoothstep.

use ph_media::Frame;
use std::collections::VecDeque;

pub struct LightMap {
    pub width: u32,
    pub height: u32,
    /// Row-major, 0..1.
    pub lit: Vec<f32>,
    /// Shadowed / lit colour ratio (linear RGB), e.g. bluish under open sky.
    pub shade: [f32; 3],
}

/// Longest side of the analysis image (shadow patterns are low frequency).
const MAX_SIDE: u32 = 768;

fn downsample(frame: &Frame) -> (u32, u32, Vec<[f32; 3]>) {
    let (fw, fh) = (frame.width, frame.height);
    let k = (fw.max(fh) as f32 / MAX_SIDE as f32).ceil().max(1.0) as u32;
    let (w, h) = ((fw / k).max(1), (fh / k).max(1));
    let mut out = vec![[0.0f32; 3]; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for yy in y * k..((y + 1) * k).min(fh) {
                for xx in x * k..((x + 1) * k).min(fw) {
                    let i = ((yy * fw + xx) * 4) as usize;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += frame.pixels[i + c].to_f32();
                    }
                }
            }
            // Un-premultiply (transparent areas count as unlit black, then get ignored by percentiles).
            let a = acc[3].max(1e-6);
            out[(y * w + x) as usize] = [acc[0] / a, acc[1] / a, acc[2] / a].map(|v| v.max(0.0));
        }
    }
    (w, h, out)
}

/// Sliding-window maximum along one axis (monotonic deque, O(n)).
fn running_max(src: &[f32], dst: &mut [f32], n: usize, stride: usize, count: usize, outer: usize, r: usize) {
    let mut q: VecDeque<usize> = VecDeque::new();
    for o in 0..outer {
        let base = o * if stride == 1 { n } else { 1 };
        q.clear();
        let at = |i: usize| src[base + i * stride];
        let mut next = 0;
        for i in 0..count {
            while next < count && next <= i + r {
                while q.back().is_some_and(|&b| at(b) <= at(next)) {
                    q.pop_back();
                }
                q.push_back(next);
                next += 1;
            }
            while q.front().is_some_and(|&f| f + r < i) {
                q.pop_front();
            }
            dst[base + i * stride] = at(*q.front().expect("window is never empty"));
        }
    }
}

fn box_blur(v: &mut [f32], w: usize, h: usize, r: usize) {
    let mut tmp = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            tmp[y * w + x] = v[y * w + a..=y * w + b].iter().sum::<f32>() / (b - a + 1) as f32;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
            v[y * w + x] = (a..=b).map(|yy| tmp[yy * w + x]).sum::<f32>() / (b - a + 1) as f32;
        }
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Estimate the light map of `frame` (a linear, premultiplied composite of the scene behind the
/// subject). `threshold` (-1..1) moves the lit/shadow split.
pub fn estimate(frame: &Frame, threshold: f32) -> LightMap {
    let (w, h, rgb) = downsample(frame);
    let (wu, hu) = (w as usize, h as usize);
    let luma: Vec<f32> = rgb.iter().map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]).collect();
    let r = ((w.max(h) as f32 * 0.06).round() as usize).max(2);
    let mut tmp = vec![0.0; wu * hu];
    let mut env = vec![0.0; wu * hu];
    running_max(&luma, &mut tmp, wu, 1, wu, hu, r);
    running_max(&tmp, &mut env, wu, wu, hu, wu, r);
    box_blur(&mut env, wu, hu, r);
    let ratio: Vec<f32> = luma.iter().zip(&env).map(|(l, e)| (l / e.max(1e-4)).clamp(0.0, 1.5)).collect();
    let mut sorted: Vec<f32> = ratio.iter().copied().filter(|v| v.is_finite()).collect();
    sorted.sort_by(f32::total_cmp);
    let pct = |p: f32| sorted.get(((sorted.len() as f32 - 1.0) * p) as usize).copied().unwrap_or(1.0);
    let lo = pct(0.08).min(0.85);
    let mid = (lo + 1.0) * 0.5 + threshold.clamp(-1.0, 1.0) * (1.0 - lo) * 0.4;
    let half = ((1.0 - lo) * 0.22).max(0.03);
    let lit: Vec<f32> = ratio.iter().map(|&v| smoothstep(mid - half, mid + half, v)).collect();
    // Shade colour: shadowed vs lit pixels, compared against their local envelopes so the
    // ground's own colour cancels out.
    let (mut sh, mut li, mut ns, mut nl) = ([0.0f64; 3], [0.0f64; 3], 0usize, 0usize);
    for (i, &v) in lit.iter().enumerate() {
        let e = env[i].max(1e-4) as f64;
        if v < 0.15 {
            for c in 0..3 {
                sh[c] += rgb[i][c] as f64 / e;
            }
            ns += 1;
        } else if v > 0.85 {
            for c in 0..3 {
                li[c] += rgb[i][c] as f64 / e;
            }
            nl += 1;
        }
    }
    let shade = if ns > 20 && nl > 20 {
        std::array::from_fn(|c| ((sh[c] / ns as f64) / (li[c] / nl as f64).max(1e-6)).clamp(0.03, 1.0) as f32)
    } else {
        [lo.clamp(0.05, 1.0); 3]
    };
    LightMap { width: w, height: h, lit, shade }
}

#[cfg(test)]
mod tests {
    use super::*;
    use half::f16;

    /// Mid-grey ground, a bright and a dark half, with shadow blobs at 30 % of the local light.
    fn dappled() -> Frame {
        let (w, h) = (200u32, 120u32);
        let px: Vec<f16> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = ((i % w) as i32, (i / w) as i32);
                let base = if x < 100 { 0.6 } else { 0.25 };
                let blob = ((x % 40) - 20).pow(2) + ((y % 40) - 20).pow(2) < 64;
                let v = if blob { base * 0.3 } else { base };
                [v * 0.9, v, v * 1.1, 1.0].map(f16::from_f32)
            })
            .collect();
        Frame::new(w, h, px)
    }

    #[test]
    fn finds_shadow_blobs_on_bright_and_dark_ground() {
        let m = estimate(&dappled(), 0.0);
        let at = |x: u32, y: u32| m.lit[(y * m.width + x) as usize];
        assert!(at(20, 20) < 0.2, "blob centre on bright ground: {}", at(20, 20));
        assert!(at(140, 60) < 0.2, "blob centre on dark ground: {}", at(140, 60));
        assert!(at(5, 5) > 0.8, "lit bright ground: {}", at(5, 5));
        assert!(at(185, 5) > 0.8, "lit dark ground: {}", at(185, 5));
        // Shadow / lit ratio ≈ 0.3 on every channel.
        for c in m.shade {
            assert!((c - 0.3).abs() < 0.08, "{:?}", m.shade);
        }
    }

    #[test]
    fn running_max_matches_naive() {
        let v: Vec<f32> = (0..50).map(|i| ((i * 37) % 23) as f32).collect();
        let mut out = vec![0.0; 50];
        running_max(&v, &mut out, 50, 1, 50, 1, 3);
        for i in 0..50usize {
            let naive = v[i.saturating_sub(3)..=(i + 3).min(49)].iter().copied().fold(f32::MIN, f32::max);
            assert_eq!(out[i], naive, "at {i}");
        }
    }
}

#[cfg(test)]
mod real_scene {
    /// Opt-in check against a Blender ground truth:
    /// `PHOTOHOP_TEST_DAPPLE=dir` with plate.png, subject.png and cast.json (from scene.py) writes
    /// ours.png (Shadows on me) and plain.png (without) into the same folder.
    #[test]
    fn dapple_against_blender() {
        let Some(dir) = std::env::var_os("PHOTOHOP_TEST_DAPPLE").map(std::path::PathBuf::from) else { return };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else { return };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut app = crate::app::App::new(&egui::Context::default(), &device, &queue);
        app.import(&dir.join("plate.png")).unwrap();
        app.import(&dir.join("subject.png")).unwrap();
        let cast: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("cast.json")).unwrap()).unwrap();
        let id = app.doc.layers[1].id;
        for (name, dapple) in [("plain", false), ("ours", true)] {
            app.edit("setup", |d| {
                let l = d.layer_mut(id).unwrap();
                let s = &mut l.fx.shadow;
                s.enabled = true;
                s.angle = ph_core::Animated::new(cast["angle"].as_f64().unwrap() as f32);
                s.length = ph_core::Animated::new(cast["length"].as_f64().unwrap() as f32);
                s.ground = cast["ground"].as_f64().unwrap() as f32;
                s.opacity = ph_core::Animated::new(0.75);
                s.softness = ph_core::Animated::new(3.0);
                s.contact = ph_core::Animated::new(0.4);
                l.fx.dapple.enabled = dapple;
            });
            let ids: Vec<_> = app.doc.layers.iter().map(|l| l.id).collect();
            let f = app.composite_only(|l| ids.contains(&l), None).unwrap();
            ph_media::save_frame(&f, &dir.join(format!("{name}.png")), false).unwrap();
        }
    }
}
