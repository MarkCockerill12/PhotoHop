//! "Fit into the scene" actions on the App: smart colour match, one-click fit (colour + shadow +
//! camera look), and splitting a picture with several objects into one layer per object.

use crate::app::App;
use crate::paint::PaintRaster;
use crate::scenematch::{Px, features_perceptual, fit_smart, sample, targets};
use anyhow::{Context, Result, bail};
use glam::Vec2;
use ph_core::doc::LayerContent;
use ph_core::look::{LookSliders, MatchMethod, grade_perceptual};
use ph_core::{Animated, LayerId, MediaId, MediaKind, MediaRef};

/// Mask value (0..1) of a layer at normalised source coordinates, honouring enable/invert.
fn mask_fn<'a>(raster: Option<&'a PaintRaster>, mask: Option<&ph_core::LayerMask>) -> impl Fn(f32, f32) -> f32 + 'a {
    let state = mask.filter(|m| m.enabled && m.video.is_none()).map(|m| m.invert);
    move |u, v| {
        let (Some(r), Some(inv)) = (raster, state) else { return 1.0 };
        let x = ((u * r.width as f32) as u32).min(r.width - 1);
        let y = ((v * r.height as f32) as u32).min(r.height - 1);
        let m = r.data[(y * r.width + x) as usize][0];
        if inv { 1.0 - m } else { m }
    }
}

impl App {
    /// Smart "Match to scene" on a layer: fit exposure, temperature, tint, contrast, saturation
    /// and shadow level to the scene around it (or to the chosen reference layer). Instant, no AI.
    pub fn match_to_scene(&mut self, id: LayerId) -> Result<String> {
        let idx = self.doc.layer_index(id).context("layer missing")?;
        let subject = self.composite_only(|l| l == id, Some(id))?;
        let (w, h) = (subject.width as usize, subject.height as usize);
        let alpha = |x: usize, y: usize| subject.pixels[(y * w + x) * 4 + 3].to_f32();
        // Bounding box of the visible subject on the canvas.
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
        for y in (0..h).step_by(2) {
            for x in (0..w).step_by(2) {
                if alpha(x, y) > 0.5 {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                }
            }
        }
        if x1 <= x0 || y1 <= y0 {
            bail!("nothing visible on this layer to match");
        }
        let scene_px: Vec<([f32; 3], f32)> = if let Some(r) = self.match_reference.filter(|r| *r != id && self.doc.layer(*r).is_some()) {
            let frame = self.layer_frame(r).context("the reference layer has no pixels yet")?;
            sample(&frame, 120_000, |_, _, a| a).into_iter().map(|p| (p.rgb.map(ph_core::look::to_perceptual), p.w)).collect()
        } else {
            if idx == 0 {
                bail!("put a background layer under this one first (or pick a reference layer)");
            }
            let below: Vec<LayerId> = self.doc.layers[..idx].iter().map(|l| l.id).collect();
            let bg = self.composite_only(|l| below.contains(&l), None)?;
            // The ring around the subject counts most; the whole scene a little (for its black
            // and white levels when the ring is all sky or all floor).
            let (gx, gy) = ((x1 - x0) / 2, (y1 - y0) / 2);
            let (bx0, by0, bx1, by1) = (x0.saturating_sub(gx), y0.saturating_sub(gy), x1 + gx, y1 + gy);
            sample(&bg, 160_000, |x, y, _| {
                if alpha(x, y) > 0.05 {
                    0.0
                } else if x >= bx0 && x <= bx1 && y >= by0 && y <= by1 {
                    1.0
                } else {
                    0.2
                }
            })
            .into_iter()
            .map(|p| (p.rgb.map(ph_core::look::to_perceptual), p.w))
            .collect()
        };
        let subject_px: Vec<Px> = sample(&subject, 60_000, |_, _, a| if a > 0.5 { a } else { 0.0 });
        if subject_px.is_empty() || scene_px.is_empty() {
            bail!("nothing to measure");
        }
        let (t, auto) = (self.time, self.auto_key);
        let layer = self.doc.layer(id).context("layer missing")?;
        let pivot = layer.grade.pivot.at(t);
        let start = layer.grade.scene_match.map_or_else(|| LookSliders::read(&layer.grade, t), |m| m.original);
        let fs = features_perceptual(&subject_px.iter().map(|p| (grade_perceptual(p.rgb, &start, pivot), p.w)).collect::<Vec<_>>());
        let ft = features_perceptual(&scene_px);
        let matched = fit_smart(&subject_px, &targets(&fs, &ft), start, pivot);
        self.edit("Match to scene", |d| {
            if let Some(l) = d.layer_mut(id) {
                l.grade.apply_scene_match(matched, MatchMethod::Smart, t, auto);
                l.grade.match_amount = Animated::new(0.0);
            }
        });
        Ok(format!("Matched to the scene: {}. Drag Match strength to compare with the original.", crate::scenematch::describe(&start, &matched)))
    }

