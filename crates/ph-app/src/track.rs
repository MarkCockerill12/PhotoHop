//! Match-moving from AI mask videos: follow a tracked region's motion with another layer.

use crate::app::App;
use anyhow::{Context, Result, bail};
use glam::Vec2;
use ph_core::LayerId;
use ph_media::{Frame, VideoReader};

/// Bounding box `(min, max)` of mask pixels above `threshold` (in frame pixels).
pub fn mask_bbox(f: &Frame, threshold: f32) -> Option<(Vec2, Vec2)> {
    let (w, h) = (f.width as usize, f.height as usize);
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    let mut any = false;
    for y in 0..h {
        for x in 0..w {
            if f.pixels[(y * w + x) * 4].to_f32() > threshold {
                let p = Vec2::new(x as f32, y as f32);
                lo = lo.min(p);
                hi = hi.max(p + Vec2::ONE);
                any = true;
            }
        }
    }
    any.then_some((lo, hi))
}

impl App {
    /// Keyframe `target`'s position and scale every frame so it moves with the region tracked in
    /// `tracked`'s mask video, keeping the current placement as the starting point.
    pub fn follow_track(&mut self, target: LayerId, tracked: LayerId) -> Result<usize> {
        let tl = self.doc.layer(tracked).context("tracked layer missing")?.clone();
        let mid = tl.mask.as_ref().and_then(|m| m.video).context("that layer has no tracked mask video")?;
        let mref = self.doc.media(mid).context("mask video missing")?.clone();
        let mut reader = VideoReader::open_scaled(&mref.path, 4)?;
        let fps = self.doc.canvas.fps;
        let frames = self.doc.canvas.frame_count();
        let src_size = self.doc.layer_source_size(&tl);
        let to_src = Vec2::new(src_size.x / reader.out_width as f32, src_size.y / reader.out_height as f32);
        // Linear value of a mid-grey (sRGB 0.5) — the mask video stores encoded greys.
        let threshold = 0.21;
        let mut samples = Vec::new();
        for f in 0..frames {
            let t = f as f64 / fps;
            if !tl.active_at(t) {
                continue;
            }
            let idx = (tl.media_time(t) * reader.info.fps).floor().max(0.0) as u64;
            let Ok(frame) = reader.frame(idx) else { continue };
            let Some((lo, hi)) = mask_bbox(&frame, threshold) else { continue };
            let m = tl.transform.matrix(t, src_size);
            let (a, b) = (m.transform_point2(lo * to_src), m.transform_point2(hi * to_src));
            let centre = (a + b) * 0.5;
            let size = (b - a).abs();
            samples.push((t, centre, size.x.max(size.y)));
        }
        let Some(&(t0, c0, s0)) = samples.first() else { bail!("the tracked mask is empty") };
        let target_layer = self.doc.layer(target).context("target layer missing")?;
        let p0 = Vec2::from(target_layer.transform.position.at(t0));
        let k0 = target_layer.transform.scale.at(t0);
        let n = samples.len();
        self.edit("Follow tracked region", |d| {
            if let Some(l) = d.layer_mut(target) {
                l.transform.position.keys.clear();
                l.transform.scale.keys.clear();
                for (t, c, s) in &samples {
                    let p = p0 + (*c - c0);
                    let k = s / s0.max(1e-3);
                    l.transform.position.set(*t, p.to_array(), true);
                    l.transform.scale.set(*t, [k0[0] * k, k0[1] * k], true);
                }
            }
        });
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use half::f16;

    #[test]
    fn bbox_of_bright_block() {
        let (w, h) = (8usize, 6usize);
        let px: Vec<f16> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                let v = if (2..5).contains(&x) && (1..3).contains(&y) { 1.0 } else { 0.0 };
                [v, v, v, 1.0].map(f16::from_f32)
            })
            .collect();
        let (lo, hi) = mask_bbox(&Frame::new(w as u32, h as u32, px), 0.5).unwrap();
        assert_eq!((lo, hi), (Vec2::new(2.0, 1.0), Vec2::new(5.0, 3.0)));
    }
}

#[cfg(test)]
mod real_project {
    /// Opt-in: `PHOTOHOP_TEST_WINDOW=/path/window_test.phop cargo test -p photohop follow_real`.
    /// The project's bottom layer follows the top layer's tracked mask.
    #[test]
    fn follow_real() {
        let Some(path) = std::env::var_os("PHOTOHOP_TEST_WINDOW") else { return };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else { return };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut app = crate::app::App::new(&egui::Context::default(), &device, &queue);
        app.open(std::path::Path::new(&path)).unwrap();
        let (target, tracked) = (app.doc.layers[0].id, app.doc.layers[1].id);
        let n = app.follow_track(target, tracked).unwrap();
        let keys = &app.doc.layer(target).unwrap().transform.position.keys;
        let xs: Vec<f32> = keys.iter().map(|k| k.value[0]).collect();
        let span = xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
        eprintln!("{n} keys, x drift span {span:.1}px, first {:?} last {:?}", keys.first().map(|k| k.value), keys.last().map(|k| k.value));
        assert!(n > 100);
    }
}
