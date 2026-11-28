//! "Follow the camera" for Replace window / screen: the panes (and the view seen through them)
//! are keyframed with the handheld camera's motion, measured around the window without AI.

use crate::app::App;
use crate::camtrack::{self, Luma};
use anyhow::{Context, Result, bail};
use glam::{Affine2, Vec2};
use ph_core::{LayerContent, LayerId, RotoMode, RotoPath};
use ph_media::VideoReader;
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};

pub struct CameraJob {
    layer: LayerId,
    /// Project time the panes were drawn at (the reference frame).
    ref_time: f64,
    rx: Receiver<Result<Vec<Option<Affine2>>>>,
}

/// Tracking images are about this size on their long side.
const TRACK_SIDE: u32 = 960;

/// Track `count` frames of `path` around `region` (video pixels) from frame `reference`. Returns per
/// frame the map from reference-frame video pixels to that frame's video pixels.
pub fn track_video(path: &Path, reference: usize, region: (Vec2, Vec2)) -> Result<Vec<Option<Affine2>>> {
    let probe = VideoReader::open(path)?;
    let div = probe.info.width.max(probe.info.height).div_ceil(TRACK_SIDE).max(1);
    let mut reader = VideoReader::open_scaled(path, div)?;
    let count = reader.info.frame_count() as usize;
    let k = reader.info.width as f32 / reader.out_width as f32;
    let mut cache: HashMap<usize, Luma> = HashMap::new();
    // Backward steps decode a block at a time (seeking per frame would restart the decoder).
    let frame = |i: usize| -> Option<Luma> {
        if let Some(l) = cache.remove(&i) {
            return Some(l);
        }
        if i > 0 && (i as u64) < reader_next(&reader) {
            let start = i.saturating_sub(47);
            for j in start..=i {
                if let Ok(f) = reader.frame(j as u64) {
                    cache.insert(j, Luma::from_frame(&f));
                }
            }
            return cache.remove(&i);
        }
        reader.frame(i as u64).ok().map(|f| Luma::from_frame(&f))
    };
    let small = (region.0 / k, region.1 / k);
    let moves = camtrack::track(frame, count, reference.min(count.saturating_sub(1)), small);
    let (to, from) = (Affine2::from_scale(Vec2::splat(k)), Affine2::from_scale(Vec2::splat(1.0 / k)));
    Ok(moves.into_iter().map(|m| m.map(|m| to * m * from)).collect())
}

fn reader_next(r: &VideoReader) -> u64 {
    r.next_frame()
}

