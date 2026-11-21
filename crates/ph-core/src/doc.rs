//! The document: canvas, media references and the layer stack.
//! Pixel data never lives here (media is referenced by path, paint rasters are sidecar files),
//! so a whole document is cheap to clone — that is what undo snapshots rely on.

use crate::anim::Animated;
use crate::blend::BlendMode;
use crate::fx::LayerFx;
use crate::grade::{Grade, LayerMask};
use glam::{Affine2, Vec2};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LayerId(pub Uuid);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediaId(pub Uuid);

impl LayerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl MediaId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for LayerId {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for MediaId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Timeline length in seconds. Stills projects use one frame.
    pub duration: f64,
    /// Linear RGBA shown behind all layers (alpha 0 = transparent export).
    pub background: [f32; 4],
}

impl Canvas {
    pub fn frame_count(&self) -> u64 {
        (self.duration * self.fps).round().max(1.0) as u64
    }

    pub fn frame_time(&self, frame: u64) -> f64 {
        frame as f64 / self.fps
    }

    pub fn snap_to_frame(&self, t: f64) -> f64 {
        (t * self.fps).round() / self.fps
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MediaKind {
    Image,
    Video { duration: f64, fps: f64, has_audio: bool },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MediaRef {
    pub id: MediaId,
    /// Absolute in memory; written relative to the project file when possible.
    pub path: PathBuf,
    pub kind: MediaKind,
    pub width: u32,
    pub height: u32,
}

impl MediaRef {
    pub fn is_video(&self) -> bool {
        matches!(self.kind, MediaKind::Video { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LayerContent {
    /// A still or video file. `trim_in` is the source time shown at the layer's start.
    Media { media: MediaId, trim_in: f64 },
    /// A raster the user paints on. Pixels live in a sidecar file in the project's assets folder.
    Paint { width: u32, height: u32 },
    /// Flat colour filling the layer rectangle (linear RGBA, straight alpha).
    Solid { color: Animated<[f32; 4]>, width: u32, height: u32 },
}

/// Placement of the layer's source rectangle on the canvas. `position` is where the source's
/// centre lands (canvas pixels); scale and rotation (degrees) act about that centre.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerTransform {
    pub position: Animated<[f32; 2]>,
    pub scale: Animated<[f32; 2]>,
    pub rotation: Animated<f32>,
}

impl LayerTransform {
    pub fn centered(canvas: &Canvas) -> Self {
        Self {
            position: Animated::new([canvas.width as f32 * 0.5, canvas.height as f32 * 0.5]),
            scale: Animated::new([1.0, 1.0]),
            rotation: Animated::new(0.0),
        }
    }

    /// Source-pixel → canvas-pixel transform at time `t` for a source of `size` pixels.
    pub fn matrix(&self, t: f64, size: Vec2) -> Affine2 {
        let p = self.position.at(t);
        let s = self.scale.at(t);
        Affine2::from_translation(Vec2::from(p))
            * Affine2::from_angle(self.rotation.at(t).to_radians())
            * Affine2::from_scale(Vec2::from(s))
            * Affine2::from_translation(-size * 0.5)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub blend: BlendMode,
    pub opacity: Animated<f32>,
    pub transform: LayerTransform,
    pub content: LayerContent,
    /// Timeline placement (seconds). `length = None` lasts to the end of the timeline.
    pub start: f64,
    pub length: Option<f64>,
    #[serde(default)]
    pub grade: Grade,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<LayerMask>,
    #[serde(default)]
    pub fx: LayerFx,
    /// The layer has a baked normal map (AI estimate) in the project assets, used by relighting.
    #[serde(default)]
    pub has_normals: bool,
    /// The layer has an AI depth map in the project assets (depth of field).
    #[serde(default)]
    pub has_depth: bool,
    /// Bezier roto shapes masking this layer (combined with the painted / AI mask).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roto: Vec<crate::roto::RotoShape>,
    /// Set on an AI "remove object" patch: the layer whose object it paints out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch_of: Option<LayerId>,
    /// "Replace window / screen" settings, when used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glass: Option<crate::glass::Glass>,
}

impl Layer {
    pub fn new(name: impl Into<String>, content: LayerContent, canvas: &Canvas) -> Self {
        Self {
            id: LayerId::new(),
            name: name.into(),
            visible: true,
            locked: false,
            blend: BlendMode::Normal,
            opacity: Animated::new(1.0),
            transform: LayerTransform::centered(canvas),
            content,
            start: 0.0,
            length: None,
            grade: Grade::default(),
            mask: None,
            fx: LayerFx::default(),
            has_normals: false,
            has_depth: false,
            roto: Vec::new(),
            patch_of: None,
            glass: None,
        }
    }

    pub fn active_at(&self, t: f64) -> bool {
        t + 1e-9 >= self.start && self.length.is_none_or(|len| t < self.start + len)
    }

    /// Source time for media layers at timeline time `t`.
    pub fn media_time(&self, t: f64) -> f64 {
        match &self.content {
            LayerContent::Media { trim_in, .. } => (t - self.start).max(0.0) + trim_in,
            _ => 0.0,
        }
    }

    /// Every keyframe time on this layer (for the timeline).
    pub fn key_times(&self) -> Vec<f64> {
        let tr = &self.transform;
        let mut v: Vec<f64> = tr
            .position
            .key_times()
            .chain(tr.scale.key_times())
            .chain(tr.rotation.key_times())
            .chain(self.opacity.key_times())
            .chain(self.grade.key_times())
            .chain(self.fx.key_times())
            .collect();
        v.sort_by(f64::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < crate::anim::KEY_EPS);
        v
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    pub name: String,
    pub canvas: Canvas,
    pub media: Vec<MediaRef>,
    /// Bottom-most layer first.
    pub layers: Vec<Layer>,
    /// 3D camera solve of the main video (camera.json from photohop-ai), used by the Blender bridge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_solve: Option<std::path::PathBuf>,
    /// Body motion capture (body.json + .obj + .pc2 beside it), used by the Blender bridge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_motion: Option<std::path::PathBuf>,
}

pub const FORMAT_VERSION: u32 = 1;

impl Project {
    pub fn new(name: impl Into<String>, width: u32, height: u32, fps: f64, duration: f64) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            name: name.into(),
            canvas: Canvas { width, height, fps, duration, background: [0.0, 0.0, 0.0, 0.0] },
            media: Vec::new(),
            layers: Vec::new(),
            camera_solve: None,
            body_motion: None,
        }
    }

    pub fn media(&self, id: MediaId) -> Option<&MediaRef> {
        self.media.iter().find(|m| m.id == id)
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub fn layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    /// Pixel size of a layer's source rectangle.
    pub fn layer_source_size(&self, layer: &Layer) -> Vec2 {
        match &layer.content {
            LayerContent::Media { media, .. } => self
                .media(*media)
                .map(|m| Vec2::new(m.width as f32, m.height as f32))
                .unwrap_or(Vec2::ONE),
            LayerContent::Paint { width, height } | LayerContent::Solid { width, height, .. } => {
                Vec2::new(*width as f32, *height as f32)
            }
        }
    }

    pub fn layer_matrix(&self, layer: &Layer, t: f64) -> Affine2 {
        layer.transform.matrix(t, self.layer_source_size(layer))
    }

    /// Crop the canvas to `[x, y, w, h]` (canvas pixels): resizes it and shifts every layer
    /// (including all position keys) so content stays where it was.
    pub fn crop_canvas(&mut self, x: f32, y: f32, w: u32, h: u32) {
        self.canvas.width = w.max(1);
        self.canvas.height = h.max(1);
        for l in &mut self.layers {
            let p = &mut l.transform.position;
            p.base = [p.base[0] - x, p.base[1] - y];
            for k in &mut p.keys {
                k.value = [k.value[0] - x, k.value[1] - y];
            }
        }
    }
}
