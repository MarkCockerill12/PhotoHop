//! Bridges the document to the GPU compositor and the egui viewer texture.

use crate::paint::PaintRaster;
use glam::Vec2;
use half::f16;
use ph_core::doc::{LayerContent, MediaRef};
use ph_core::{LayerId, Project};
use ph_gpu::{Compositor, LayerDraw};
use ph_media::Frame;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

#[derive(Clone, Copy, PartialEq)]
pub enum DragMode {
    Move,
    /// Scale from a corner (index 0..4, source-space corner order TL, TR, BR, BL).
    Scale,
    Rotate,
    Crop,
    Paint,
}

/// State captured when a viewer drag starts (values are absolute, so drags never drift).
#[derive(Clone, Copy)]
pub struct Drag {
    pub mode: DragMode,
    pub layer: Option<LayerId>,
    pub start: Vec2,
    pub position: [f32; 2],
    pub scale: [f32; 2],
    pub rotation: f32,
}

pub struct Viewer {
    pub gpu: Compositor,
    pub drag: Option<Drag>,
    /// Pending crop rectangle in canvas pixels (two corners).
    pub crop: Option<(Vec2, Vec2)>,
    pub tex_id: Option<egui::TextureId>,
    tex_gen: u64,
    /// Screen pixels per canvas pixel.
    pub zoom: f32,
    /// Offset of the canvas centre from the viewport centre (screen pixels).
    pub pan: egui::Vec2,
    pub fit_pending: bool,
    /// Where the canvas was drawn last frame.
    pub canvas_rect: egui::Rect,
}

impl Viewer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            gpu: Compositor::new(device, queue),
            drag: None,
            crop: None,
            tex_id: None,
            tex_gen: 0,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            fit_pending: true,
            canvas_rect: egui::Rect::NOTHING,
        }
    }

    /// Register/refresh the composite as an egui texture after the compositor (re)created it.
    pub fn sync_texture(&mut self, rs: &egui_wgpu::RenderState) {
        let Some(view) = self.gpu.display_view() else { return };
        let mut renderer = rs.renderer.write();
        match self.tex_id {
            Some(id) if self.tex_gen == self.gpu.display_generation => {
                let _ = id;
            }
            Some(id) => {
                renderer.update_egui_texture_from_wgpu_texture(&rs.device, view, wgpu::FilterMode::Linear, id);
                self.tex_gen = self.gpu.display_generation;
            }
            None => {
                self.tex_id = Some(renderer.register_native_texture(&rs.device, view, wgpu::FilterMode::Linear));
                self.tex_gen = self.gpu.display_generation;
            }
        }
    }

    pub fn canvas_to_screen(&self, p: Vec2) -> egui::Pos2 {
        self.canvas_rect.min + egui::vec2(p.x, p.y) * self.zoom
    }

    pub fn screen_to_canvas(&self, p: egui::Pos2) -> Vec2 {
        let v = (p - self.canvas_rect.min) / self.zoom;
        Vec2::new(v.x, v.y)
    }
}

fn hash_of(parts: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

fn paint_key(id: LayerId) -> u64 {
    hash_of(("paint", id))
}

/// Upload every source visible at `t` and composite. `fetch` supplies media frames
/// (non-blocking cache for the viewer, full-resolution blocking readers for export).
pub fn composite(
    gpu: &mut Compositor,
    doc: &Project,
    t: f64,
    paint: &HashMap<LayerId, PaintRaster>,
    mut fetch: impl FnMut(&MediaRef, f64) -> Option<(u64, Frame)>,
) {
    let mut draws = Vec::new();
    let mut keep = HashSet::new();
    for layer in doc.layers.iter().filter(|l| l.visible && l.active_at(t)) {
        let size = doc.layer_source_size(layer);
        let key = match &layer.content {
            LayerContent::Media { media, .. } => {
                let Some(m) = doc.media(*media) else { continue };
                let Some((key, frame)) = fetch(m, layer.media_time(t)) else { continue };
                if !gpu.has_source(key, 0) {
                    gpu.upload(key, 0, frame.width, frame.height, &frame.pixels);
                }
                key
            }
            LayerContent::Paint { .. } => {
                let Some(r) = paint.get(&layer.id) else { continue };
                let key = paint_key(layer.id);
                if !gpu.has_source(key, r.revision) {
                    let f = r.to_frame();
                    gpu.upload(key, r.revision, f.width, f.height, &f.pixels);
                }
                key
            }
            LayerContent::Solid { color, .. } => {
                let c = color.at(t);
                let key = hash_of(("solid", layer.id, c.map(f32::to_bits)));
                if !gpu.has_source(key, 0) {
                    let px = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]].map(f16::from_f32);
                    gpu.upload(key, 0, 1, 1, &px);
                }
                key
            }
        };
        keep.insert(key);
        draws.push(LayerDraw {
            source: key,
            size,
            matrix: layer.transform.matrix(t, size),
            opacity: layer.opacity.at(t),
            blend: layer.blend,
        });
    }
    gpu.retain_sources(&keep);
    gpu.render(doc.canvas.width, doc.canvas.height, doc.canvas.background, &draws);
}
