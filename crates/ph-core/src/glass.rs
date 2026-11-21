//! "Replace window / screen": show another clip through glass in a layer (a window, a TV, a phone
//! screen) while keeping the glass's reflections and anything in front of it. One setting on the
//! layer; the renderer expands it into: the view clip, a Screen-blended reflection of the layer
//! limited to the glass, then the layer itself with the glass cut out.

use crate::anim::Animated;
use crate::doc::{LayerTransform, MediaId};
use crate::roto::RotoShape;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Glass {
    pub enabled: bool,
    /// The clip or image seen through the glass.
    pub view: Option<MediaId>,
    /// Where the view sits on the canvas (its centre, scale, rotation).
    pub view_transform: LayerTransform,
    /// Play the view on a loop (wallpaper loops are usually shorter than the shot).
    pub loop_view: bool,
    /// Glass panes in the layer's pixel space: Add shapes are glass, Subtract shapes keep things
    /// in front of the glass.
    pub panes: Vec<RotoShape>,
    /// AI-tracked mask videos of objects in front of the glass (kept from the original layer).
    pub front: Vec<MediaId>,
    /// How much of the original glass (reflections, glare) stays on top of the view (0..1).
    pub reflection: Animated<f32>,
    /// Colour of the glass, multiplied into the view (linear RGB).
    pub tint: [f32; 3],
    /// Brightness of the view (gain).
    pub brightness: Animated<f32>,
    /// The camera was focused on the room, not the view: lens blur in canvas pixels.
    pub defocus: Animated<f32>,
    /// Dirt / distance haze over the view (0..1), towards the glass tint.
    pub haze: Animated<f32>,
    /// Sensor noise on the view, to match the shot (perceptual units).
    pub grain: Animated<f32>,
    /// Glow around bright lights in the view.
    pub bloom: Animated<f32>,
}

impl Default for Glass {
    fn default() -> Self {
        Self {
            enabled: true,
            view: None,
            view_transform: LayerTransform {
                position: Animated::new([0.0, 0.0]),
                scale: Animated::new([1.0, 1.0]),
                rotation: Animated::new(0.0),
            },
            loop_view: true,
            panes: Vec::new(),
            front: Vec::new(),
            reflection: Animated::new(0.6),
            tint: [1.0, 1.0, 1.0],
            brightness: Animated::new(1.0),
            defocus: Animated::new(0.0),
            haze: Animated::new(0.0),
            grain: Animated::new(0.0),
            bloom: Animated::new(0.0),
        }
    }
}

impl Glass {
    pub fn has_glass(&self) -> bool {
        self.panes.iter().any(|p| p.active() && p.mode == crate::roto::RotoMode::Add)
    }

    pub fn is_active(&self) -> bool {
        self.enabled && self.view.is_some() && self.has_glass()
    }
}
