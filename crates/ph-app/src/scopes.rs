//! Video scopes computed on the CPU from a small sRGB copy of the composite
//! (see `Compositor::request_scope`): waveform, RGB parade, vectorscope, histogram.

use ph_gpu::ScopeImage;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Waveform,
    Parade,
    Vectorscope,
    Histogram,
}

impl ScopeKind {
    pub const ALL: [ScopeKind; 4] = [Self::Waveform, Self::Parade, Self::Vectorscope, Self::Histogram];

    pub fn label(self) -> &'static str {
        match self {
            Self::Waveform => "Waveform",
            Self::Parade => "Parade",
            Self::Vectorscope => "Vectorscope",
            Self::Histogram => "Histogram",
        }
    }
}

pub struct Scopes {
    pub show: bool,
    pub kind: ScopeKind,
    pub texture: Option<egui::TextureHandle>,
    last: Option<ScopeImage>,
    dirty: bool,
}

impl Default for Scopes {
    fn default() -> Self {
        Self { show: false, kind: ScopeKind::Waveform, texture: None, last: None, dirty: false }
    }
}

const H: usize = 256;

/// Rec.709 luma on encoded values.
fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Map hit counts to brightness with a soft log curve so sparse traces stay visible.
fn level(count: u32, scale: f32) -> u8 {
    if count == 0 { 0 } else { (255.0 * (1.0 - (-(count as f32) * scale).exp())).clamp(30.0, 255.0) as u8 }
}

fn waveform(img: &ScopeImage, channel: Option<usize>, out_w: usize) -> Vec<u32> {
    let (w, h) = (img.width as usize, img.height as usize);
    let mut bins = vec![0u32; out_w * H];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            if img.rgba[i + 3] < 8 {
                continue;
            }
            let px = &img.rgba[i..i + 3];
            let v = match channel {
                Some(c) => px[c] as f32,
                None => luma(px[0] as f32, px[1] as f32, px[2] as f32),
            };
            let col = x * out_w / w;
            let row = H - 1 - (v as usize).min(H - 1);
            bins[row * out_w + col] += 1;
        }
    }
    bins
}

fn render(img: &ScopeImage, kind: ScopeKind) -> egui::ColorImage {
    let (w, h) = (img.width as usize, img.height as usize);
    // Expected hits per waveform bin ≈ h / H; one hit should already be clearly visible.
    let scale = 0.4 * H as f32 / (h as f32).max(1.0);
    match kind {
        ScopeKind::Waveform => {
            let bins = waveform(img, None, w);
            let mut out = egui::ColorImage::filled([w, H], egui::Color32::BLACK);
            for (p, c) in out.pixels.iter_mut().zip(&bins) {
                let l = level(*c, scale);
                *p = egui::Color32::from_rgb(l / 3, l, l / 2);
            }
            out
        }
        ScopeKind::Parade => {
            let third = w.div_ceil(3).max(1);
            let mut out = egui::ColorImage::filled([third * 3, H], egui::Color32::BLACK);
            for c in 0..3 {
                let bins = waveform(img, Some(c), third);
                for row in 0..H {
                    for col in 0..third {
                        let l = level(bins[row * third + col], scale * 3.0);
                        let mut rgb = [l / 5; 3];
                        rgb[c] = l;
                        out.pixels[row * third * 3 + c * third + col] = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                    }
                }
            }
            out
        }
        ScopeKind::Vectorscope => {
            let n = 256usize;
            let mut acc = vec![[0u32; 4]; n * n];
            for i in (0..w * h).map(|p| p * 4) {
                if img.rgba[i + 3] < 8 {
                    continue;
                }
                let [r, g, b] = [0, 1, 2].map(|c| img.rgba[i + c] as f32 / 255.0);
                let y = luma(r, g, b);
                let (cb, cr) = ((b - y) / 1.8556, (r - y) / 1.5748);
                let x = ((cb + 0.5) * (n - 1) as f32) as usize;
                let yy = ((0.5 - cr) * (n - 1) as f32) as usize;
                let a = &mut acc[yy.min(n - 1) * n + x.min(n - 1)];
                a[0] += img.rgba[i] as u32;
                a[1] += img.rgba[i + 1] as u32;
                a[2] += img.rgba[i + 2] as u32;
                a[3] += 1;
            }
            let mut out = egui::ColorImage::filled([n, n], egui::Color32::BLACK);
            for (p, a) in out.pixels.iter_mut().zip(&acc) {
                if a[3] > 0 {
                    let l = level(a[3], scale * 2.0) as u32;
                    let c = |v: u32| ((v / a[3]).max(60) * l / 255) as u8;
                    *p = egui::Color32::from_rgb(c(a[0]), c(a[1]), c(a[2]));
                }
            }
            out
        }
        ScopeKind::Histogram => {
            let mut hist = [[0u32; 256]; 4];
            for i in (0..w * h).map(|p| p * 4) {
                if img.rgba[i + 3] < 8 {
                    continue;
                }
                let px = &img.rgba[i..i + 3];
                for c in 0..3 {
                    hist[c][px[c] as usize] += 1;
                }
                hist[3][luma(px[0] as f32, px[1] as f32, px[2] as f32) as usize] += 1;
            }
            let peak = hist.iter().flat_map(|h| h[1..255].iter()).copied().max().unwrap_or(1).max(1) as f32;
            let mut out = egui::ColorImage::filled([256, H], egui::Color32::BLACK);
            for x in 0..256 {
                let heights = hist.map(|h| ((h[x] as f32 / peak).min(1.0) * (H - 1) as f32) as usize);
                for y in 0..H {
                    let from_bottom = H - 1 - y;
                    let on = heights.map(|hh| from_bottom < hh);
                    let mut rgb = [0u8; 3];
                    for c in 0..3 {
                        if on[c] {
                            rgb[c] = 200;
                        }
                    }
                    if on[3] && rgb == [0; 3] {
                        rgb = [90; 3];
                    }
                    out.pixels[y * 256 + x] = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                }
            }
            out
        }
    }
}

