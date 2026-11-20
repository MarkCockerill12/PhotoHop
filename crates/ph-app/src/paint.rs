//! Paint-layer rasters and the brush. Brush falloff follows GIMP's generated brushes:
//! `gauss(pow(d, 0.4 / (1 - hardness)))` (AiContext research/GIMP_PAINT_SELECT_TRANSFORM.md §A).

use half::f16;
use ph_media::Frame;
use std::path::Path;

#[derive(Clone, Copy, PartialEq)]
pub struct BrushSettings {
    /// Diameter in layer pixels.
    pub size: f32,
    pub hardness: f32,
    pub opacity: f32,
    /// Spacing as a fraction of the diameter.
    pub spacing: f32,
    /// Straight sRGB colour picked in the UI.
    pub color: egui::Color32,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self { size: 40.0, hardness: 0.6, opacity: 1.0, spacing: 0.1, color: egui::Color32::WHITE }
    }
}

/// A rectangular block of pixels saved before a stroke, for undo.
pub struct Patch {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<[f32; 4]>,
}

pub struct PaintRaster {
    pub width: u32,
    pub height: u32,
    /// Linear premultiplied RGBA.
    pub data: Vec<[f32; 4]>,
    /// Bumped on every change (drives GPU re-upload).
    pub revision: u64,
    /// Pixels at stroke start (whole raster) and the stroke's dirty bounds.
    stroke_base: Option<Vec<[f32; 4]>>,
    stroke_cov: Vec<f32>,
    dirty: Option<[u32; 4]>,
    last_stamp: Option<(f32, f32)>,
    carry: f32,
}

fn gauss(f: f32) -> f32 {
    if f < 0.5 { 1.0 - 2.0 * f * f } else { 2.0 * (1.0 - f) * (1.0 - f) }
}

impl PaintRaster {
    pub fn new(width: u32, height: u32) -> Self {
        let n = (width * height) as usize;
        Self {
            width,
            height,
            data: vec![[0.0; 4]; n],
            revision: 1,
            stroke_base: None,
            stroke_cov: Vec::new(),
            dirty: None,
            last_stamp: None,
            carry: 0.0,
        }
    }

    pub fn filled(width: u32, height: u32, value: [f32; 4]) -> Self {
        let mut r = Self::new(width, height);
        r.data.fill(value);
        r
    }

    pub fn duplicate(&self) -> Self {
        let mut r = Self::new(self.width, self.height);
        r.data.clone_from(&self.data);
        r
    }

    pub fn to_frame(&self) -> Frame {
        let px: Vec<f16> = self.data.iter().flatten().map(|v| f16::from_f32(*v)).collect();
        Frame::new(self.width, self.height, px)
    }

    pub fn from_frame(f: &Frame) -> Self {
        let mut r = Self::new(f.width, f.height);
        for (dst, src) in r.data.iter_mut().zip(f.pixels.as_chunks::<4>().0) {
            *dst = src.map(f16::to_f32);
        }
        r
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Ok(Self::from_frame(&ph_media::load_image(path)?))
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        ph_media::save_frame(&self.to_frame(), path, true)
    }

    pub fn begin_stroke(&mut self) {
        self.stroke_base = Some(self.data.clone());
        self.stroke_cov = vec![0.0; self.data.len()];
        self.dirty = None;
        self.last_stamp = None;
        self.carry = 0.0;
    }

    /// Finish a stroke, returning the pixels it replaced (for undo).
    pub fn end_stroke(&mut self) -> Option<Patch> {
        let base = self.stroke_base.take()?;
        self.stroke_cov = Vec::new();
        let [x0, y0, x1, y1] = self.dirty.take()?;
        let (w, h) = (x1 - x0, y1 - y0);
        let mut pixels = Vec::with_capacity((w * h) as usize);
        for y in y0..y1 {
            let row = (y * self.width) as usize;
            pixels.extend_from_slice(&base[row + x0 as usize..row + x1 as usize]);
        }
        Some(Patch { x: x0, y: y0, w, h, pixels })
    }

