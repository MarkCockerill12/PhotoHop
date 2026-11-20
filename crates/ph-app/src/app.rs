//! Application state and document-level actions (new/open/save/import/undo, layer ops).

use crate::export::ExportState;
use crate::media_cache::MediaCache;
use crate::paint::{BrushSettings, Patch, PaintRaster};
use crate::render::Viewer;
use anyhow::{Context, Result};
use ph_core::doc::{LayerContent, MediaKind, MediaRef};
use ph_core::{History, Layer, LayerId, MediaId, Project, project_file};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Move,
    Crop,
    Brush,
    Eraser,
    Hand,
}

impl Tool {
    pub const ALL: [Tool; 5] = [Tool::Move, Tool::Crop, Tool::Brush, Tool::Eraser, Tool::Hand];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Move => "Move (V)",
            Tool::Crop => "Crop (C)",
            Tool::Brush => "Brush (B)",
            Tool::Eraser => "Eraser (E)",
            Tool::Hand => "Hand (H)",
        }
    }

    pub fn icon(self) -> egui_material_icons::MaterialIcon {
        use egui_material_icons::icons::*;
        match self {
            Tool::Move => ICON_OPEN_WITH,
            Tool::Crop => ICON_CROP,
            Tool::Brush => ICON_BRUSH,
            Tool::Eraser => ICON_INK_ERASER,
            Tool::Hand => ICON_PAN_TOOL,
        }
    }
}

/// Undo entries that hold pixels (paint strokes) live outside the document history.
pub enum PixelUndo {
    /// `mask` = the stroke was on the layer's mask rather than its paint raster.
    Stroke { layer: LayerId, mask: bool, patch: Patch },
}

pub struct App {
    pub doc: Project,
    pub history: History,
    pub project_path: Option<PathBuf>,
    pub selected: Option<LayerId>,
    pub tool: Tool,
    pub time: f64,
    pub playing: bool,
    pub play_anchor: Option<(Instant, f64)>,
    pub media: MediaCache,
    pub paint: HashMap<LayerId, PaintRaster>,
    /// Layer masks (red channel = coverage, white = visible).
    pub masks: HashMap<LayerId, PaintRaster>,
    /// AI-estimated normal maps (RGB = n·0.5+0.5) used by relighting.
    pub normals: HashMap<LayerId, PaintRaster>,
    /// Brush/eraser paint into the selected layer's mask instead of its pixels.
    pub edit_mask: bool,
    pub match_mode: crate::matching::MatchMode,
    pub match_reference: Option<LayerId>,
    pub curve_channel: crate::ui::CurveChannel,
    /// Index of the curve point being dragged.
    pub curve_drag: Option<usize>,
    pub pixel_undo: Vec<PixelUndo>,
    pub pixel_redo: Vec<PixelUndo>,
    pub brush: BrushSettings,
    pub viewer: Viewer,
    pub export: ExportState,
    pub blender_job: Option<crate::blender::BlenderJob>,
    pub scopes: crate::scopes::Scopes,
    pub ai: crate::ai::AiState,
    pub egui_ctx: egui::Context,
    pub status: String,
    /// Unsaved changes since last save.
    pub dirty: bool,
    pub auto_key: bool,
}

/// Fraction of the frame height just below the lowest row that has a meaningful run of solid pixels.
pub fn lowest_solid_row(frame: &ph_media::Frame, mask: impl Fn(f32, f32) -> f32) -> f32 {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let need = (w / 200).max(2);
    for y in (0..h).rev() {
        let v = (y as f32 + 0.5) / h as f32;
        let solid = (0..w)
            .filter(|&x| frame.pixels[(y * w + x) * 4 + 3].to_f32() * mask((x as f32 + 0.5) / w as f32, v) > 0.5)
            .count();
        if solid >= need {
            return (y + 1) as f32 / h as f32;
        }
    }
    1.0
}