impl Scopes {
    pub fn set_image(&mut self, img: ScopeImage) {
        self.last = Some(img);
        self.dirty = true;
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        if !self.dirty {
            return;
        }
        let Some(img) = &self.last else { return };
        let image = render(img, self.kind);
        match &mut self.texture {
            Some(t) => t.set(image, egui::TextureOptions::LINEAR),
            None => self.texture = Some(ctx.load_texture("scope", image, egui::TextureOptions::LINEAR)),
        }
        self.dirty = false;
    }

    pub fn window(&mut self, ctx: &egui::Context) {
        if !self.show {
            return;
        }
        self.refresh(ctx);
        let mut open = self.show;
        egui::Window::new("Scopes").open(&mut open).default_size([420.0, 300.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for k in ScopeKind::ALL {
                    if ui.selectable_label(self.kind == k, k.label()).clicked() {
                        self.kind = k;
                        self.dirty = true;
                    }
                }
            });
            let avail = ui.available_size();
            let aspect = if self.kind == ScopeKind::Vectorscope { 1.0 } else { 1.6 };
            let w = avail.x.min(avail.y * aspect).max(120.0);
            let size = egui::vec2(w, w / aspect);
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 0.0, egui::Color32::BLACK);
            if let Some(t) = &self.texture {
                p.image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
            draw_graticule(&p, rect, self.kind);
        });
        self.show = open;
    }
}

fn draw_graticule(p: &egui::Painter, rect: egui::Rect, kind: ScopeKind) {
    let line = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(40));
    let text = egui::Color32::from_white_alpha(110);
    match kind {
        ScopeKind::Waveform | ScopeKind::Parade => {
            for pct in [0, 25, 50, 75, 100] {
                let y = rect.bottom() - rect.height() * pct as f32 / 100.0;
                p.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], line);
                p.text(egui::pos2(rect.left() + 2.0, y - 1.0), egui::Align2::LEFT_BOTTOM, format!("{pct}"), egui::FontId::monospace(9.0), text);
            }
            if kind == ScopeKind::Parade {
                for i in 1..3 {
                    let x = rect.left() + rect.width() * i as f32 / 3.0;
                    p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], line);
                }
            }
        }
        ScopeKind::Vectorscope => {
            let c = rect.center();
            let r = rect.width() * 0.5;
            p.circle_stroke(c, r * 0.98, line);
            p.line_segment([egui::pos2(rect.left(), c.y), egui::pos2(rect.right(), c.y)], line);
            p.line_segment([egui::pos2(c.x, rect.top()), egui::pos2(c.x, rect.bottom())], line);
            // 75% colour targets.
            for (name, rgb) in [
                ("R", [0.75, 0.0, 0.0]),
                ("Y", [0.75, 0.75, 0.0]),
                ("G", [0.0, 0.75, 0.0]),
                ("C", [0.0, 0.75, 0.75]),
                ("B", [0.0, 0.0, 0.75]),
                ("M", [0.75, 0.0, 0.75]),
            ] {
                let y = luma(rgb[0], rgb[1], rgb[2]);
                let (cb, cr) = ((rgb[2] - y) / 1.8556, (rgb[0] - y) / 1.5748);
                let pt = egui::pos2(c.x + cb * 2.0 * r, c.y - cr * 2.0 * r);
                p.rect_stroke(egui::Rect::from_center_size(pt, egui::vec2(8.0, 8.0)), 0.0, line, egui::StrokeKind::Middle);
                p.text(pt + egui::vec2(6.0, -6.0), egui::Align2::LEFT_BOTTOM, name, egui::FontId::monospace(9.0), text);
            }
            // Skin-tone line (≈123° from +Cb): faces sit along it under neutral light.
            let a = 123f32.to_radians();
            let end = egui::pos2(c.x + a.cos() * r * 0.95, c.y - a.sin() * r * 0.95);
            p.line_segment([c, end], egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 200, 150, 90)));
            p.text(end, egui::Align2::RIGHT_BOTTOM, "skin", egui::FontId::monospace(9.0), text);
        }
        ScopeKind::Histogram => {
            for q in 1..4 {
                let x = rect.left() + rect.width() * q as f32 / 4.0;
                p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_bins_luma() {
        let img = ScopeImage { width: 2, height: 1, rgba: vec![255, 255, 255, 255, 0, 0, 0, 255] };
        let bins = waveform(&img, None, 2);
        assert_eq!(bins[0], 1, "white lands on the top row");
        assert_eq!(bins[(H - 1) * 2 + 1], 1, "black lands on the bottom row");
    }
}
