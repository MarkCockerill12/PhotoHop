//! Blender round trip. "Export to Blender" writes a bundle folder that the PhotoHop Bridge add-on
//! (blender_addon/) turns into a ready scene; its render comes back as an image-sequence layer.
//!
//! Bundle layout:
//! ```text
//! manifest.json
//! subject/subject_0001.png …     the selected layer alone (RGBA, canvas-aligned)
//! background/background_0001.png the other layers (camera background for lining things up)
//! render/render_0001.png …       written by Blender: props + their shadows (RGBA)
//! ```

use crate::app::App;
use crate::export::fetch_full;
use crate::render;
use anyhow::{Context, Result, bail};
use glam::Vec2;
use ph_core::{LayerId, MediaId, Project};
use ph_media::{Frame, VideoReader};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MANIFEST: &str = "manifest.json";

pub struct BlenderJob {
    dir: PathBuf,
    subject: LayerId,
    frame: u64,
    pub total: u64,
    readers: HashMap<MediaId, VideoReader>,
    stills: HashMap<MediaId, Frame>,
}

impl BlenderJob {
    pub fn progress(&self) -> f32 {
        self.frame as f32 / self.total.max(1) as f32
    }
}

fn only(doc: &Project, keep: impl Fn(LayerId) -> bool) -> Project {
    let mut d = doc.clone();
    for l in &mut d.layers {
        l.visible = l.visible && keep(l.id);
    }
    d.canvas.background = [0.0; 4];
    d
}

impl App {
    /// Start writing a Blender bundle for the selected layer into `dir`.
    pub fn start_blender_export(&mut self, dir: &Path) -> Result<()> {
        let id = self.selected.context("select the layer that is you (the subject) first")?;
        let layer = self.doc.layer(id).context("layer missing")?.clone();
        let any_video = self.doc.media.iter().any(|m| m.is_video());
        let total = if any_video { self.doc.canvas.frame_count() } else { 1 };
        std::fs::create_dir_all(dir.join("subject"))?;
        std::fs::create_dir_all(dir.join("background"))?;
        std::fs::create_dir_all(dir.join("render"))?;

        // Where the feet touch the ground (canvas px) and the light set on the Relight pad.
        let t0 = if any_video { 0.0 } else { self.time };
        let size = self.doc.layer_source_size(&layer);
        let ground = self.find_ground(id).unwrap_or(layer.fx.shadow.ground);
        let feet = layer.transform.matrix(t0, size).transform_point2(Vec2::new(size.x * 0.5, size.y * ground));
        let r = &layer.fx.relight;
        let c = &self.doc.canvas;
        let manifest = serde_json::json!({
            "photohop_bundle": 1,
            "name": self.doc.name,
            "width": c.width,
            "height": c.height,
            "fps": c.fps,
            "frames": total,
            "subject": "subject/subject_%04d.png",
            "background": "background/background_%04d.png",
            "render": "render/render_",
            "render_pattern": "render/render_%04d.png",
            "feet_px": [feet.x, feet.y],
            "fov_deg": 50.0,
            "light": {
                "azimuth": r.azimuth.at(t0),
                "elevation": r.elevation.at(t0),
                "color": r.color,
                "intensity": r.intensity.at(t0),
            },
        });
        std::fs::write(dir.join(MANIFEST), serde_json::to_string_pretty(&manifest)?)?;
        std::fs::write(
            dir.join("README.txt"),
            "PhotoHop → Blender bundle\n\n1. In Blender: Edit > Preferences > Add-ons > Install from disk… and pick\n   photohop_bridge.zip (from the PhotoHop download), then enable it.\n2. File > Import > PhotoHop bundle (manifest.json). The scene is built for you:\n   camera, you as a shadow-casting card, a shadow-catcher floor at your feet, a sun.\n3. Add your props (e.g. a gun model in your hand) in the 'Props' collection.\n4. Render > Render for PhotoHop. Frames are written to render/.\n5. Back in PhotoHop: File > Import Blender render… and pick this folder.\n",
        )?;
        self.playing = false;
        self.blender_job = Some(BlenderJob { dir: dir.to_path_buf(), subject: id, frame: 0, total, readers: HashMap::new(), stills: HashMap::new() });
        Ok(())
    }

