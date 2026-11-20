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

/// How a video's code values map to light.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Transfer {
    /// SDR (BT.709 / sRGB-like display gamma).
    #[default]
    Sdr,
    /// HDR10 / HDR10+ (SMPTE ST 2084).
    Pq,
    /// Hybrid log-gamma (ARIB STD-B67 / BT.2100 HLG).
    Hlg,
}

/// PQ code value → nits.
fn pq_eotf(e: f32) -> f32 {
    let (m1, m2, c1, c2, c3) = (0.159_301_76, 78.843_75, 0.835_937_5, 18.851_563, 18.6875);
    let p = e.max(0.0).powf(1.0 / m2);
    10_000.0 * ((p - c1).max(0.0) / (c2 - c3 * p)).powf(1.0 / m1)
}

/// HLG code value → scene light (0..1).
fn hlg_inverse_oetf(e: f32) -> f32 {
    let (a, b, c) = (0.178_832_77, 0.284_668_9, 0.559_910_7);
    if e <= 0.5 { e * e / 3.0 } else { (((e - c) / a).exp() + b) / 12.0 }
}

/// Reference ("graphics") white in nits: HDR content is scaled so this lands on 1.0.
pub const HDR_REFERENCE_NITS: f32 = 203.0;

/// BT.2020 → BT.709 primaries (linear light).
const BT2020_TO_709: [[f32; 3]; 3] =
    [[1.660_491, -0.587_641_1, -0.072_849_86], [-0.124_550_5, 1.132_899_9, -0.008_349_42], [-0.018_150_76, -0.100_578_9, 1.118_729_7]];

/// 16-bit RGBA video frame → linear premultiplied f16 in the working space (BT.709 primaries,
/// reference white = 1.0; HDR highlights stay above 1).
pub fn rgba16_video_to_linear_premul(src: &[u16], transfer: Transfer, bt2020: bool) -> Vec<f16> {
    let hlg_white = hlg_inverse_oetf(0.75);
    let decode = |v: f32| -> f32 {
        match transfer {
            Transfer::Sdr => srgb_to_linear(v),
            Transfer::Pq => pq_eotf(v) / HDR_REFERENCE_NITS,
            Transfer::Hlg => hlg_inverse_oetf(v) / hlg_white,
        }
    };
    let mut out = Vec::with_capacity(src.len());
    for px in src.as_chunks::<4>().0 {
        let a = px[3] as f32 / 65535.0;
        let mut rgb = [0, 1, 2].map(|c| decode(px[c] as f32 / 65535.0));
        if bt2020 {
            let m = &BT2020_TO_709;
            rgb = [0, 1, 2].map(|r| m[r][0] * rgb[0] + m[r][1] * rgb[1] + m[r][2] * rgb[2]);
        }
        for v in rgb {
            out.push(f16::from_f32(v * a));
        }
        out.push(f16::from_f32(a));
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
    fn hdr_reference_white_is_one() {
        // PQ code for 203 nits ≈ 0.58; HLG 75% is reference white.
        let pq = (0.5806 * 65535.0) as u16;
        let v = rgba16_video_to_linear_premul(&[pq, pq, pq, 65535], Transfer::Pq, false);
        assert!((v[0].to_f32() - 1.0).abs() < 0.03, "pq {}", v[0].to_f32());
        let hlg = (0.75 * 65535.0) as u16;
        let v = rgba16_video_to_linear_premul(&[hlg, hlg, hlg, 65535], Transfer::Hlg, true);
        assert!((v[1].to_f32() - 1.0).abs() < 0.01, "hlg {}", v[1].to_f32());
        // Grey stays grey through the 2020→709 matrix.
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
