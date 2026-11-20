//! Transfer functions and pixel-format conversion. PhotoHop works internally in
//! scene-linear Rec.709 primaries, premultiplied alpha, half floats.

use half::f16;
use std::sync::OnceLock;

#[inline]
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

#[inline]
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn lut8() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| std::array::from_fn(|i| srgb_to_linear(i as f32 / 255.0)))
}

fn lut16() -> &'static [f32] {
    static LUT: OnceLock<Vec<f32>> = OnceLock::new();
    LUT.get_or_init(|| (0..65536).map(|i| srgb_to_linear(i as f32 / 65535.0)).collect())
}

/// sRGB-encoded straight RGBA8 → linear premultiplied RGBA f16.
pub fn rgba8_srgb_to_linear_premul(src: &[u8]) -> Vec<f16> {
    let lut = lut8();
    let mut out = Vec::with_capacity(src.len());
    for px in src.as_chunks::<4>().0 {
        let a = px[3] as f32 / 255.0;
        for &c in &px[..3] {
            out.push(f16::from_f32(lut[c as usize] * a));
        }
        out.push(f16::from_f32(a));
    }
    out
}

/// sRGB-encoded straight RGBA16 → linear premultiplied RGBA f16.
pub fn rgba16_srgb_to_linear_premul(src: &[u16]) -> Vec<f16> {
    let lut = lut16();
    let mut out = Vec::with_capacity(src.len());
    for px in src.as_chunks::<4>().0 {
        let a = px[3] as f32 / 65535.0;
        for &c in &px[..3] {
            out.push(f16::from_f32(lut[c as usize] * a));
        }
        out.push(f16::from_f32(a));
    }
    out
}

/// Linear straight RGBA f32 (e.g. EXR) → linear premultiplied RGBA f16.
pub fn rgba32f_linear_to_premul(src: &[f32]) -> Vec<f16> {
    let mut out = Vec::with_capacity(src.len());
    for px in src.as_chunks::<4>().0 {
        let a = px[3].clamp(0.0, 1.0);
        for &c in &px[..3] {
            out.push(f16::from_f32(c * a));
        }
        out.push(f16::from_f32(a));
    }
    out
}

/// Linear premultiplied RGBA f16 → sRGB straight RGBA8 (for PNG/JPEG export and thumbnails).
pub fn linear_premul_to_rgba8(src: &[f16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len());
    for px in src.as_chunks::<4>().0 {
        let a = px[3].to_f32().clamp(0.0, 1.0);
        let inv = if a > 1e-6 { 1.0 / a } else { 0.0 };
        for c in &px[..3] {
            let v = linear_to_srgb((c.to_f32() * inv).clamp(0.0, 1.0));
            out.push((v * 255.0 + 0.5) as u8);
        }
        out.push((a * 255.0 + 0.5) as u8);
    }
    out
}

/// Linear premultiplied RGBA f16 → sRGB straight RGBA16 (16-bit PNG export).
pub fn linear_premul_to_rgba16(src: &[f16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(src.len());
    for px in src.as_chunks::<4>().0 {
        let a = px[3].to_f32().clamp(0.0, 1.0);
        let inv = if a > 1e-6 { 1.0 / a } else { 0.0 };
        for c in &px[..3] {
            let v = linear_to_srgb((c.to_f32() * inv).clamp(0.0, 1.0));
            out.push((v * 65535.0 + 0.5) as u16);
        }
        out.push((a * 65535.0 + 0.5) as u16);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_roundtrip() {
        for i in 0..=255u8 {
            let v = i as f32 / 255.0;
            assert!((linear_to_srgb(srgb_to_linear(v)) - v).abs() < 1e-5);
        }
    }

    #[test]
    fn premul_roundtrip_8bit() {
        let src = [255u8, 128, 0, 255, 200, 100, 50, 128, 0, 0, 0, 0];
        let back = linear_premul_to_rgba8(&rgba8_srgb_to_linear_premul(&src));
        for (a, b) in src.iter().zip(&back) {
            assert!((*a as i32 - *b as i32).abs() <= 2, "{src:?} vs {back:?}");
        }
    }
}
