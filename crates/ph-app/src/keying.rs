//! "Remove a colour" (colour key: every pixel close to a picked colour leaves the cut-out, wherever
//! it is in the picture) and "Shape from outline" (a normal map guessed from the cut-out's
//! silhouette, so Relight works on any object without AI).

use crate::app::{App, PixelUndo};
use crate::paint::PaintRaster;
use anyhow::{Context, Result, bail};
use ph_core::LayerId;
use ph_media::Frame;

/// The last picked colour, so Tolerance / Softness can re-apply it live.
pub struct KeyState {
    pub layer: LayerId,
    /// Perceptual (sRGB-encoded) colour that was picked.
    pub colour: [f32; 3],
    /// Mask before the colour was removed.
    base: Vec<[f32; 4]>,
}

/// Straight-alpha perceptual colour of a frame pixel.
fn perceptual(frame: &Frame, x: usize, y: usize) -> ([f32; 3], f32) {
    let i = (y * frame.width as usize + x) * 4;
    let a = frame.pixels[i + 3].to_f32();
    let c = std::array::from_fn(|c| if a > 1e-4 { ph_media::color::linear_to_srgb((frame.pixels[i + c].to_f32() / a).max(0.0)) } else { 0.0 });
    (c, a)
}

/// How much of each mask pixel is the keyed colour (1 = remove fully). `tolerance` is the colour
/// distance (0..1, perceptual RGB) removed completely, `softness` the fade after it.
pub fn key_coverage(frame: &Frame, w: u32, h: u32, colour: [f32; 3], tolerance: f32, softness: f32) -> Vec<f32> {
    let (fw, fh) = (frame.width as usize, frame.height as usize);
    let mut out = vec![0.0; (w * h) as usize];
    for y in 0..h as usize {
        let fy = ((y as f32 + 0.5) / h as f32 * fh as f32) as usize;
        for x in 0..w as usize {
            let fx = ((x as f32 + 0.5) / w as f32 * fw as f32) as usize;
            let (c, a) = perceptual(frame, fx.min(fw - 1), fy.min(fh - 1));
            if a <= 0.0 {
                continue;
            }
            let d = ((c[0] - colour[0]).powi(2) + (c[1] - colour[1]).powi(2) + (c[2] - colour[2]).powi(2)).sqrt();
            out[y * w as usize + x] = if d <= tolerance {
                1.0
            } else if softness > 0.0 && d < tolerance + softness {
                1.0 - (d - tolerance) / softness
            } else {
                0.0
            };
        }
    }
    out
}

/// Normal map guessed from a silhouette: the object is treated as a rounded "pillow" that rises
/// from its outline, so light wraps around its edges. Convention: x right, y up, z towards the
/// viewer, stored as `n * 0.5 + 0.5`. Outside the object the normal faces the camera.
pub fn outline_normals(cov: &[f32], w: usize, h: usize) -> Vec<[f32; 4]> {
    // Distance to the outline (two-pass chamfer, 1 / √2 steps).
    let inf = (w + h) as f32;
    let mut d: Vec<f32> = cov.iter().map(|c| if *c > 0.5 { inf } else { 0.0 }).collect();
    let (a, b) = (1.0f32, std::f32::consts::SQRT_2);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if d[i] == 0.0 {
                continue;
            }
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + a);
            } else {
                v = v.min(a);
            }
            if y > 0 {
                v = v.min(d[i - w] + a);
                if x > 0 {
                    v = v.min(d[i - w - 1] + b);
                }
                if x + 1 < w {
                    v = v.min(d[i - w + 1] + b);
                }
            } else {
                v = v.min(a);
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if d[i] == 0.0 {
                continue;
            }
            let mut v = d[i];
            if x + 1 < w {
                v = v.min(d[i + 1] + a);
            } else {
                v = v.min(a);
            }
            if y + 1 < h {
                v = v.min(d[i + w] + a);
                if x + 1 < w {
                    v = v.min(d[i + w + 1] + b);
                }
                if x > 0 {
                    v = v.min(d[i + w - 1] + b);
                }
            } else {
                v = v.min(a);
            }
            d[i] = v;
        }
    }
    // Rounded profile over the whole width (like a cylinder seen side-on), so every part leans
    // a little towards its nearest edge and the light direction shows across the whole object.
    let r = d.iter().cloned().fold(0.0f32, f32::max).max(1.0);
    let height: Vec<f32> = d
        .iter()
        .map(|v| {
            let t = (v / r).min(1.0);
            r * (1.0 - (1.0 - t) * (1.0 - t)).sqrt()
        })
        .collect();
    // Light blur so the chamfer steps don't show as facets.
    let height = box_blur(&box_blur(&height, w, h, 2), w, h, 2);
    let mut out = vec![[0.5, 0.5, 1.0, 1.0]; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if cov[i] <= 0.01 {
                continue;
            }
            let hx = |xx: usize| height[y * w + xx.min(w - 1)];
            let hy = |yy: usize| height[yy.min(h - 1) * w + x];
            let gx = (hx(x + 1) - hx(x.saturating_sub(1))) * 0.5;
            let gy = (hy(y + 1) - hy(y.saturating_sub(1))) * 0.5;
            // Height rises inwards: at the left edge it grows to the right, so the surface faces left.
            let n = glam::Vec3::new(-gx, gy, 1.0).normalize();
            out[i] = [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5, 1.0];
        }
    }
    out
}