    /// Swap a patch in, returning what was there (so the same call serves undo and redo).
    pub fn swap_patch(&mut self, p: Patch) -> Patch {
        let mut old = Vec::with_capacity(p.pixels.len());
        for row in 0..p.h {
            let start = ((p.y + row) * self.width + p.x) as usize;
            let src = &p.pixels[(row * p.w) as usize..((row + 1) * p.w) as usize];
            old.extend_from_slice(&self.data[start..start + p.w as usize]);
            self.data[start..start + p.w as usize].copy_from_slice(src);
        }
        self.revision += 1;
        Patch { pixels: old, ..p }
    }

    /// Continue the stroke to layer-pixel position `(x, y)`, stamping along the way.
    pub fn stroke_to(&mut self, x: f32, y: f32, brush: &BrushSettings, erase: bool) {
        let step = (brush.size * brush.spacing).max(0.5);
        match self.last_stamp {
            None => {
                self.stamp(x, y, brush, erase);
                self.last_stamp = Some((x, y));
            }
            Some((lx, ly)) => {
                let (dx, dy) = (x - lx, y - ly);
                let dist = (dx * dx + dy * dy).sqrt();
                let mut t = step - self.carry;
                while t <= dist {
                    let f = t / dist.max(1e-6);
                    self.stamp(lx + dx * f, ly + dy * f, brush, erase);
                    t += step;
                }
                self.carry = (dist - (t - step)).max(0.0);
                if dist > 0.0 {
                    self.last_stamp = Some((x, y));
                }
            }
        }
        self.revision += 1;
    }

    /// One dab. Coverage accumulates GIMP-style ("constant" mode): overlapping dabs never exceed
    /// the brush opacity within a stroke, and the stroke composites over the pre-stroke pixels.
    fn stamp(&mut self, cx: f32, cy: f32, brush: &BrushSettings, erase: bool) {
        let Some(base) = self.stroke_base.as_ref() else { return };
        let r = (brush.size * 0.5).max(0.5);
        let exponent = if brush.hardness >= 0.999 { 1e6 } else { 0.4 / (1.0 - brush.hardness) };
        let x0 = (cx - r - 1.0).floor().max(0.0) as u32;
        let y0 = (cy - r - 1.0).floor().max(0.0) as u32;
        let x1 = ((cx + r + 1.0).ceil() as u32).min(self.width);
        let y1 = ((cy + r + 1.0).ceil() as u32).min(self.height);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let rgba = egui::Rgba::from(brush.color);
        let col = [rgba.r(), rgba.g(), rgba.b()];
        let op = brush.opacity.clamp(0.0, 1.0);
        for y in y0..y1 {
            for x in x0..x1 {
                let d = (((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt() / r).min(1.0);
                let m = if d >= 1.0 { 0.0 } else { gauss(d.powf(exponent)) };
                if m <= 0.0 {
                    continue;
                }
                let i = (y * self.width + x) as usize;
                let c = &mut self.stroke_cov[i];
                if op > *c {
                    *c += (op - *c) * m * op;
                }
                let a = *c;
                let b = base[i];
                self.data[i] = if erase {
                    b.map(|v| v * (1.0 - a))
                } else {
                    [
                        col[0] * a + b[0] * (1.0 - a),
                        col[1] * a + b[1] * (1.0 - a),
                        col[2] * a + b[2] * (1.0 - a),
                        a + b[3] * (1.0 - a),
                    ]
                };
            }
        }
        let d = self.dirty.get_or_insert([x0, y0, x1, y1]);
        *d = [d[0].min(x0), d[1].min(y0), d[2].max(x1), d[3].max(y1)];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stroke_and_undo_patch() {
        let mut r = PaintRaster::new(64, 64);
        let brush = BrushSettings { color: egui::Color32::RED, ..Default::default() };
        r.begin_stroke();
        r.stroke_to(10.0, 10.0, &brush, false);
        r.stroke_to(50.0, 10.0, &brush, false);
        let patch = r.end_stroke().expect("dirty");
        assert!(r.data[(10 * 64 + 30) as usize][3] > 0.9);
        let redo = r.swap_patch(patch);
        assert_eq!(r.data[(10 * 64 + 30) as usize][3], 0.0);
        r.swap_patch(redo);
        assert!(r.data[(10 * 64 + 30) as usize][3] > 0.9);
    }
}
