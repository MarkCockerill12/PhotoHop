//! PhotoHop GPU compositor (wgpu compute, scene-linear premultiplied rgba16float).

pub mod compositor;
mod pipelines;

pub use compositor::{Compositor, DISPLAY_FORMAT, FxSample, LayerDraw, ScopeImage, ShadowDraw, WORK_FORMAT};