fn box_blur(v: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0.0; v.len()];
    for y in 0..h {
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r).min(w - 1));
            tmp[y * w + x] = (x0..=x1).map(|k| v[y * w + k]).sum::<f32>() / (x1 - x0 + 1) as f32;
        }
    }
    let mut out = vec![0.0; v.len()];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r).min(h - 1));
        for x in 0..w {
            out[y * w + x] = (y0..=y1).map(|k| tmp[k * w + x]).sum::<f32>() / (y1 - y0 + 1) as f32;
        }
    }
    out
}

impl App {
    /// Remove every pixel close to the colour under canvas point `p` from the layer's cut-out.
    pub fn remove_colour_at(&mut self, id: LayerId, p: glam::Vec2) -> Result<String> {
        let layer = self.doc.layer(id).context("layer missing")?.clone();
        if layer.mask.as_ref().is_some_and(|m| m.video.is_some()) {
            bail!("this layer's cut-out is an AI video track; remove it first");
        }
        let frame = self.layer_frame(id).context("this layer has no pixels yet")?;
        let size = self.doc.layer_source_size(&layer);
        let s = self.doc.layer_matrix(&layer, self.time).inverse().transform_point2(p);
        if s.x < 0.0 || s.y < 0.0 || s.x >= size.x || s.y >= size.y {
            bail!("click on the layer itself");
        }
        let (fx, fy) = ((s.x / size.x * frame.width as f32) as usize, (s.y / size.y * frame.height as f32) as usize);
        // Average a 5×5 patch so noise doesn't decide the colour.
        let (mut sum, mut n) = ([0.0f32; 3], 0.0);
        for dy in 0..5 {
            for dx in 0..5 {
                let (x, y) = ((fx + dx).saturating_sub(2).min(frame.width as usize - 1), (fy + dy).saturating_sub(2).min(frame.height as usize - 1));
                let (c, a) = perceptual(&frame, x, y);
                if a > 0.5 {
                    for k in 0..3 {
                        sum[k] += c[k];
                    }
                    n += 1.0;
                }
            }
        }
        if n == 0.0 {
            bail!("that spot is already see-through");
        }
        let colour = sum.map(|v| v / n);
        if layer.mask.is_none() {
            let keep = self.selected;
            self.selected = Some(id);
            self.add_mask();
            self.selected = keep;
            self.edit_mask = false;
        }
        let raster = self.masks.get(&id).context("mask raster missing")?;
        let base = raster.data.clone();
        let (w, h) = (raster.width, raster.height);
        self.pixel_undo.push(PixelUndo::Stroke { layer: id, mask: true, patch: crate::paint::Patch { x: 0, y: 0, w, h, pixels: base.clone() } });
        self.pixel_redo.clear();
        self.key = Some(KeyState { layer: id, colour, base });
        self.reapply_key()?;
        let hex = colour.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        Ok(format!(
            "Removed colour #{:02x}{:02x}{:02x}. Adjust Tolerance and Softness to remove more or less; Ctrl+Z undoes.",
            hex[0], hex[1], hex[2]
        ))
    }

    /// Re-run the last colour removal with the current tolerance / softness.
    pub fn reapply_key(&mut self) -> Result<()> {
        let Some(k) = &self.key else { return Ok(()) };
        let id = k.layer;
        let (colour, base) = (k.colour, k.base.clone());
        let frame = self.layer_frame(id).context("this layer has no pixels yet")?;
        let (tol, soft) = (self.key_tolerance, self.key_softness);
        let raster = self.masks.get_mut(&id).context("mask raster missing")?;
        let cov = key_coverage(&frame, raster.width, raster.height, colour, tol, soft);
        raster.data = base;
        crate::selection::apply(&mut raster.data, &cov, crate::selection::Combine::Subtract);
        raster.revision += 1;
        self.dirty = true;
        Ok(())
    }

