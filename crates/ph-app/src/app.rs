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
    Stroke { layer: LayerId, patch: Patch },
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
    pub pixel_undo: Vec<PixelUndo>,
    pub pixel_redo: Vec<PixelUndo>,
    pub brush: BrushSettings,
    pub viewer: Viewer,
    pub export: ExportState,
    pub status: String,
    /// Unsaved changes since last save.
    pub dirty: bool,
    pub auto_key: bool,
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
            pixel_undo: Vec::new(),
            pixel_redo: Vec::new(),
            brush: BrushSettings::default(),
            viewer: Viewer::new(device, queue),
            export: ExportState::default(),
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
        } else if let Some(PixelUndo::Stroke { layer, patch }) = self.pixel_undo.pop()
            && let Some(r) = self.paint.get_mut(&layer)
        {
            let redo = r.swap_patch(patch);
            self.pixel_redo.push(PixelUndo::Stroke { layer, patch: redo });
            self.status = "Undo: brush stroke".into();
        }
        self.fix_selection();
    }

    pub fn redo(&mut self) {
        if let Some(l) = self.history.redo(&mut self.doc) {
            self.status = format!("Redo: {l}");
        } else if let Some(PixelUndo::Stroke { layer, patch }) = self.pixel_redo.pop()
            && let Some(r) = self.paint.get_mut(&layer)
        {
            let undo = r.swap_patch(patch);
            self.pixel_undo.push(PixelUndo::Stroke { layer, patch: undo });
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
        }
        self.selected = doc.layers.last().map(|l| l.id);
        self.doc = doc;
        self.project_path = Some(path.to_path_buf());
        self.status = format!("Opened {}", path.display());
        Ok(())
    }

    pub fn save_to(&mut self, path: &Path) -> Result<()> {
        let path = path.with_extension(project_file::EXTENSION);
        project_file::save(&self.doc, &path)?;
        for (id, raster) in &self.paint {
            if self.doc.layer(*id).is_some() {
                raster.save(&project_file::paint_layer_path(&path, *id)).context("saving paint layer")?;
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
            let mut copy = PaintRaster::new(r.width, r.height);
            copy.data.clone_from(&r.data);
            self.paint.insert(new_id, copy);
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
