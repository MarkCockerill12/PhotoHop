//! Per-layer integration effects: light wrap, blur/sharpen, grain, cast/contact shadow and
//! normal-based relighting. Implemented on the GPU in ph-gpu (composite.wgsl).

use crate::anim::Animated;
use glam::{Affine2, Mat2, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shadow {
    pub enabled: bool,
    /// Direction the shadow falls, degrees on screen (0 = right, 90 = towards the viewer/down).
    pub angle: Animated<f32>,
    /// Shadow length relative to the subject's height.
    pub length: Animated<f32>,
    /// Blur radius in canvas pixels.
    pub softness: Animated<f32>,
    pub opacity: Animated<f32>,
    /// Linear RGB tint of the shadow (multiplied into the background).
    pub color: [f32; 3],
    /// Strength of the short, dark contact shadow right under the subject.
    pub contact: Animated<f32>,
    /// Where the ground touches the subject, as a fraction of the layer height from the top (1 = bottom edge).
    pub ground: f32,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            enabled: false,
            angle: Animated::new(60.0),
            length: Animated::new(0.6),
            softness: Animated::new(12.0),
            opacity: Animated::new(0.6),
            color: [0.05, 0.05, 0.08],
            contact: Animated::new(0.7),
            ground: 1.0,
        }
    }
}

impl Shadow {
    /// Canvas → canvas map that throws the subject's silhouette onto the ground plane.
    /// `layer` maps source → canvas; the anchor is the ground point under the layer centre.
    pub fn cast_matrix(&self, t: f64, layer: Affine2, size: Vec2, length_scale: f32) -> Affine2 {
        let anchor = layer.transform_point2(Vec2::new(size.x * 0.5, size.y * self.ground));
        let a = self.angle.at(t).to_radians();
        let len = self.length.at(t) * length_scale;
        // v = point - anchor (y down). Height above ground h = -v.y.
        // shadow(v) = (v.x + h·len·cos a, h·len·sin a), sin a clamped so the map stays invertible.
        let s = a.sin();
        let s = if s.abs() < 0.05 { 0.05f32.copysign(if s == 0.0 { 1.0 } else { s }) } else { s };
        let m = Mat2::from_cols(Vec2::new(1.0, 0.0), Vec2::new(-len * a.cos(), -len * s));
        Affine2::from_translation(anchor) * Affine2::from_mat2(m) * Affine2::from_translation(-anchor)
    }

    /// Short, nearly flat contact shadow right under the subject.
    pub fn contact_matrix(&self, layer: Affine2, size: Vec2) -> Affine2 {
        let anchor = layer.transform_point2(Vec2::new(size.x * 0.5, size.y * self.ground));
        let m = Mat2::from_cols(Vec2::new(1.0, 0.0), Vec2::new(0.0, -0.06));
        Affine2::from_translation(anchor) * Affine2::from_mat2(m) * Affine2::from_translation(-anchor)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Relight {
    pub enabled: bool,
    /// Light direction: azimuth (degrees, 0 = from the right, 90 = from above) and elevation
    /// (degrees above the image plane towards the viewer).
    pub azimuth: Animated<f32>,
    pub elevation: Animated<f32>,
    pub intensity: Animated<f32>,
    /// Linear RGB light colour.
    pub color: [f32; 3],
    /// Light reaching surfaces facing away (0 = black shadows, 1 = no shading change).
    pub ambient: Animated<f32>,
    /// Rim / back light strength and colour.
    pub rim: Animated<f32>,
    pub rim_color: [f32; 3],
    /// Wrap lighting (soft, skin-like falloff; a cheap subsurface-scattering look).
    pub wrap: f32,
    /// Blend between original (0) and relit (1).
    pub amount: Animated<f32>,
}

impl Default for Relight {
    fn default() -> Self {
        Self {
            enabled: false,
            azimuth: Animated::new(45.0),
            elevation: Animated::new(35.0),
            intensity: Animated::new(1.0),
            color: [1.0, 0.95, 0.9],
            ambient: Animated::new(0.45),
            rim: Animated::new(0.0),
            rim_color: [1.0, 0.8, 0.6],
            wrap: 0.3,
            amount: Animated::new(1.0),
        }
    }
}

impl Relight {
    /// Unit light vector in normal-map space (x right, y up, z towards the viewer).
    pub fn direction(&self, t: f64) -> [f32; 3] {
        let (az, el) = (self.azimuth.at(t).to_radians(), self.elevation.at(t).to_radians());
        [el.cos() * az.cos(), el.cos() * az.sin(), el.sin()]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerFx {
    /// Background light bleeding over the layer's edges (0..1) and its width in canvas pixels.
    pub light_wrap: Animated<f32>,
    pub light_wrap_width: f32,
    /// Gaussian-like blur radius in canvas pixels.
    pub blur: Animated<f32>,
    /// Unsharp-mask amount (0..2).
    pub sharpen: Animated<f32>,
    /// Film grain strength (perceptual units) and size in canvas pixels.
    pub grain: Animated<f32>,
    pub grain_size: f32,
    pub grain_mono: bool,
    pub shadow: Shadow,
    pub relight: Relight,
}

impl Default for LayerFx {
    fn default() -> Self {
        Self {
            light_wrap: Animated::new(0.0),
            light_wrap_width: 24.0,
            blur: Animated::new(0.0),
            sharpen: Animated::new(0.0),
            grain: Animated::new(0.0),
            grain_size: 1.5,
            grain_mono: false,
            shadow: Shadow::default(),
            relight: Relight::default(),
        }
    }
}

impl LayerFx {
    pub fn is_identity(&self) -> bool {
        *self == LayerFx::default()
    }

    pub fn key_times(&self) -> impl Iterator<Item = f64> + '_ {
        let s = &self.shadow;
        let r = &self.relight;
        self.light_wrap
            .key_times()
            .chain(self.blur.key_times())
            .chain(self.sharpen.key_times())
            .chain(self.grain.key_times())
            .chain(s.angle.key_times())
            .chain(s.length.key_times())
            .chain(s.softness.key_times())
            .chain(s.opacity.key_times())
            .chain(s.contact.key_times())
            .chain(r.azimuth.key_times())
            .chain(r.elevation.key_times())
            .chain(r.intensity.key_times())
            .chain(r.ambient.key_times())
            .chain(r.rim.key_times())
            .chain(r.amount.key_times())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_keeps_feet_and_throws_head() {
        let s = Shadow { angle: Animated::new(90.0), length: Animated::new(0.5), ..Default::default() };
        let layer = Affine2::IDENTITY;
        let size = Vec2::new(100.0, 200.0);
        let m = s.cast_matrix(0.0, layer, size, 1.0);
        let feet = Vec2::new(50.0, 200.0);
        assert!((m.transform_point2(feet) - feet).length() < 1e-3, "anchor stays put");
        let head = m.transform_point2(Vec2::new(50.0, 0.0));
        // Shadow falls "down" (towards viewer) by half the height at 90°.
        assert!((head - Vec2::new(50.0, 300.0)).length() < 0.5, "{head:?}");
        assert!(m.matrix2.determinant().abs() > 1e-3);
    }

    #[test]
    fn light_direction_is_unit() {
        let d = Relight::default().direction(0.0);
        assert!(((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - 1.0).abs() < 1e-5);
    }
}