    /// Give the layer a normal map guessed from its outline (no AI), for Relight.
    pub fn outline_shape(&mut self, id: LayerId) -> Result<()> {
        let layer = self.doc.layer(id).context("layer missing")?.clone();
        let frame = self.layer_frame(id).context("this layer has no pixels yet")?;
        let k = (1024.0 / frame.width.max(frame.height) as f32).min(1.0);
        let (w, h) = (((frame.width as f32 * k) as usize).max(4), ((frame.height as f32 * k) as usize).max(4));
        let raster = self.masks.get(&id);
        let mask = layer.mask.as_ref().filter(|m| m.enabled && m.video.is_none());
        let cov: Vec<f32> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let (u, v) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                let fi = ((v * frame.height as f32) as usize).min(frame.height as usize - 1) * frame.width as usize + ((u * frame.width as f32) as usize).min(frame.width as usize - 1);
                let a = frame.pixels[fi * 4 + 3].to_f32();
                let m = match (raster, mask) {
                    (Some(r), Some(m)) => {
                        let v = r.data[((v * r.height as f32) as u32).min(r.height - 1) as usize * r.width as usize + ((u * r.width as f32) as u32).min(r.width - 1) as usize][0];
                        if m.invert { 1.0 - v } else { v }
                    }
                    _ => 1.0,
                };
                a * m
            })
            .collect();
        let mut normals = PaintRaster::new(w as u32, h as u32);
        normals.data = outline_normals(&cov, w, h);
        self.normals.insert(id, normals);
        self.edit("Shape from outline", |d| {
            if let Some(l) = d.layer_mut(id) {
                l.has_normals = true;
                if !l.fx.relight.enabled {
                    l.fx.relight.enabled = true;
                    strong_start(&mut l.fx.relight);
                }
            }
        });
        Ok(())
    }
}

/// An outline shape is rounder than a real body, so start with a lower sun, less fill and less
/// wrap: moving the light then visibly changes which side is lit.
pub fn strong_start(r: &mut ph_core::fx::Relight) {
    r.elevation.set(0.0, 20.0, false);
    r.ambient.set(0.0, 0.2, false);
    r.wrap = 0.1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use half::f16;

    #[test]
    fn key_removes_only_close_colours() {
        let px: Vec<f16> = [[0.5f32, 0.5, 0.5, 1.0], [0.05, 0.05, 0.6, 1.0]].iter().flatten().map(|v| f16::from_f32(*v)).collect();
        let f = Frame::new(2, 1, px);
        let grey = ph_media::color::linear_to_srgb(0.5);
        let cov = key_coverage(&f, 2, 1, [grey; 3], 0.1, 0.05);
        assert_eq!(cov, vec![1.0, 0.0]);
    }

    #[test]
    fn outline_normals_face_outwards() {
        let (w, h) = (40, 40);
        let cov: Vec<f32> = (0..w * h).map(|i| if (5..35).contains(&(i % w)) && (5..35).contains(&(i / w)) { 1.0 } else { 0.0 }).collect();
        let n = outline_normals(&cov, w, h);
        let at = |x: usize, y: usize| n[y * w + x].map(|v| v * 2.0 - 1.0);
        assert!(at(6, 20)[0] < -0.3, "left edge faces left: {:?}", at(6, 20));
        assert!(at(33, 20)[0] > 0.3, "right edge faces right");
        assert!(at(20, 6)[1] > 0.3, "top edge faces up");
        assert!(at(20, 20)[2] > 0.95, "middle faces the camera");
    }
}

#[cfg(test)]
mod real_project {
    /// Opt-in: `PHOTOHOP_TEST_KEY=in.phop|out.phop|layer|x,y|relight layer|azimuth` removes the
    /// colour at canvas (x, y) from `layer` (after dropping its old mask) and relights the other
    /// layer from `azimuth` with a shape from its outline.
    #[test]
    fn key_real() {
        let Some(spec) = std::env::var("PHOTOHOP_TEST_KEY").ok() else {
            return;
        };
        let v: Vec<&str> = spec.split('|').collect();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut app = crate::app::App::new(&egui::Context::default(), &device, &queue);
        app.open(std::path::Path::new(v[0])).unwrap();
        let find = |app: &crate::app::App, n: &str| app.doc.layers.iter().find(|l| l.name == n).map(|l| l.id).expect("layer");
        let id = find(&app, v[2]);
        app.edit("drop mask", |d| {
            if let Some(l) = d.layer_mut(id) {
                l.mask = None;
                l.visible = true;
            }
        });
        let (x, y) = v[3].split_once(',').unwrap();
        eprintln!("{}", app.remove_colour_at(id, glam::Vec2::new(x.parse().unwrap(), y.parse().unwrap())).unwrap());
        if v.len() > 5 {
            let r = find(&app, v[4]);
            app.outline_shape(r).unwrap();
            let az: f32 = v[5].parse().unwrap();
            app.edit("light", |d| {
                if let Some(l) = d.layer_mut(r) {
                    l.fx.relight.azimuth.set(0.0, az, false);
                }
            });
        }
        app.save_to(std::path::Path::new(v[1])).unwrap();
    }
}