impl App {
    pub fn new(ctx: &egui::Context, device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            doc: Project::new("Untitled", 1920, 1080, 24.0, 10.0),
            history: History::new(200),
            project_path: None,
            selected: None,
            tool: Tool::Move,
            time: 0.0,
            playing: false,
            play_anchor: None,
            media: MediaCache::new(ctx.clone()),
            paint: HashMap::new(),
            masks: HashMap::new(),
            normals: HashMap::new(),
            edit_mask: false,
            match_mode: crate::matching::MatchMode::ColourCast,
            match_reference: None,
            curve_channel: Default::default(),
            curve_drag: None,
            pixel_undo: Vec::new(),
            pixel_redo: Vec::new(),
            brush: BrushSettings::default(),
            viewer: Viewer::new(device, queue),
            export: ExportState::default(),
            blender_job: None,
            scopes: crate::scopes::Scopes::default(),
            ai: crate::ai::AiState::default(),
            egui_ctx: ctx.clone(),
            status: "Import a photo or video to start (File > Import, or drop files onto the window).".into(),
            dirty: false,
            auto_key: false,
        }
    }

    pub fn title(&self) -> String {
        let name = self.project_path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
        format!("{}{} — PhotoHop", name.unwrap_or_else(|| self.doc.name.clone()), if self.dirty { " •" } else { "" })
    }

    pub fn selected_layer(&self) -> Option<&Layer> {
        self.selected.and_then(|id| self.doc.layer(id))
    }

    // ---------- document edits (snapshot undo) ----------

    /// Apply a one-shot edit with an undo entry.
    pub fn edit(&mut self, label: &str, f: impl FnOnce(&mut Project)) {
        self.history.record(label, &self.doc);
        f(&mut self.doc);
        self.dirty = true;
    }

    /// Start/continue a gesture (drag, slider); call [`App::end_gesture`] when it ends.
    pub fn gesture(&mut self, label: &str) -> &mut Project {
        self.history.begin(label, &self.doc);
        self.dirty = true;
        &mut self.doc
    }

    pub fn end_gesture(&mut self) {
        self.history.commit(&self.doc);
    }

    pub fn undo(&mut self) {
        if let Some(l) = self.history.undo(&mut self.doc) {
            self.status = format!("Undo: {l}");
        } else if let Some(PixelUndo::Stroke { layer, mask, patch }) = self.pixel_undo.pop()
            && let Some(r) = if mask { self.masks.get_mut(&layer) } else { self.paint.get_mut(&layer) }
        {
            let redo = r.swap_patch(patch);
            self.pixel_redo.push(PixelUndo::Stroke { layer, mask, patch: redo });
            self.status = "Undo: brush stroke".into();
        }
        self.fix_selection();
    }

    pub fn redo(&mut self) {
        if let Some(l) = self.history.redo(&mut self.doc) {
            self.status = format!("Redo: {l}");
        } else if let Some(PixelUndo::Stroke { layer, mask, patch }) = self.pixel_redo.pop()
            && let Some(r) = if mask { self.masks.get_mut(&layer) } else { self.paint.get_mut(&layer) }
        {
            let undo = r.swap_patch(patch);
            self.pixel_undo.push(PixelUndo::Stroke { layer, mask, patch: undo });
            self.status = "Redo: brush stroke".into();
        }
        self.fix_selection();
    }

    fn fix_selection(&mut self) {
        if self.selected.is_some_and(|id| self.doc.layer(id).is_none()) {
            self.selected = self.doc.layers.last().map(|l| l.id);
        }
    }

    // ---------- project files ----------

    pub fn new_project(&mut self, w: u32, h: u32, fps: f64, duration: f64) {
        self.doc = Project::new("Untitled", w, h, fps, duration);
        self.history = History::new(200);
        self.project_path = None;
        self.selected = None;
        self.paint.clear();
        self.masks.clear();
        self.normals.clear();
        self.edit_mask = false;
        self.match_reference = None;
        self.pixel_undo.clear();
        self.pixel_redo.clear();
        self.media.clear();
        self.time = 0.0;
        self.dirty = false;
        self.viewer.fit_pending = true;
    }

    pub fn open(&mut self, path: &Path) -> Result<()> {
        let doc = project_file::load(path)?;
        self.new_project(doc.canvas.width, doc.canvas.height, doc.canvas.fps, doc.canvas.duration);
        for l in &doc.layers {
            if let LayerContent::Paint { width, height } = l.content {
                let file = project_file::paint_layer_path(path, l.id);
                let raster = PaintRaster::load(&file).unwrap_or_else(|_| PaintRaster::new(width, height));
                self.paint.insert(l.id, raster);
            }
            if l.has_normals
                && let Ok(r) = PaintRaster::load(&project_file::normals_path(path, l.id))
            {
                self.normals.insert(l.id, r);
            }
            if let Some(m) = &l.mask {
                let file = project_file::mask_path(path, l.id);
                let raster = PaintRaster::load(&file).unwrap_or_else(|_| PaintRaster::filled(m.width, m.height, [1.0; 4]));
                self.masks.insert(l.id, raster);
            }
        }
        self.selected = doc.layers.last().map(|l| l.id);
        self.doc = doc;
        self.project_path = Some(path.to_path_buf());
        self.status = format!("Opened {}", path.display());
        Ok(())
    }

    pub fn save_to(&mut self, path: &Path) -> Result<()> {
        let path = path.with_extension(project_file::EXTENSION);
        // Move AI results out of the temp folder into the project's assets so they survive.
        let tmp = crate::ai::results_dir();
        let assets = project_file::assets_dir(&path);
        for m in &mut self.doc.media {
            if m.path.starts_with(&tmp) {
                std::fs::create_dir_all(&assets)?;
                let dest = assets.join(m.path.file_name().context("media file name")?);
                std::fs::copy(&m.path, &dest).context("copying AI result into project assets")?;
                m.path = dest;
            }
        }
        project_file::save(&self.doc, &path)?;
        for (id, raster) in &self.paint {
            if self.doc.layer(*id).is_some() {
                raster.save(&project_file::paint_layer_path(&path, *id)).context("saving paint layer")?;
            }
        }
        for (id, raster) in &self.normals {
            if self.doc.layer(*id).is_some_and(|l| l.has_normals) {
                raster.save(&project_file::normals_path(&path, *id)).context("saving normal map")?;
            }
        }
        for (id, raster) in &self.masks {
            if self.doc.layer(*id).is_some_and(|l| l.mask.is_some()) {
                raster.save(&project_file::mask_path(&path, *id)).context("saving layer mask")?;
            }
        }
        self.project_path = Some(path.clone());
        self.dirty = false;
        self.status = format!("Saved {}", path.display());
        Ok(())
    }

    // ---------- import & layers ----------

    /// Import a still or video as a new layer on top. The first import sizes the canvas
    /// (and, for video, the timeline) to match.
    pub fn import(&mut self, path: &Path) -> Result<()> {
        let first = self.doc.layers.is_empty();
        let media = if ph_media::video::is_video_path(path) {
            let info = ph_media::video::probe(path)?;
            MediaRef {
                id: MediaId::new(),
                path: path.to_path_buf(),
                kind: MediaKind::Video { duration: info.duration, fps: info.fps, has_audio: info.has_audio },
                width: info.width,
                height: info.height,
            }
        } else {
            let (w, h) = ph_media::image_io::image_dimensions(path)?;
            MediaRef { id: MediaId::new(), path: path.to_path_buf(), kind: MediaKind::Image, width: w, height: h }
        };
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Layer".into());
        let mid = media.id;
        let video = match media.kind {
            MediaKind::Video { duration, fps, .. } => Some((duration, fps)),
            MediaKind::Image => None,
        };
        let (mw, mh) = (media.width, media.height);
        self.edit("Import", |doc| {
            if first {
                doc.canvas.width = mw;
                doc.canvas.height = mh;
                doc.name = name.clone();
            }
            if let Some((duration, fps)) = video
                && (first || doc.canvas.duration < duration)
            {
                doc.canvas.duration = duration;
                if first {
                    doc.canvas.fps = fps;
                }
            }
            doc.media.push(media);
            let layer = Layer::new(name, LayerContent::Media { media: mid, trim_in: 0.0 }, &doc.canvas);
            doc.layers.push(layer);
        });
        self.selected = self.doc.layers.last().map(|l| l.id);
        if first {
            self.viewer.fit_pending = true;
        }
        self.status = format!("Imported {}", path.display());
        Ok(())
    }

    pub fn add_paint_layer(&mut self) {
        let (w, h) = (self.doc.canvas.width, self.doc.canvas.height);
        let id = LayerId::new();
        self.edit("New paint layer", |doc| {
            let mut l = Layer::new("Paint", LayerContent::Paint { width: w, height: h }, &doc.canvas);
            l.id = id;
            doc.layers.push(l);
        });
        self.paint.insert(id, PaintRaster::new(w, h));
        self.selected = Some(id);
        self.tool = Tool::Brush;
    }

    pub fn add_solid_layer(&mut self) {
        let (w, h) = (self.doc.canvas.width, self.doc.canvas.height);
        self.edit("New colour layer", |doc| {
            let color = ph_core::Animated::new([0.5, 0.5, 0.5, 1.0]);
            doc.layers.push(Layer::new("Colour", LayerContent::Solid { color, width: w, height: h }, &doc.canvas));
        });
        self.selected = self.doc.layers.last().map(|l| l.id);
    }

    pub fn delete_selected(&mut self) {
        if let Some(id) = self.selected {
            self.edit("Delete layer", |doc| doc.layers.retain(|l| l.id != id));
            self.fix_selection();
        }
    }

    pub fn duplicate_selected(&mut self) {
        let Some(id) = self.selected else { return };
        let Some(idx) = self.doc.layer_index(id) else { return };
        let new_id = LayerId::new();
        self.edit("Duplicate layer", |doc| {
            let mut l = doc.layers[idx].clone();
            l.id = new_id;
            l.name = format!("{} copy", l.name);
            doc.layers.insert(idx + 1, l);
        });
        if let Some(r) = self.paint.get(&id) {
            self.paint.insert(new_id, r.duplicate());
        }
        if let Some(r) = self.masks.get(&id) {
            self.masks.insert(new_id, r.duplicate());
        }
        if let Some(r) = self.normals.get(&id) {
            self.normals.insert(new_id, r.duplicate());
        }
        self.selected = Some(new_id);
    }

    /// Move a layer up (+1) or down (-1) in the stack.
    pub fn move_layer(&mut self, id: LayerId, delta: i32) {
        let Some(i) = self.doc.layer_index(id) else { return };
        let j = i as i32 + delta;
        if j < 0 || j >= self.doc.layers.len() as i32 {
            return;
        }
        self.edit("Reorder layers", |doc| doc.layers.swap(i, j as usize));
    }

    // ---------- masks & colour matching ----------

    /// Give the selected layer an all-white (fully visible) mask at its source resolution.
    pub fn add_mask(&mut self) {
        let Some(layer) = self.selected_layer() else { return };
        if layer.mask.is_some() {
            return;
        }
        let id = layer.id;
        let size = self.doc.layer_source_size(layer);
        let (w, h) = (size.x.round().max(1.0) as u32, size.y.round().max(1.0) as u32);
        self.edit("Add mask", |d| {
            if let Some(l) = d.layer_mut(id) {
                l.mask = Some(ph_core::LayerMask { enabled: true, invert: false, width: w, height: h, video: None });
            }
        });
        self.masks.insert(id, PaintRaster::filled(w, h, [1.0; 4]));
        self.edit_mask = true;
        self.status = "Mask added. Brush hides, eraser reveals (while \"Edit mask\" is on).".into();
    }

    pub fn delete_mask(&mut self) {
        let Some(id) = self.selected else { return };
        self.edit("Delete mask", |d| {
            if let Some(l) = d.layer_mut(id) {
                l.mask = None;
            }
        });
        self.edit_mask = false;
    }

    /// Current frame of a layer's source (preview resolution for video).
    pub fn layer_frame(&mut self, id: LayerId) -> Option<ph_media::Frame> {
        let layer = self.doc.layer(id)?;
        match &layer.content {
            LayerContent::Media { media, .. } => {
                let t = layer.media_time(self.time);
                let m = self.doc.media(*media)?.clone();
                self.media.frame(&m, t).map(|(_, f)| f)
            }
            LayerContent::Paint { .. } => self.paint.get(&id).map(PaintRaster::to_frame),
            LayerContent::Solid { color, .. } => {
                let c = color.at(self.time);
                let px = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]].map(half::f16::from_f32);
                Some(ph_media::Frame::new(1, 1, px.to_vec()))
            }
        }
    }

    /// Ground line (fraction of layer height) where the subject's visible pixels end — the feet.
    pub fn find_ground(&mut self, id: LayerId) -> Option<f32> {
        let frame = self.layer_frame(id)?;
        let mask = self.doc.layer(id).and_then(|l| l.mask.as_ref().filter(|m| m.enabled).map(|m| m.invert));
        let raster = self.masks.get(&id);
        Some(lowest_solid_row(&frame, |u, v| match (raster, mask) {
            (Some(r), Some(inv)) => {
                let x = ((u * r.width as f32) as u32).min(r.width - 1);
                let y = ((v * r.height as f32) as u32).min(r.height - 1);
                let m = r.data[(y * r.width + x) as usize][0];
                if inv { 1.0 - m } else { m }
            }
            _ => 1.0,
        }))
    }

    /// Match the selected layer's colours to a reference layer (default: the layer below it).
    pub fn match_colour(&mut self) -> Result<()> {
        use crate::matching::{stats, transfer};
        let id = self.selected.context("select the layer to adjust")?;
        let idx = self.doc.layer_index(id).context("layer missing")?;
        let reference = self
            .match_reference
            .filter(|r| *r != id && self.doc.layer(*r).is_some())
            .or_else(|| idx.checked_sub(1).map(|i| self.doc.layers[i].id))
            .context("add a background layer below this one (or pick a reference) first")?;
        let src = self.layer_frame(id).context("this layer has no pixels yet (video still decoding?)")?;
        let target = self.layer_frame(reference).context("reference layer has no pixels yet")?;
        let mask = self.doc.layer(id).and_then(|l| l.mask.as_ref().filter(|m| m.enabled).map(|m| m.invert));
        let raster = self.masks.get(&id);
        let mask_fn = |u: f32, v: f32| -> f32 {
            let Some(r) = raster else { return 1.0 };
            let x = ((u * r.width as f32) as u32).min(r.width - 1);
            let y = ((v * r.height as f32) as u32).min(r.height - 1);
            let m = r.data[(y * r.width + x) as usize][0];
            if mask == Some(true) { 1.0 - m } else { m }
        };
        let s = stats(&src, mask.map(|_| &mask_fn as &dyn Fn(f32, f32) -> f32));
        let t = stats(&target, None);
        if s.weight <= 0.0 || t.weight <= 0.0 {
            anyhow::bail!("nothing visible to measure");
        }
        let (scale, offset) = transfer(&s, &t, self.match_mode);
        let (time, auto) = (self.time, self.auto_key);
        self.edit("Match colour", |d| {
            if let Some(l) = d.layer_mut(id) {
                l.grade.match_scale = scale;
                l.grade.match_offset = offset;
                if l.grade.match_amount.at(time) <= 0.0 {
                    l.grade.match_amount.set(time, 1.0, auto);
                }
            }
        });
        let name = self.doc.layer(reference).map(|l| l.name.clone()).unwrap_or_default();
        self.status = format!("Matched colour to \"{name}\" ({}). Use the Match amount slider to blend.", self.match_mode.label());
        Ok(())
    }

    // ---------- playback ----------

    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
        self.play_anchor = self.playing.then(|| (Instant::now(), self.time));
    }

    pub fn advance_playback(&mut self) {
        if let (true, Some((start, t0))) = (self.playing, self.play_anchor) {
            let dur = self.doc.canvas.duration.max(1e-3);
            let t = t0 + start.elapsed().as_secs_f64();
            self.time = self.doc.canvas.snap_to_frame(t % dur);
        }
    }

    pub fn set_time(&mut self, t: f64) {
        self.time = self.doc.canvas.snap_to_frame(t.clamp(0.0, self.doc.canvas.duration));
        if self.playing {
            self.play_anchor = Some((Instant::now(), self.time));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use half::f16;

    #[test]
    fn ground_is_below_lowest_solid_row() {
        // 10x10, opaque rows 2..=6, transparent below.
        let px: Vec<f16> = (0..100)
            .flat_map(|i| {
                let a = if (2..=6).contains(&(i / 10)) { 1.0 } else { 0.0 };
                [a, a, a, a].map(f16::from_f32)
            })
            .collect();
        let f = ph_media::Frame::new(10, 10, px);
        assert!((lowest_solid_row(&f, |_, _| 1.0) - 0.7).abs() < 1e-6);
    }
}