    /// One click: colour match, a shadow that agrees with the others in the scene, and the
    /// camera's grain and softness.
    pub fn auto_fit(&mut self, id: LayerId) -> Result<String> {
        let keep = self.selected;
        self.selected = Some(id);
        let colour = self.match_to_scene(id);
        let shadow = self.auto_shadow(id);
        self.selected = keep;
        let camera = self.match_grain(id);
        let mut out = Vec::new();
        out.push(match colour {
            Ok(_) => "colour matched".to_owned(),
            Err(e) => format!("colour: {e:#}"),
        });
        out.push(match shadow {
            Ok(s) => s,
            Err(e) => format!("shadow: {e:#}"),
        });
        out.push(match camera {
            Ok(g) => format!("grain {g:.3}"),
            Err(e) => format!("camera: {e:#}"),
        });
        Ok(format!("Fitted into the scene: {}. Fine-tune any slider; Match strength blends the colour.", out.join("; ")))
    }

    /// Copy the scene's grain only (One-click fit never blurs: blur and motion blur are left to the
    /// user via Match the camera, as false motion from straight edges would smear the subject).
    pub fn match_grain(&mut self, id: LayerId) -> Result<f32> {
        let before = self.doc.layer(id).context("layer missing")?.fx.clone();
        let keep = self.selected;
        self.selected = Some(id);
        let r = self.match_detail();
        self.selected = keep;
        r?;
        let grain = self.doc.layer(id).map_or(0.0, |l| l.fx.grain.at(self.time));
        self.edit("Match grain", |d| {
            if let Some(l) = d.layer_mut(id) {
                l.fx.blur = before.blur.clone();
                l.fx.motion_blur = before.motion_blur.clone();
                l.fx.motion_angle = before.motion_angle;
            }
        });
        Ok(grain)
    }