impl App {
    /// Start tracking the camera around a layer's glass (runs in the background).
    pub fn glass_follow_camera(&mut self, id: LayerId) -> Result<()> {
        if self.camera_job.is_some() {
            bail!("already following the camera");
        }
        let layer = self.doc.layer(id).context("layer missing")?;
        let g = layer.glass.as_ref().context("set up Replace window first")?;
        let LayerContent::Media { media, .. } = &layer.content else { bail!("this works on video layers") };
        let m = self.doc.media(*media).context("media missing")?;
        let ph_core::MediaKind::Video { fps, .. } = m.kind else { bail!("this works on video layers") };
        let t = self.time;
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for pane in g.panes.iter().filter(|p| p.active() && p.mode == RotoMode::Add) {
            for p in pane.polygon(t, 4) {
                lo = lo.min(Vec2::from(p));
                hi = hi.max(Vec2::from(p));
            }
        }
        if hi.x <= lo.x {
            bail!("find or draw the glass first");
        }
        // Lock onto the window and the wall around it.
        let grow = (hi - lo) * 0.3;
        let region = (lo - grow, hi + grow);
        let reference = (layer.media_time(t) * fps).floor().max(0.0) as usize;
        let path = m.path.clone();
        let (tx, rx) = channel();
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(track_video(&path, reference, region));
            ctx.request_repaint();
        });
        self.camera_job = Some(CameraJob { layer: id, ref_time: t, rx });
        self.status = "Following the camera around the glass…".into();
        Ok(())
    }

    pub fn camera_job_running(&self) -> bool {
        self.camera_job.is_some()
    }

    /// Apply a finished camera track: keyframe every pane and the view each frame.
    pub fn poll_camera_job(&mut self) {
        let Some(job) = &self.camera_job else { return };
        let Ok(res) = job.rx.try_recv() else { return };
        let (id, t0) = (job.layer, job.ref_time);
        self.camera_job = None;
        self.status = match res.and_then(|moves| self.apply_camera_moves(id, t0, &moves)) {
            Ok(n) => format!("The glass now follows the camera ({n} frames keyed). Check a few frames; Undo removes it."),
            Err(e) => format!("Follow the camera: {e:#}"),
        };
    }

    fn apply_camera_moves(&mut self, id: LayerId, t0: f64, moves: &[Option<Affine2>]) -> Result<usize> {
        let layer = self.doc.layer(id).context("layer missing")?.clone();
        let LayerContent::Media { media, .. } = &layer.content else { bail!("not a video layer") };
        let fps = match self.doc.media(*media).map(|m| &m.kind) {
            Some(ph_core::MediaKind::Video { fps, .. }) => *fps,
            _ => bail!("not a video layer"),
        };
        let found = moves.iter().filter(|m| m.is_some()).count();
        if found < moves.len() / 2 {
            bail!("lost the camera on most frames ({found} of {}): there may be too little detail around the glass", moves.len());
        }
        let g = layer.glass.clone().context("glass missing")?;
        let size = self.doc.layer_source_size(&layer);
        let canvas = &self.doc.canvas;
        let frames: Vec<(f64, Affine2)> = (0..canvas.frame_count())
            .filter_map(|f| {
                let t = canvas.frame_time(f);
                if !layer.active_at(t) {
                    return None;
                }
                let i = (layer.media_time(t) * fps).floor().max(0.0) as usize;
                // A frame the track lost holds the nearest good one.
                let m = (0..moves.len()).map(|d| [i.saturating_sub(d), (i + d).min(moves.len() - 1)]).find_map(|[a, b]| moves[a].or(moves[b]))?;
                Some((t, m))
            })
            .collect();
        let pane_base: Vec<RotoPath> = g.panes.iter().map(|p| p.path.at(t0)).collect();
        let lm = layer.transform.matrix(t0, size);
        let vt = &g.view_transform;
        let (vpos, vscale, vrot) = (Vec2::from(vt.position.at(t0)), vt.scale.at(t0), vt.rotation.at(t0));
        let n = frames.len();
        self.edit("Follow the camera", |d| {
            let Some(g) = d.layer_mut(id).and_then(|l| l.glass.as_mut()) else { return };
            for (pane, base) in g.panes.iter_mut().zip(&pane_base) {
                pane.path.keys.clear();
                for (t, m) in &frames {
                    let moved = RotoPath(
                        base.0
                            .iter()
                            .map(|q| ph_core::RotoPoint {
                                p: m.transform_point2(Vec2::from(q.p)).into(),
                                h_in: m.transform_vector2(Vec2::from(q.h_in)).into(),
                                h_out: m.transform_vector2(Vec2::from(q.h_out)).into(),
                            })
                            .collect(),
                    );
                    pane.path.set(*t, moved, true);
                }
            }
            // The view moves with the camera too (seen through the glass, far away).
            let v = &mut g.view_transform;
            v.position.keys.clear();
            v.scale.keys.clear();
            v.rotation.keys.clear();
            for (t, m) in &frames {
                let c = lm * *m * lm.inverse();
                let s = c.matrix2.x_axis.length();
                let a = c.matrix2.x_axis.y.atan2(c.matrix2.x_axis.x).to_degrees();
                v.position.set(*t, c.transform_point2(vpos).into(), true);
                v.scale.set(*t, [vscale[0] * s, vscale[1] * s], true);
                v.rotation.set(*t, vrot + a, true);
            }
        });
        Ok(n)
    }
}

#[cfg(test)]
mod real {
    /// Opt-in: `PHOTOHOP_TEST_GLASS_TRACK=in.phop|out.phop|t` follows the camera on the first glass layer.
    #[test]
    fn glass_track_real() {
        let Some(spec) = std::env::var("PHOTOHOP_TEST_GLASS_TRACK").ok() else {
            return;
        };
        let mut it = spec.split('|');
        let (inp, out, t) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap_or("0").parse::<f64>().unwrap());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut app = crate::app::App::new(&egui::Context::default(), &device, &queue);
        app.open(std::path::Path::new(inp)).unwrap();
        app.time = t;
        let id = app.doc.layers.iter().find(|l| l.glass.is_some()).unwrap().id;
        let start = std::time::Instant::now();
        app.glass_follow_camera(id).unwrap();
        while app.camera_job_running() {
            std::thread::sleep(std::time::Duration::from_millis(200));
            app.poll_camera_job();
        }
        eprintln!("{} ({:.1}s)", app.status, start.elapsed().as_secs_f32());
        let g = app.doc.layer(id).unwrap().glass.as_ref().unwrap();
        let xs: Vec<f32> = g.view_transform.position.keys.iter().map(|k| k.value[0]).collect();
        let ys: Vec<f32> = g.view_transform.position.keys.iter().map(|k| k.value[1]).collect();
        let span = |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min);
        eprintln!("view drift span x {:.1}px y {:.1}px", span(&xs), span(&ys));
        app.save_to(std::path::Path::new(out)).unwrap();
    }
}
