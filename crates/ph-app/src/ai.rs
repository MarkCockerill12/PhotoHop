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

#[derive(Clone, Debug, PartialEq)]
pub enum AiTask {
    /// Automatic subject cutout (BiRefNet).
    Matte,
    /// Text-prompted mask (SAM 3).
    Segment(String),
    /// Surface normals for relighting (Sapiens2).
    Normals,
    /// Track the region inside a box (source pixels `[x0, y0, x1, y1]`) through a video (SAM 2.1).
    TrackBox([f32; 4]),
    /// Cut the main subject out of a whole video (BiRefNet + MatAnyone 2).
    VideoCutout,
    /// Repaint an object away (SAM 3 + FLUX.2 klein); result is a patch layer limited to the object.
    Remove(String),
    /// AI auto-colour (Harmonizer): the suggestion is fitted into the layer's R/G/B curves.
    AutoColour,
    /// 3D camera solve of a video layer (Depth Anything 3) for the Blender bridge.
    CameraSolve,
    /// 3D body motion of the person in a video layer (SAM 3D Body) for the Blender bridge.
    BodyMotion,
    /// Depth map of the layer (Depth Anything 3) for depth of field.
    Depth,
    /// Find window panes / screens (SAM 3) as glass shapes for "Replace window / screen".
    FindGlass(String),
    /// Track something in front of the glass (a box, SAM 2.1) so it stays in front of the view.
    GlassFront([f32; 4]),
}

impl AiTask {
    fn label(&self) -> String {
        match self {
            Self::Matte => "AI cutout".into(),
            Self::Segment(p) => format!("AI mask \"{p}\""),
            Self::Normals => "AI normals".into(),
            Self::TrackBox(_) => "AI region track".into(),
            Self::VideoCutout => "AI video cutout".into(),
            Self::Remove(p) => format!("AI remove \"{p}\""),
            Self::AutoColour => "AI auto colour".into(),
            Self::CameraSolve => "AI camera solve".into(),
            Self::BodyMotion => "AI body motion".into(),
            Self::FindGlass(_) => "AI find glass".into(),
            Self::Depth => "AI depth".into(),
            Self::GlassFront(_) => "AI keep in front".into(),
        }
    }

    fn is_video(&self) -> bool {
        matches!(self, Self::TrackBox(_) | Self::VideoCutout | Self::CameraSolve | Self::BodyMotion | Self::GlassFront(_))
    }
}

pub struct AiJob {
    pub layer: LayerId,
    pub task: AiTask,
    pub output: PathBuf,
    /// Inputs written for the job (auto colour: composite and mask), kept for post-processing.
    aux: Vec<PathBuf>,
    rx: Receiver<Result<serde_json::Value, String>>,
}

/// Where AI results live until the project is saved (then they are copied into its assets).
pub fn results_dir() -> PathBuf {
    std::env::temp_dir().join("photohop-ai")
}

pub struct AiState {
    /// Python interpreter of the environment where `photohop-ai` is installed.
    pub python: String,
    pub prompt: String,
    pub remove_prompt: String,
    /// What to look for when finding glass ("window pane", "tv screen", ...).
    pub glass_prompt: String,
    pub show_dialog: bool,
    pub job: Option<AiJob>,
    /// Next viewer drag draws a box for region tracking.
    pub picking_box: bool,
    /// The box being drawn marks something in front of the glass (not a region to track as a mask).
    pub box_for_glass: bool,
}

impl Default for AiState {
    fn default() -> Self {
        let python = std::env::var("PHOTOHOP_AI_PYTHON")
            .unwrap_or_else(|_| if cfg!(windows) { "python".into() } else { "python3".into() });
        Self {
            python,
            prompt: "person".into(),
            remove_prompt: String::new(),
            glass_prompt: "window pane".into(),
            show_dialog: false,
            job: None,
            picking_box: false,
            box_for_glass: false,
        }
    }
}