    /// Turn on the cast shadow with the feet found. If another layer already has a shadow, copy
    /// its direction and look (one sun for the whole scene), with softness scaled to this
    /// layer's size; otherwise aim it from the scene's light.
    pub fn auto_shadow(&mut self, id: LayerId) -> Result<String> {
        let t = self.time;
        let ground = self.find_ground(id).context("this layer has no pixels yet")?;
        let layer = self.doc.layer(id).context("layer missing")?;
        let size = self.doc.layer_source_size(layer);
        let mut probe = layer.fx.shadow.clone();
        probe.ground = ground;
        let (_, tall) = probe.anchor_and_height(self.doc.layer_matrix(layer, t), size);
        let donor = self
            .doc
            .layers
            .iter()
            .filter(|l| l.id != id && l.visible && l.fx.shadow.enabled)
            .min_by_key(|l| self.doc.layer_index(l.id))
            .map(|l| {
                let (_, h) = l.fx.shadow.anchor_and_height(self.doc.layer_matrix(l, t), self.doc.layer_source_size(l));
                (l.name.clone(), l.fx.shadow.clone(), h)
            });
        let auto = self.auto_key;
        if let Some((name, s, their_tall)) = donor {
            let k = (tall / their_tall.max(1.0)).clamp(0.15, 4.0);
            self.edit("Auto shadow", |d| {
                if let Some(l) = d.layer_mut(id) {
                    let sh = &mut l.fx.shadow;
                    sh.enabled = true;
                    sh.ground = ground;
                    sh.angle.set(t, s.angle.at(t), auto);
                    sh.length.set(t, s.length.at(t), auto);
                    sh.thickness.set(t, s.thickness.at(t), auto);
                    sh.opacity.set(t, s.opacity.at(t), auto);
                    sh.contact.set(t, s.contact.at(t), auto);
                    sh.softness.set(t, (s.softness.at(t) * k).max(0.5), auto);
                    sh.color = s.color;
                }
            });
            return Ok(format!("shadow copied from \"{name}\" (same sun, softness ×{k:.2} for size)"));
        }
        let keep = self.selected;
        self.selected = Some(id);
        let r = self.match_light();
        self.selected = keep;
        r?;
        let k = (tall / 480.0).clamp(0.15, 4.0);
        self.edit("Auto shadow", |d| {
            if let Some(l) = d.layer_mut(id) {
                l.fx.relight.enabled = false;
                let sh = &mut l.fx.shadow;
                sh.enabled = true;
                sh.ground = ground;
                let soft = sh.softness.at(t) * k;
                sh.softness.set(t, soft.max(0.5), auto);
            }
        });
        Ok("shadow aimed from the scene's light".into())
    }

