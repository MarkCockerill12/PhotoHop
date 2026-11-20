//! Bridges the document to the GPU compositor and the egui viewer texture.

use crate::paint::PaintRaster;
use glam::Vec2;
use half::f16;
use ph_core::doc::{LayerContent, MediaRef};
use ph_core::{LayerId, Project};
use ph_gpu::{Compositor, FxSample, LayerDraw, ShadowDraw};
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
    AiBox,
    Select,
}

/// State captured when a viewer drag starts (values are absolute, so drags never drift).
#[derive(Clone, Copy)]
pub struct Drag {
    pub mode: DragMode,
    pub layer: Option<LayerId>,
    /// Painting into the layer mask instead of a paint raster.
    pub target_mask: bool,
    pub start: Vec2,
    pub position: [f32; 2],
    pub scale: [f32; 2],
    pub rotation: f32,
}

pub struct Viewer {
    pub gpu: Compositor,
    pub drag: Option<Drag>,
    /// Selection being drawn (canvas pixels): rect/ellipse corners or lasso points.
    pub select_pts: Vec<Vec2>,
    /// AI region box being drawn (canvas pixels), see `AiState::picking_box`.
    pub ai_box: Option<(Vec2, Vec2)>,
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
            select_pts: Vec::new(),
            ai_box: None,
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

/// GPU effect parameters for a layer at time `t` (None when no effect is active).
fn fx_sample(fx: &ph_core::LayerFx, t: f64, fps: f64, has_normals: bool) -> Option<FxSample> {
    let r = &fx.relight;
    let active = fx.light_wrap.at(t) > 0.0
        || fx.blur.at(t) > 0.0
        || fx.sharpen.at(t) > 0.0
        || fx.grain.at(t) > 0.0
        || fx.haze.at(t) > 0.0
        || fx.pixelate >= 1.5
        || fx.posterize >= 2.0
        || r.enabled;
    if !active {
        return None;
    }
    let d = r.direction(t);
    Some(FxSample {
        wrap: [fx.light_wrap.at(t), fx.light_wrap_width, fx.blur.at(t), fx.sharpen.at(t)],
        grain: [fx.grain.at(t), fx.grain_size, f32::from(u8::from(fx.grain_mono)), ((t * fps).round() % 997.0) as f32],
        relight0: [d[0], d[1], d[2], f32::from(u8::from(r.enabled))],
        relight1: [r.color[0], r.color[1], r.color[2], r.intensity.at(t)],
        relight2: [r.ambient.at(t), r.rim.at(t), r.wrap, r.amount.at(t)],
        relight3: [r.rim_color[0], r.rim_color[1], r.rim_color[2], f32::from(u8::from(has_normals))],
        scale: [0.0; 4],
        haze: [fx.haze_color[0], fx.haze_color[1], fx.haze_color[2], fx.haze.at(t)],
        game: [fx.pixelate, fx.posterize, 0.0, 0.0],
    })
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
    masks: &HashMap<LayerId, PaintRaster>,
    normals: &HashMap<LayerId, PaintRaster>,
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
        let mut mask_srgb = false;
        let mask = match (&layer.mask, masks.get(&layer.id)) {
            (Some(m), _) if m.enabled && m.video.is_some() => {
                let mv = m.video.and_then(|id| doc.media(id));
                match mv.and_then(|mref| fetch(mref, layer.media_time(t))) {
                    Some((mkey, frame)) => {
                        if !gpu.has_source(mkey, 0) {
                            gpu.upload(mkey, 0, frame.width, frame.height, &frame.pixels);
                        }
                        keep.insert(mkey);
                        mask_srgb = true;
                        Some(mkey)
                    }
                    None => None,
                }
            }
            (Some(m), Some(r)) if m.enabled => {
                let mkey = hash_of(("mask", layer.id));
                if !gpu.has_source(mkey, r.revision) {
                    let f = r.to_frame();
                    gpu.upload(mkey, r.revision, f.width, f.height, &f.pixels);
                }
                keep.insert(mkey);
                Some(mkey)
            }
            _ => None,
        };
        let normals_key = match (layer.has_normals, normals.get(&layer.id)) {
            (true, Some(r)) if layer.fx.relight.enabled => {
                let nkey = hash_of(("normals", layer.id));
                if !gpu.has_source(nkey, r.revision) {
                    let f = r.to_frame();
                    gpu.upload(nkey, r.revision, f.width, f.height, &f.pixels);
                }
                keep.insert(nkey);
                Some(nkey)
            }
            _ => None,
        };
        let matrix = layer.transform.matrix(t, size);
        let fx = fx_sample(&layer.fx, t, doc.canvas.fps, normals_key.is_some());
        let shadow = layer.fx.shadow.enabled.then(|| {
            let s = &layer.fx.shadow;
            ShadowDraw {
                cast: s.cast_matrix(t, matrix, size, 1.0),
                contact: s.contact_matrix(matrix, size),
                softness: s.softness.at(t),
                contact_softness: s.softness.at(t) * 0.25 + 1.5,
                opacity: s.opacity.at(t),
                contact_opacity: s.contact.at(t),
                color: s.color,
            }
        });
        let graded = layer.grade.enabled && !layer.grade.is_identity();
        let grade = graded.then(|| layer.grade.sample(t));
        let curves = (graded && !layer.grade.curves.is_identity()).then(|| layer.grade.curves.bake());
        draws.push(LayerDraw {
            source: key,
            size,
            matrix,
            opacity: layer.opacity.at(t),
            blend: layer.blend,
            mask,
            mask_invert: layer.mask.as_ref().is_some_and(|m| m.invert),
            mask_srgb,
            grade,
            curves,
            fx,
            normals: normals_key,
            shadow,
        });
    }
    gpu.retain_sources(&keep);
    gpu.render(doc.canvas.width, doc.canvas.height, doc.canvas.background, &draws);
}
