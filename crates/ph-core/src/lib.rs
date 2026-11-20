//! PhotoHop document model.
//!
//! - [`anim`]: keyframed parameters
//! - [`blend`]: blend modes (GIMP-compatible formulas)
//! - [`doc`]: canvas, media references, layers
//! - [`grade`]: per-layer colour grade and masks
//! - [`history`]: snapshot undo/redo
//! - [`project_file`]: `.phop` save/load

pub mod anim;
pub mod blend;
pub mod doc;
pub mod fx;
pub mod grade;
pub mod history;
pub mod project_file;

pub use anim::{Animated, Interp};
pub use blend::BlendMode;
pub use doc::{Canvas, Layer, LayerContent, LayerId, LayerTransform, MediaId, MediaKind, MediaRef, Project};
pub use fx::{LayerFx, Relight, Shadow};
pub use grade::{Curves, Grade, GradeSample, LayerMask, Qualifier};
pub use history::History;
