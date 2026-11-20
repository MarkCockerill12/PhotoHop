//! Optional AI helpers via the `photohop-ai` Python sidecar (see `ai/README.md`).
//! Jobs run in a background thread; results are plain mask images that get baked into the
//! layer's mask, so projects stay portable to machines without the AI environment.

use crate::app::App;
use crate::paint::PaintRaster;
use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, unbounded};
use ph_core::LayerId;
use ph_core::doc::LayerContent;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq)]
pub enum AiTask {
    /// Automatic subject cutout (BiRefNet).
    Matte,
    /// Text-prompted mask (SAM 3).
    Segment(String),
}

impl AiTask {
    fn label(&self) -> String {
        match self {
            Self::Matte => "AI cutout".into(),
            Self::Segment(p) => format!("AI mask \"{p}\""),
        }
    }
}

pub struct AiJob {
    pub layer: LayerId,
    pub task: AiTask,
    pub output: PathBuf,
    rx: Receiver<Result<serde_json::Value, String>>,
}

pub struct AiState {
    /// Python interpreter of the environment where `photohop-ai` is installed.
    pub python: String,
    pub prompt: String,
    pub show_dialog: bool,
    pub job: Option<AiJob>,
}

impl Default for AiState {
    fn default() -> Self {
        let python = std::env::var("PHOTOHOP_AI_PYTHON")
            .unwrap_or_else(|_| if cfg!(windows) { "python".into() } else { "python3".into() });
        Self { python, prompt: "person".into(), show_dialog: false, job: None }
    }
}

fn temp_dir() -> Result<PathBuf> {
    let d = std::env::temp_dir().join("photohop-ai");
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

/// Run the sidecar and parse its single JSON result line.
fn run_sidecar(python: &str, args: &[String]) -> Result<serde_json::Value, String> {
    let out = Command::new(python)
        .args(["-m", "photohop_ai.cli"])
        .args(args)
        .output()
        .map_err(|e| format!("could not start {python}: {e}. Set the AI Python in AI > Settings."))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().rev().find(|l| l.trim_start().starts_with('{'));
    match line.and_then(|l| serde_json::from_str::<serde_json::Value>(l).ok()) {
        Some(v) if v["ok"].as_bool() == Some(true) => Ok(v),
        Some(v) => Err(v["error"].as_str().unwrap_or("unknown error").to_owned()),
        None => {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = err.lines().rev().take(3).collect();
            Err(format!("photohop-ai gave no result. {}", tail.into_iter().rev().collect::<Vec<_>>().join(" | ")))
        }
    }
}

/// Load a greyscale mask image as a mask raster (red channel = coverage, opaque).
fn load_mask(path: &Path) -> Result<PaintRaster> {
    let img = image::open(path).with_context(|| format!("reading {}", path.display()))?.to_luma8();
    let (w, h) = img.dimensions();
    let mut r = PaintRaster::new(w, h);
    for (dst, v) in r.data.iter_mut().zip(img.as_raw()) {
        let m = *v as f32 / 255.0;
        *dst = [m, m, m, 1.0];
    }
    Ok(r)
}

impl App {
    /// Write the selected layer's current source at full resolution and start an AI job on it.
    pub fn start_ai(&mut self, task: AiTask) -> Result<()> {
        if self.ai.job.is_some() {
            bail!("an AI job is already running");
        }
        let id = self.selected.context("select a layer first")?;
        let layer = self.doc.layer(id).context("layer missing")?;
        let dir = temp_dir()?;
        let input = match &layer.content {
            LayerContent::Media { media, .. } => {
                let m = self.doc.media(*media).context("media missing")?;
                if m.is_video() {
                    let mut r = ph_media::VideoReader::open(&m.path)?;
                    let idx = (layer.media_time(self.time) * r.info.fps).floor().max(0.0) as u64;
                    let f = r.frame(idx)?;
                    let p = dir.join("input.png");
                    ph_media::save_frame(&f, &p, false)?;
                    p
                } else {
                    m.path.clone()
                }
            }
            LayerContent::Paint { .. } => {
                let p = dir.join("input.png");
                ph_media::save_frame(&self.paint.get(&id).context("paint raster missing")?.to_frame(), &p, false)?;
                p
            }
            LayerContent::Solid { .. } => bail!("AI masks need a photo, video or paint layer"),
        };
        let output = dir.join(format!("mask-{}.png", id.0));
        let mut args = vec![String::new(), input.to_string_lossy().into_owned(), output.to_string_lossy().into_owned()];
        match &task {
            AiTask::Matte => args[0] = "matte".into(),
            AiTask::Segment(prompt) => {
                args[0] = "segment".into();
                args.extend(["--prompt".into(), prompt.clone()]);
            }
        }
        let (tx, rx) = unbounded();
        let python = self.ai.python.clone();
        let ctx = self.egui_ctx.clone();
        std::thread::Builder::new().name("photohop-ai".into()).spawn(move || {
            let _ = tx.send(run_sidecar(&python, &args));
            ctx.request_repaint();
        })?;
        self.status = format!("{} running… (first run downloads the model)", task.label());
        self.ai.job = Some(AiJob { layer: id, task, output, rx });
        Ok(())
    }

    /// Collect a finished AI job: the result becomes the layer's mask (undoable as a document edit).
    pub fn poll_ai(&mut self) {
        let Some(job) = &self.ai.job else { return };
        let Ok(result) = job.rx.try_recv() else { return };
        let job = self.ai.job.take().expect("job");
        let applied: Result<String> = (|| {
            let v = result.map_err(|e| anyhow::anyhow!(e))?;
            let raster = load_mask(&job.output)?;
            let (w, h) = (raster.width, raster.height);
            let id = job.layer;
            self.edit(&job.task.label(), |d| {
                if let Some(l) = d.layer_mut(id) {
                    l.mask = Some(ph_core::LayerMask { enabled: true, invert: false, width: w, height: h });
                }
            });
            self.masks.insert(id, raster);
            Ok(format!("{} done in {:.1}s — applied as the layer mask (paint to refine).", job.task.label(), v["seconds"].as_f64().unwrap_or(0.0)))
        })();
        self.status = match applied {
            Ok(s) => s,
            Err(e) => format!("{} failed: {e:#}", job.task.label()),
        };
    }

    pub fn ai_running(&self) -> bool {
        self.ai.job.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_png_becomes_raster() {
        let dir = std::env::temp_dir().join(format!("ph-ai-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.png");
        let img = image::GrayImage::from_fn(4, 2, |x, _| image::Luma([if x < 2 { 0 } else { 255 }]));
        img.save(&path).unwrap();
        let r = load_mask(&path).unwrap();
        assert_eq!((r.width, r.height), (4, 2));
        assert_eq!(r.data[0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(r.data[3], [1.0, 1.0, 1.0, 1.0]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_python_reports_error() {
        let e = run_sidecar("definitely-not-a-python-binary", &["info".into()]).unwrap_err();
        assert!(e.contains("could not start"), "{e}");
    }
}