    /// Write queued bundle frames for up to ~40 ms. Returns true while still running.
    pub fn pump_blender_export(&mut self) -> bool {
        let Some(mut job) = self.blender_job.take() else { return false };
        let deadline = Instant::now() + Duration::from_millis(40);
        let result: Result<bool> = (|| {
            while job.frame < job.total && Instant::now() < deadline {
                let t = if job.total == 1 { self.time } else { self.doc.canvas.frame_time(job.frame) };
                let n = job.frame + 1;
                for (part, keep_subject) in [("subject", true), ("background", false)] {
                    let doc = only(&self.doc, |l| (l == job.subject) == keep_subject);
                    let (readers, stills) = (&mut job.readers, &mut job.stills);
                    render::composite(&mut self.viewer.gpu, &doc, t, &self.paint, &self.masks, &self.normals, |m, mt| {
                        fetch_full(readers, stills, m, mt)
                    });
                    let (w, h, px) = self.viewer.gpu.read_back()?;
                    ph_media::save_frame(&Frame::new(w, h, px), &job.dir.join(part).join(format!("{part}_{n:04}.png")), false)?;
                }
                job.frame += 1;
            }
            Ok(job.frame < job.total)
        })();
        match result {
            Ok(true) => {
                self.blender_job = Some(job);
                true
            }
            Ok(false) => {
                self.status = format!("Blender bundle written to {} — open it with the PhotoHop Bridge add-on.", job.dir.display());
                false
            }
            Err(e) => {
                self.status = format!("Blender export failed: {e:#}");
                false
            }
        }
    }

    /// Add Blender's render (props + shadows) from a bundle folder as the top layer.
    pub fn import_blender_render(&mut self, dir: &Path) -> Result<()> {
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(MANIFEST)).context("no manifest.json in that folder")?)?;
        let pattern = dir.join(manifest["render_pattern"].as_str().unwrap_or("render/render_%04d.png"));
        let fps = manifest["fps"].as_f64().unwrap_or(self.doc.canvas.fps);
        if ph_media::video::sequence_files(&pattern).is_empty() {
            bail!("no rendered frames in {} yet — render in Blender first", pattern.parent().unwrap_or(dir).display());
        }
        let (info, _) = ph_media::video::probe_sequence(&pattern, fps)?;
        let media = ph_core::MediaRef {
            id: MediaId::new(),
            path: pattern,
            kind: ph_core::MediaKind::Video { duration: info.duration, fps, has_audio: false },
            width: info.width,
            height: info.height,
        };
        let mid = media.id;
        self.edit("Import Blender render", |d| {
            d.media.push(media);
            let l = ph_core::Layer::new("Blender render", ph_core::LayerContent::Media { media: mid, trim_in: 0.0 }, &d.canvas);
            d.layers.push(l);
        });
        self.selected = self.doc.layers.last().map(|l| l.id);
        self.status = "Blender render added on top (props + their shadows).".into();
        Ok(())
    }
}

#[cfg(test)]
mod real_project {
    /// Opt-in: `PHOTOHOP_TEST_BUNDLE=project.phop:out_dir cargo test -p photohop bundle_real`.
    /// Exports a Blender bundle for the project's top layer.
    #[test]
    fn bundle_real() {
        let Some(spec) = std::env::var("PHOTOHOP_TEST_BUNDLE").ok() else { return };
        let (proj, out) = spec.rsplit_once(':').expect("project.phop:out_dir");
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else { return };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut app = crate::app::App::new(&egui::Context::default(), &device, &queue);
        app.open(std::path::Path::new(proj)).unwrap();
        app.selected = app.doc.layers.last().map(|l| l.id);
        app.start_blender_export(std::path::Path::new(out)).unwrap();
        while app.pump_blender_export() {}
        eprintln!("{}", app.status);
        assert!(std::path::Path::new(out).join("subject/subject_0001.png").is_file());
    }
}
