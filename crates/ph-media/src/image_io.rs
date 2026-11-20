//! Still images: decode to working format (linear premultiplied f16), encode exports.

use crate::color;
use anyhow::{Context, Result};
use half::f16;
use image::{DynamicImage, ImageBuffer, Rgba};
use std::path::Path;
use std::sync::Arc;

/// An image in PhotoHop's working format: linear light, premultiplied RGBA, f16.
#[derive(Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<Vec<f16>>,
}

impl Frame {
    pub fn new(width: u32, height: u32, pixels: Vec<f16>) -> Self {
        debug_assert_eq!(pixels.len(), (width * height * 4) as usize);
        Self { width, height, pixels: Arc::new(pixels) }
    }

    pub fn byte_size(&self) -> usize {
        self.pixels.len() * 2
    }
}

pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "tif", "tiff", "exr"];

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.as_str()))
}

pub fn image_dimensions(path: &Path) -> Result<(u32, u32)> {
    image::image_dimensions(path).with_context(|| format!("reading size of {}", path.display()))
}

pub fn load_image(path: &Path) -> Result<Frame> {
    let img = image::open(path).with_context(|| format!("decoding {}", path.display()))?;
    let (w, h) = (img.width(), img.height());
    let is_exr = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exr"));
    let pixels = match img {
        DynamicImage::ImageRgba32F(buf) => color::rgba32f_linear_to_premul(buf.as_raw()),
        DynamicImage::ImageRgb32F(_) => color::rgba32f_linear_to_premul(img.to_rgba32f().as_raw()),
        _ if is_exr => color::rgba32f_linear_to_premul(img.to_rgba32f().as_raw()),
        DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_) | DynamicImage::ImageLuma16(_)
        | DynamicImage::ImageLumaA16(_) => color::rgba16_srgb_to_linear_premul(img.to_rgba16().as_raw()),
        _ => color::rgba8_srgb_to_linear_premul(img.to_rgba8().as_raw()),
    };
    Ok(Frame::new(w, h, pixels))
}

/// Save a working-format frame. Format from extension: PNG (16-bit when `deep`), JPEG, TIFF, EXR.
pub fn save_frame(frame: &Frame, path: &Path, deep: bool) -> Result<()> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let (w, h) = (frame.width, frame.height);
    match ext.as_str() {
        "exr" => {
            let data: Vec<f32> = frame
                .pixels
                .as_chunks::<4>().0.iter()
                .flat_map(|p| {
                    let a = p[3].to_f32();
                    let inv = if a > 1e-6 { 1.0 / a } else { 0.0 };
                    [p[0].to_f32() * inv, p[1].to_f32() * inv, p[2].to_f32() * inv, a]
                })
                .collect();
            let buf: ImageBuffer<Rgba<f32>, _> = ImageBuffer::from_raw(w, h, data).context("exr buffer")?;
            DynamicImage::ImageRgba32F(buf).save(path)?;
        }
        "jpg" | "jpeg" => {
            let rgba = color::linear_premul_to_rgba8(&frame.pixels);
            let buf: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(w, h, rgba).context("jpeg buffer")?;
            DynamicImage::ImageRgba8(buf).to_rgb8().save(path)?;
        }
        _ if deep => {
            let rgba = color::linear_premul_to_rgba16(&frame.pixels);
            let buf: ImageBuffer<Rgba<u16>, _> = ImageBuffer::from_raw(w, h, rgba).context("png16 buffer")?;
            DynamicImage::ImageRgba16(buf).save(path)?;
        }
        _ => {
            let rgba = color::linear_premul_to_rgba8(&frame.pixels);
            let buf: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(w, h, rgba).context("png buffer")?;
            DynamicImage::ImageRgba8(buf).save(path)?;
        }
    }
    Ok(())
}