    /// Split a still layer whose picture holds several separate objects (three figures in one
    /// render) into one layer per object, so each gets its own shadow, feet and colour match.
    /// The original layer is kept, hidden. Returns the new layer ids, left to right.
    pub fn split_objects(&mut self, id: LayerId) -> Result<Vec<LayerId>> {
        let layer = self.doc.layer(id).context("layer missing")?.clone();
        let LayerContent::Media { media, .. } = &layer.content else {
            bail!("Split works on photo layers");
        };
        if self.doc.media(*media).is_some_and(MediaRef::is_video) {
            bail!("Split works on still photos, not video");
        }
        let tr = &layer.transform;
        if tr.position.is_animated() || tr.scale.is_animated() || tr.rotation.is_animated() {
            bail!("remove the layer's movement keyframes first");
        }
        let frame = self.layer_frame(id).context("this layer has no pixels yet")?;
        let (w, h) = (frame.width as usize, frame.height as usize);
        // Coverage = alpha × mask, at source resolution.
        let cov: Vec<f32> = {
            let mfn = mask_fn(self.masks.get(&id), layer.mask.as_ref());
            (0..w * h)
                .map(|i| {
                    let (x, y) = (i % w, i / w);
                    frame.pixels[i * 4 + 3].to_f32() * mfn((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32)
                })
                .collect()
        };
        let labels = label_objects(&cov, w, h);
        if labels.parts.len() < 2 {
            bail!("found only one object in this layer (objects must have a gap between them)");
        }
        let dir = crate::ai::results_dir();
        std::fs::create_dir_all(&dir)?;
        let matrix = self.doc.layer_matrix(&layer, self.time);
        let mut new_layers = Vec::new();
        let mut new_media = Vec::new();
        for (k, part) in labels.parts.iter().enumerate() {
            let pad = 6;
            let (bx0, by0) = (part.x0.saturating_sub(pad), part.y0.saturating_sub(pad));
            let (bx1, by1) = ((part.x1 + pad).min(w - 1), (part.y1 + pad).min(h - 1));
            let (bw, bh) = (bx1 - bx0 + 1, by1 - by0 + 1);
            let mut px = vec![half::f16::ZERO; bw * bh * 4];
            for y in 0..bh {
                for x in 0..bw {
                    let (sx, sy) = (bx0 + x, by0 + y);
                    let si = sy * w + sx;
                    if labels.label[si] != part.label {
                        continue;
                    }
                    let a = frame.pixels[si * 4 + 3].to_f32();
                    let c = cov[si];
                    if c <= 0.0 || a <= 0.0 {
                        continue;
                    }
                    // Premultiplied: scale colour and alpha by the mask share of the alpha.
                    let k = c / a;
                    for ch in 0..4 {
                        px[(y * bw + x) * 4 + ch] = half::f16::from_f32(frame.pixels[si * 4 + ch].to_f32() * k);
                    }
                }
            }
            let rgba = ph_media::color::linear_premul_to_rgba16(&px);
            let img = image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(bw as u32, bh as u32, rgba).context("part image")?;
            let mid = MediaId::new();
            let path = dir.join(format!("part-{}.png", mid.0));
            img.save(&path).context("saving the split part")?;
            new_media.push(MediaRef { id: mid, path, kind: MediaKind::Image, width: bw as u32, height: bh as u32 });
            let centre = Vec2::new(bx0 as f32 + bw as f32 * 0.5, by0 as f32 + bh as f32 * 0.5);
            let pos = matrix.transform_point2(centre);
            let mut l = layer.clone();
            l.id = LayerId::new();
            l.name = format!("{} {}", layer.name, k + 1);
            l.content = LayerContent::Media { media: mid, trim_in: 0.0 };
            l.transform.position = Animated::new([pos.x, pos.y]);
            l.mask = None;
            l.roto.clear();
            l.has_normals = false;
            l.has_depth = false;
            l.body_mesh = None;
            l.visible = true;
            new_layers.push(l);
        }
        let ids: Vec<LayerId> = new_layers.iter().map(|l| l.id).collect();
        let n = ids.len();
        self.edit("Split into objects", |d| {
            d.media.extend(new_media);
            let idx = d.layer_index(id).unwrap_or(d.layers.len().saturating_sub(1));
            if let Some(l) = d.layer_mut(id) {
                l.visible = false;
                if !l.name.ends_with("(original)") {
                    l.name = format!("{} (original)", l.name);
                }
            }
            for (k, l) in new_layers.into_iter().enumerate() {
                d.layers.insert(idx + 1 + k, l);
            }
        });
        // Each part's feet: its own ground line.
        for &pid in &ids {
            if let Some(g) = self.find_ground(pid) {
                self.edit("Find feet", |d| d.layer_mut(pid).map_or((), |l| l.fx.shadow.ground = g));
            }
        }
        self.selected = ids.first().copied();
        self.status = format!("Split into {n} layers (the original is hidden). Fit each one into the scene: select it and click One-click fit.");
        Ok(ids)
    }
}

pub struct Part {
    pub label: u32,
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
    pub area: usize,
}

pub struct Labels {
    /// Per pixel: 0 = background, else the part label.
    pub label: Vec<u32>,
    /// Kept parts, left to right.
    pub parts: Vec<Part>,
}

/// Connected objects in a coverage map. Pieces closer than ~0.6 % of the image (a gap in an arm,
/// a held weapon) are joined; specks much smaller than the biggest object are dropped.
pub fn label_objects(cov: &[f32], w: usize, h: usize) -> Labels {
    let solid: Vec<bool> = cov.iter().map(|c| *c > 0.1).collect();
    // Dilate (square, radius r) by two separable passes so nearby pieces connect.
    let r = ((w.max(h) as f32 * 0.006) as usize).max(2);
    let mut tmp = vec![false; w * h];
    for y in 0..h {
        let mut last: isize = -(r as isize) - 1;
        for x in 0..w {
            if solid[y * w + x] {
                last = x as isize;
            }
            tmp[y * w + x] = x as isize - last <= r as isize;
        }
        let mut next: isize = (w + r + 1) as isize;
        for x in (0..w).rev() {
            if solid[y * w + x] {
                next = x as isize;
            }
            tmp[y * w + x] |= next - x as isize <= r as isize;
        }
    }
    let mut grown = vec![false; w * h];
    for x in 0..w {
        let mut last: isize = -(r as isize) - 1;
        for y in 0..h {
            if tmp[y * w + x] {
                last = y as isize;
            }
            grown[y * w + x] = y as isize - last <= r as isize;
        }
        let mut next: isize = (h + r + 1) as isize;
        for y in (0..h).rev() {
            if tmp[y * w + x] {
                next = y as isize;
            }
            grown[y * w + x] |= next - y as isize <= r as isize;
        }
    }
    let mut label = vec![0u32; w * h];
    let mut parts = Vec::new();
    let mut next_label = 1u32;
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !grown[start] || label[start] != 0 {
            continue;
        }
        let lab = next_label;
        next_label += 1;
        let mut p = Part { label: lab, x0: w, y0: h, x1: 0, y1: 0, area: 0 };
        label[start] = lab;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            if solid[i] {
                p.area += 1;
                (p.x0, p.y0, p.x1, p.y1) = (p.x0.min(x), p.y0.min(y), p.x1.max(x), p.y1.max(y));
            }
            let mut visit = |j: usize| {
                if grown[j] && label[j] == 0 {
                    label[j] = lab;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        if p.area > 0 {
            parts.push(p);
        }
    }
    let biggest = parts.iter().map(|p| p.area).max().unwrap_or(0);
    let min_area = (biggest / 200).max(64);
    parts.retain(|p| p.area >= min_area);
    parts.sort_by_key(|p| p.x0);
    Labels { label, parts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_figures_three_parts() {
        let (w, h) = (200, 100);
        let mut cov = vec![0.0f32; w * h];
        let mut rect = |x0: usize, y0: usize, x1: usize, y1: usize| {
            for y in y0..y1 {
                for x in x0..x1 {
                    cov[y * w + x] = 1.0;
                }
            }
        };
        rect(10, 10, 40, 90); // big figure
        rect(41, 40, 44, 60); // weapon touching it: same part
        rect(90, 50, 96, 64); // small figure far away
        rect(160, 5, 199, 99); // figure cut by the edge
        rect(120, 2, 121, 3); // speck: dropped
        let l = label_objects(&cov, w, h);
        assert_eq!(l.parts.len(), 3);
        assert_eq!((l.parts[0].x0, l.parts[0].x1), (10, 43));
        assert_eq!((l.parts[1].x0, l.parts[1].y1), (90, 63));
    }
}

#[cfg(test)]
mod real_project {
    /// Opt-in: `PHOTOHOP_TEST_FIT=in.phop|out.phop|layer name|split,fit` splits that layer into
    /// objects and/or runs One-click fit on it (or on each part).
    #[test]
    fn fit_real() {
        let Some(spec) = std::env::var("PHOTOHOP_TEST_FIT").ok() else {
            return;
        };
        let mut it = spec.split('|');
        let (inp, out, name, what) = (it.next().unwrap(), it.next().unwrap(), it.next().unwrap(), it.next().unwrap_or("split,fit"));
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut app = crate::app::App::new(&egui::Context::default(), &device, &queue);
        app.open(std::path::Path::new(inp)).unwrap();
        let id = app.doc.layers.iter().find(|l| l.name == name).map(|l| l.id).expect("layer");
        let mut targets = vec![id];
        if what.contains("split") {
            targets = app.split_objects(id).unwrap();
            eprintln!("{}", app.status);
        }
        if what.contains("fit") {
            for t in targets {
                let n = app.doc.layer(t).unwrap().name.clone();
                eprintln!("{n}: {}", app.auto_fit(t).unwrap());
                let g = &app.doc.layer(t).unwrap().grade;
                eprintln!("   {:?}", g.scene_match.map(|m| m.matched));
            }
        } else if what.contains("colour") {
            for t in targets {
                eprintln!("{}", app.match_to_scene(t).unwrap());
            }
        }
        app.save_to(std::path::Path::new(out)).unwrap();
    }
}
