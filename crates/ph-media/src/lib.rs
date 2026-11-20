//! PhotoHop media I/O.
//!
//! Working format everywhere: scene-linear Rec.709, premultiplied RGBA, `f16`.
//! - [`color`]: transfer functions and conversions
//! - [`image_io`]: stills in/out
//! - [`video`]: video decode/encode through the ffmpeg executables

pub mod color;
pub mod image_io;
pub mod process;
pub mod video;

pub use image_io::{Frame, load_image, save_frame};
pub use video::{VideoCodec, VideoInfo, VideoReader, VideoWriter};
