//! Blend modes. Formulas follow GIMP's non-legacy layer modes
//! (see AiContext research/GIMP_COMPOSITING.md §1.5); the GPU implementation lives in ph-gpu.

use serde::{Deserialize, Serialize};

/// Colour space a blend formula is evaluated in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendSpace {
    /// Scene-linear light.
    Linear,
    /// sRGB transfer curve on the working primaries (GIMP "perceptual").
    Perceptual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    SoftLight,
    HardLight,
    Add,
    Subtract,
    Difference,
    Darken,
    Lighten,
    Dodge,
    Burn,
    LinearLight,
    GrainExtract,
    GrainMerge,
    Divide,
    Exclusion,
    Luminosity,
}

impl BlendMode {
    pub const ALL: [BlendMode; 19] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::SoftLight,
        Self::HardLight,
        Self::Add,
        Self::Subtract,
        Self::Difference,
        Self::Darken,
        Self::Lighten,
        Self::Dodge,
        Self::Burn,
        Self::LinearLight,
        Self::GrainExtract,
        Self::GrainMerge,
        Self::Divide,
        Self::Exclusion,
        Self::Luminosity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Overlay => "Overlay",
            Self::SoftLight => "Soft light",
            Self::HardLight => "Hard light",
            Self::Add => "Addition",
            Self::Subtract => "Subtract",
            Self::Difference => "Difference",
            Self::Darken => "Darken only",
            Self::Lighten => "Lighten only",
            Self::Dodge => "Dodge",
            Self::Burn => "Burn",
            Self::LinearLight => "Linear light",
            Self::GrainExtract => "Grain extract",
            Self::GrainMerge => "Grain merge",
            Self::Divide => "Divide",
            Self::Exclusion => "Exclusion",
            Self::Luminosity => "Luminance",
        }
    }

    /// Stable id passed to the shader. Must match `blend()` in ph-gpu's composite.wgsl.
    pub fn shader_id(self) -> u32 {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0) as u32
    }

    /// Space GIMP evaluates this mode in.
    pub fn space(self) -> BlendSpace {
        match self {
            Self::Normal
            | Self::Multiply
            | Self::Add
            | Self::Subtract
            | Self::Darken
            | Self::Lighten
            | Self::Divide
            | Self::Luminosity => BlendSpace::Linear,
            _ => BlendSpace::Perceptual,
        }
    }
}