fn temp_dir() -> Result<PathBuf> {
    let d = results_dir();
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

/// Run the sidecar and parse its single JSON result line.
fn run_sidecar(python: &str, args: &[String], exe_dir: Option<&Path>) -> Result<serde_json::Value, String> {
    let mut cmd = ph_media::process::command(python);
    cmd.args(["-m", "photohop_ai.cli"]).args(args);
    // Let the helper find the FFmpeg bundled next to PhotoHop.
    if let Some(dir) = exe_dir {
        let mut paths = vec![dir.to_path_buf()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        if let Ok(joined) = std::env::join_paths(paths) {
            cmd.env("PATH", joined);
        }
    }
    let out = cmd
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
        let dir = temp_dir()?;
        if task == AiTask::AutoColour {
            return self.start_auto_colour(id, &dir);
        }
        let layer = self.doc.layer(id).context("layer missing")?;
        let mut input = match &layer.content {
            LayerContent::Media { media, .. } if task.is_video() => {
                let m = self.doc.media(*media).context("media missing")?;
                if !m.is_video() {
                    bail!("this needs a video layer");
                }
                m.path.clone()
            }
            _ if task.is_video() => bail!("this needs a video layer"),
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
        // Removing something from a cut-out layer: the AI needs the scene behind it as context.
        let mut aux = Vec::new();
        if matches!(task, AiTask::Remove(_))
            && let Some(p) = self.remove_context_input(id, &input, &dir)?
        {
            input = p;
            aux.push(input.clone());
        }
        let layer = self.doc.layer(id).context("layer missing")?;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let output = match &task {
            AiTask::Normals => dir.join(format!("normals-{}.png", id.0)),
            AiTask::Depth => dir.join(format!("depth-{}-{stamp}.png", id.0)),
            AiTask::CameraSolve => dir.join(format!("camera-{stamp}.json")),
            AiTask::BodyMotion => dir.join(format!("body-{stamp}.json")),
            AiTask::FindGlass(_) => dir.join(format!("glass-{stamp}.json")),
            t if t.is_video() => dir.join(format!("maskvideo-{}-{stamp}.mp4", id.0)),
            AiTask::Remove(_) => dir.join(format!("patch-{}-{stamp}.png", id.0)),
            _ => dir.join(format!("mask-{}.png", id.0)),
        };
        // Never let a stale result from an earlier run be read back as this job's output.
        let _ = std::fs::remove_file(&output);
        let mut args = vec![String::new(), input.to_string_lossy().into_owned(), output.to_string_lossy().into_owned()];
        match &task {
            AiTask::Matte => args[0] = "matte".into(),
            AiTask::Segment(prompt) => {
                args[0] = "segment".into();
                args.extend(["--prompt".into(), prompt.clone()]);
            }
            AiTask::Normals => args[0] = "normals".into(),
            AiTask::Depth => args[0] = "depth".into(),
            AiTask::FindGlass(prompt) => {
                args[0] = "find-glass".into();
                args.extend(["--prompt".into(), prompt.clone()]);
            }
            AiTask::TrackBox(b) | AiTask::GlassFront(b) => {
                args[0] = "track".into();
                args.extend(["--box".into(), format!("{},{},{},{}", b[0], b[1], b[2], b[3])]);
            }
            AiTask::VideoCutout => args[0] = "video-matte".into(),
            AiTask::Remove(prompt) => {
                args[0] = "remove".into();
                args.extend(["--prompt".into(), prompt.clone()]);
            }
            AiTask::AutoColour => unreachable!("handled by start_auto_colour"),
            AiTask::BodyMotion => {
                args[0] = "body-motion".into();
                let mask = layer.mask.as_ref().and_then(|m| m.video).and_then(|mid| self.doc.media(mid)).map(|m| m.path.clone());
                let mask = mask.context("cut yourself out first (AI > Video: cut out subject) — body capture follows that cutout")?;
                args.extend(["--mask".into(), mask.to_string_lossy().into_owned()]);
                if let Some(c) = &self.doc.camera_solve {
                    args.extend(["--camera".into(), c.to_string_lossy().into_owned()]);
                }
            }
            AiTask::CameraSolve => {
                args[0] = "camera-solve".into();
                // Ignore the person when finding the floor, if a video cutout exists.
                let mask = layer.mask.as_ref().and_then(|m| m.video).and_then(|mid| self.doc.media(mid)).map(|m| m.path.clone());
                if let Some(m) = mask {
                    args.extend(["--mask".into(), m.to_string_lossy().into_owned()]);
                }
            }
        }
        let (tx, rx) = unbounded();
        let python = self.ai.python.clone();
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
        let ctx = self.egui_ctx.clone();
        std::thread::Builder::new().name("photohop-ai".into()).spawn(move || {
            let _ = tx.send(run_sidecar(&python, &args, exe_dir.as_deref()));
            ctx.request_repaint();
        })?;
        self.status = format!("{} running… (first run downloads the model)", task.label());
        self.ai.job = Some(AiJob { layer: id, task, output, rx, aux });
        Ok(())
    }

    /// For a layer with transparency (cutout PNG or mask) above other layers: the layer composited
    /// over everything below it, resampled into the layer's own pixel space. `None` = use as is.
    fn remove_context_input(&mut self, id: LayerId, src: &Path, dir: &Path) -> Result<Option<PathBuf>> {
        let idx = self.doc.layer_index(id).context("layer missing")?;
        if idx == 0 {
            return Ok(None);
        }
        let has_mask = self.doc.layers[idx].mask.as_ref().is_some_and(|m| m.enabled);
        let frame = ph_media::load_image(src)?;
        let transparent = has_mask || frame.pixels.as_chunks::<4>().0.iter().any(|p| p[3].to_f32() < 0.98);
        if !transparent {
            return Ok(None);
        }
        let upto: Vec<LayerId> = self.doc.layers[..=idx].iter().map(|l| l.id).collect();
        let comp = self.composite_only(|l| upto.contains(&l), None)?;
        let layer = &self.doc.layers[idx];
        let size = self.doc.layer_source_size(layer);
        let m = layer.transform.matrix(self.time, size);
        let (w, h) = (size.x.round().max(1.0) as u32, size.y.round().max(1.0) as u32);
        let (cw, ch) = (comp.width as i64, comp.height as i64);
        let px = |x: i64, y: i64, c: usize| -> f32 { comp.pixels[((y.clamp(0, ch - 1) * cw + x.clamp(0, cw - 1)) * 4) as usize + c].to_f32() };
        let mut out = image::RgbaImage::new(w, h);
        for (x, y, o) in out.enumerate_pixels_mut() {
            let p = m.transform_point2(glam::Vec2::new(x as f32 + 0.5, y as f32 + 0.5)) - 0.5;
            let (x0, y0) = (p.x.floor(), p.y.floor());
            let (fx, fy) = (p.x - x0, p.y - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let s: [f32; 4] = std::array::from_fn(|c| {
                let top = px(x0, y0, c) * (1.0 - fx) + px(x0 + 1, y0, c) * fx;
                let bot = px(x0, y0 + 1, c) * (1.0 - fx) + px(x0 + 1, y0 + 1, c) * fx;
                top * (1.0 - fy) + bot * fy
            });
            let a = s[3].clamp(0.0, 1.0);
            let un = |v: f32| (ph_media::color::linear_to_srgb((v / a.max(1e-4)).max(0.0)).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            *o = image::Rgba([un(s[0]), un(s[1]), un(s[2]), (a * 255.0 + 0.5) as u8]);
        }
        let path = dir.join("remove_input.png");
        out.save(&path)?;
        Ok(Some(path))
    }

    fn spawn_job(&mut self, id: LayerId, task: AiTask, output: PathBuf, args: Vec<String>, aux: Vec<PathBuf>) -> Result<()> {
        let (tx, rx) = unbounded();
        let python = self.ai.python.clone();
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
        let ctx = self.egui_ctx.clone();
        std::thread::Builder::new().name("photohop-ai".into()).spawn(move || {
            let _ = tx.send(run_sidecar(&python, &args, exe_dir.as_deref()));
            ctx.request_repaint();
        })?;
        self.status = format!("{} running…", task.label());
        self.ai.job = Some(AiJob { layer: id, task, output, rx, aux });
        Ok(())
    }

    /// Render the scene with the layer ungraded + its alpha, then ask Harmonizer for a colour fix.
    fn start_auto_colour(&mut self, id: LayerId, dir: &Path) -> Result<()> {
        let idx = self.doc.layer_index(id).context("layer missing")?;
        if idx == 0 {
            bail!("put a background layer under this one first");
        }
        let upto: Vec<LayerId> = self.doc.layers[..=idx].iter().map(|l| l.id).collect();
        let comp = self.composite_only(|l| upto.contains(&l), Some(id))?;
        let subject = self.composite_only(|l| l == id, Some(id))?;
        let scale = (1024.0 / comp.width.max(comp.height) as f32).min(1.0);
        let (w, h) = (((comp.width as f32 * scale) as u32).max(16), ((comp.height as f32 * scale) as u32).max(16));
        let rgba = ph_media::color::linear_premul_to_rgba8(&comp.pixels);
        let comp_img = image::RgbaImage::from_raw(comp.width, comp.height, rgba).context("composite image")?;
        let comp_rgb = image::DynamicImage::ImageRgba8(comp_img).resize_exact(w, h, image::imageops::FilterType::Triangle).to_rgb8();
        let alpha: Vec<u8> = subject.pixels.as_chunks::<4>().0.iter().map(|p| (p[3].to_f32().clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect();
        let mask_img = image::GrayImage::from_raw(subject.width, subject.height, alpha).context("mask image")?;
        let mask = image::DynamicImage::ImageLuma8(mask_img).resize_exact(w, h, image::imageops::FilterType::Triangle).to_luma8();
        let (cp, mp, op) = (dir.join("hz_comp.png"), dir.join("hz_mask.png"), dir.join("hz_out.png"));
        let _ = std::fs::remove_file(&op);
        comp_rgb.save(&cp)?;
        mask.save(&mp)?;
        let args = vec!["harmonize".into(), cp.to_string_lossy().into_owned(), mp.to_string_lossy().into_owned(), op.to_string_lossy().into_owned()];
        self.spawn_job(id, AiTask::AutoColour, op, args, vec![cp, mp])
    }

    /// Collect a finished AI job: the result becomes the layer's mask (undoable as a document edit).
    pub fn poll_ai(&mut self) {
        let Some(job) = &self.ai.job else { return };
        let Ok(result) = job.rx.try_recv() else { return };
        let job = self.ai.job.take().expect("job");
        let applied: Result<String> = (|| {
            let v = result.map_err(|e| anyhow::anyhow!(e))?;
            if job.task == AiTask::BodyMotion {
                let path = job.output.clone();
                self.edit("Body motion", |d| d.body_motion = Some(path));
                return Ok(format!(
                    "Body captured on {} of {} frames ({:.1} m from the camera). Export to Blender to get the animated body, its shadows and hand empties for props.",
                    v["detected"].as_u64().unwrap_or(0),
                    v["frames"].as_u64().unwrap_or(0),
                    v["distance_m"].as_f64().unwrap_or(0.0)
                ));
            }
            if job.task == AiTask::CameraSolve {
                let path = job.output.clone();
                self.edit("Camera solve", |d| d.camera_solve = Some(path));
                return Ok(format!(
                    "Camera solved: {} frames, camera moved {:.2} m{}. It will drive the Blender camera (File > Export to Blender).",
                    v["frames"].as_u64().unwrap_or(0),
                    v["travel_m"].as_f64().unwrap_or(0.0),
                    if v["ground"].as_bool() == Some(true) { ", floor found" } else { "" }
                ));
            }
            if let AiTask::FindGlass(_) = &job.task {
                let n = self.glass_apply_found(job.layer, &job.output)?;
                return Ok(if n == 0 {
                    "AI found no glass with that description. Try \"window\", \"screen\" or \"glass\", or draw the panes with the pen.".into()
                } else {
                    format!("Found {n} pane(s) as editable glass shapes. Choose what to show through them under Replace window / screen.")
                });
            }
            if let AiTask::GlassFront(_) = &job.task {
                let info = ph_media::video::probe(&job.output)?;
                let media = ph_core::MediaRef {
                    id: ph_core::MediaId::new(),
                    path: job.output.clone(),
                    kind: ph_core::MediaKind::Video { duration: info.duration, fps: info.fps, has_audio: false },
                    width: info.width,
                    height: info.height,
                };
                let mid = media.id;
                self.edit("AI keep in front", |d| d.media.push(media));
                self.glass_add_front(job.layer, mid);
                return Ok(format!("Tracked in {:.0}s: it now stays in front of the view.", v["seconds"].as_f64().unwrap_or(0.0)));
            }
            if job.task.is_video() {
                let info = ph_media::video::probe(&job.output)?;
                let media = ph_core::MediaRef {
                    id: ph_core::MediaId::new(),
                    path: job.output.clone(),
                    kind: ph_core::MediaKind::Video { duration: info.duration, fps: info.fps, has_audio: false },
                    width: info.width,
                    height: info.height,
                };
                let (mid, id) = (media.id, job.layer);
                let (w, h) = (info.width, info.height);
                self.edit(&job.task.label(), |d| {
                    d.media.push(media);
                    if let Some(l) = d.layer_mut(id) {
                        l.mask = Some(ph_core::LayerMask { enabled: true, invert: false, width: w, height: h, video: Some(mid) });
                    }
                });
                return Ok(format!(
                    "{} done in {:.0}s — applied as the layer's mask (tick Invert in the Mask section to cut the region out).",
                    job.task.label(),
                    v["seconds"].as_f64().unwrap_or(0.0)
                ));
            }
            if job.task == AiTask::AutoColour {
                let before = image::open(&job.aux[0])?.to_rgb8();
                let mask = image::open(&job.aux[1])?.to_luma8();
                let after = image::open(&job.output)?.to_rgb8();
                let [r, g, b] = crate::matching::fit_curves(before.as_raw(), after.as_raw(), mask.as_raw(), 8);
                let id = job.layer;
                self.edit("AI auto colour", |d| {
                    if let Some(l) = d.layer_mut(id) {
                        l.grade.enabled = true;
                        l.grade.curves.red = r;
                        l.grade.curves.green = g;
                        l.grade.curves.blue = b;
                    }
                });
                let f = &v["filters"];
                let pick = |k: &str| f[k].as_f64().unwrap_or(0.0);
                return Ok(format!(
                    "AI auto colour applied as R/G/B curves (temp {:+.2}, bright {:+.2}, contrast {:+.2}, sat {:+.2}, highlights {:+.2}, shadows {:+.2}). Edit them in Colour > Curves.",
                    pick("temperature"), pick("brightness"), pick("contrast"), pick("saturation"), pick("highlight"), pick("shadow")
                ));
            }
            if let AiTask::Remove(what) = &job.task {
                let (w, h) = ph_media::image_io::image_dimensions(&job.output)?;
                let media = ph_core::MediaRef {
                    id: ph_core::MediaId::new(),
                    path: job.output.clone(),
                    kind: ph_core::MediaKind::Image,
                    width: w,
                    height: h,
                };
                let (mid, src) = (media.id, job.layer);
                let context = !job.aux.is_empty();
                let name = format!("Removed: {what}");
                self.edit(&job.task.label(), |d| {
                    let Some(idx) = d.layer_index(src) else { return };
                    let base = d.layers[idx].clone();
                    d.media.push(media);
                    let mut l = ph_core::Layer::new(name, ph_core::LayerContent::Media { media: mid, trim_in: 0.0 }, &d.canvas);
                    l.transform = base.transform.clone();
                    l.patch_of = Some(src);
                    l.start = base.start;
                    l.length = base.length;
                    // With scene context the patch already looks like the composite: don't regrade it.
                    if !context {
                        l.grade = base.grade.clone();
                    }
                    d.layers.insert(idx + 1, l);
                });
                return Ok(format!(
                    "{} done in {:.0}s — added as a patch layer above (only the object area changes).",
                    job.task.label(),
                    v["seconds"].as_f64().unwrap_or(0.0)
                ));
            }
            if job.task == AiTask::Depth {
                let raster = PaintRaster::load(&job.output)?;
                let id = job.layer;
                self.edit("AI depth", |d| {
                    if let Some(l) = d.layer_mut(id) {
                        l.has_depth = true;
                        l.fx.lens.dof = true;
                    }
                });
                self.depths.insert(id, raster);
                return Ok(format!(
                    "Depth estimated in {:.0}s — depth of field is on. Click Pick focus, then click what should be sharp.",
                    v["seconds"].as_f64().unwrap_or(0.0)
                ));
            }
            if job.task == AiTask::Normals {
                let raster = PaintRaster::load(&job.output)?;
                let id = job.layer;
                self.edit("AI normals", |d| {
                    if let Some(l) = d.layer_mut(id) {
                        l.has_normals = true;
                        l.fx.relight.enabled = true;
                    }
                });
                self.normals.insert(id, raster);
                return Ok(format!("Normals estimated in {:.1}s — Relight is on (Integration section).", v["seconds"].as_f64().unwrap_or(0.0)));
            }
            let raster = load_mask(&job.output)?;
            let (w, h) = (raster.width, raster.height);
            let id = job.layer;
            self.edit(&job.task.label(), |d| {
                if let Some(l) = d.layer_mut(id) {
                    l.mask = Some(ph_core::LayerMask { enabled: true, invert: false, width: w, height: h, video: None });
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
        let e = run_sidecar("definitely-not-a-python-binary", &["info".into()], None).unwrap_err();
        assert!(e.contains("could not start"), "{e}");
    }
}

#[cfg(test)]
mod real_project {
    /// Opt-in end-to-end AI auto colour:
    /// `PHOTOHOP_TEST_AUTOCOLOUR=in.phop:out.phop PHOTOHOP_AI_PYTHON=… cargo test -p photohop autocolour_real -- --nocapture`
    #[test]
    fn autocolour_real() {
        let Some(spec) = std::env::var("PHOTOHOP_TEST_AUTOCOLOUR").ok() else { return };
        let (inp, out) = spec.rsplit_once(':').unwrap();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else { return };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut app = crate::app::App::new(&egui::Context::default(), &device, &queue);
        app.open(std::path::Path::new(inp)).unwrap();
        app.selected = app.doc.layers.last().map(|l| l.id);
        // Media frames load asynchronously for videos; stills load on first request.
        app.start_ai(super::AiTask::AutoColour).unwrap();
        while app.ai_running() {
            std::thread::sleep(std::time::Duration::from_millis(200));
            app.poll_ai();
        }
        eprintln!("{}", app.status);
        app.save_to(std::path::Path::new(out)).unwrap();
    }
}
