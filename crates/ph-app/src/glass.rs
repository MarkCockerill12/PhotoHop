//! "Replace window / screen" actions: set up glass on a layer, pick the view, fit it to the
//! panes, and turn AI results (found panes, tracked objects in front) into project data.

use crate::app::App;
use anyhow::{Context, Result};
use glam::Vec2;
use ph_core::{Glass, LayerId, MediaId, RotoMode, RotoPath, RotoPoint, RotoShape};
use std::path::Path;

impl App {
    /// Turn on glass for a layer (keeps earlier settings if it was set up before).
    pub fn glass_setup(&mut self, id: LayerId) {
        self.edit("Replace window / screen", |d| {
            if let Some(l) = d.layer_mut(id) {
                l.glass.get_or_insert_with(Glass::default).enabled = true;
            }
        });
    }

    /// Use a clip or image as the view through the glass (added to the project, no new layer).
    pub fn glass_set_view(&mut self, id: LayerId, path: &Path) -> Result<()> {
        let media = Self::media_ref(path)?;
        let mid = media.id;
        self.edit("Window view", |d| {
            d.media.push(media);
            if let Some(g) = d.layer_mut(id).and_then(|l| l.glass.as_mut()) {
                g.view = Some(mid);
            }
        });
        self.glass_fit_view(id);
        Ok(())
    }

    /// Scale and centre the view so it covers every pane (with a little margin for movement).
    pub fn glass_fit_view(&mut self, id: LayerId) {
        let Some(layer) = self.doc.layer(id) else { return };
        let Some(g) = &layer.glass else { return };
        let Some(view) = g.view.and_then(|m| self.doc.media(m)) else { return };
        let m = self.doc.layer_matrix(layer, self.time);
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for pane in g.panes.iter().filter(|p| p.active() && p.mode == RotoMode::Add) {
            for p in pane.polygon(self.time, 8) {
                let c = m.transform_point2(Vec2::from(p));
                lo = lo.min(c);
                hi = hi.max(c);
            }
        }
        if hi.x <= lo.x {
            // No panes yet: cover the whole canvas.
            lo = Vec2::ZERO;
            hi = Vec2::new(self.doc.canvas.width as f32, self.doc.canvas.height as f32);
        }
        let size = hi - lo;
        let k = (size.x / view.width as f32).max(size.y / view.height as f32) * 1.08;
        let centre = (lo + hi) * 0.5;
        self.edit("Fit view to glass", |d| {
            if let Some(g) = d.layer_mut(id).and_then(|l| l.glass.as_mut()) {
                g.view_transform.position.base = centre.into();
                g.view_transform.position.keys.clear();
                g.view_transform.scale.base = [k, k];
                g.view_transform.scale.keys.clear();
            }
        });
    }

    /// Panes found by AI (polygons in the layer's pixel space) replace the earlier glass shapes;
    /// keep-out (Subtract) shapes the user drew are kept.
    pub fn glass_apply_found(&mut self, id: LayerId, json: &Path) -> Result<usize> {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(json)?)?;
        let panes = v["panes"].as_array().context("no panes in the AI result")?;
        let shapes: Vec<RotoShape> = panes
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let pts: Vec<RotoPoint> = p.as_array()?.iter().filter_map(|q| Some(RotoPoint::corner([q[0].as_f64()? as f32, q[1].as_f64()? as f32]))).collect();
                (pts.len() >= 3).then(|| {
                    let mut s = RotoShape::new(format!("Pane {}", i + 1));
                    s.closed = true;
                    s.feather = ph_core::Animated::new(1.5);
                    s.path.base = RotoPath(pts);
                    s
                })
            })
            .collect();
        let n = shapes.len();
        self.edit("AI find glass", |d| {
            if let Some(g) = d.layer_mut(id).and_then(|l| l.glass.as_mut()) {
                g.panes.retain(|s| s.mode == RotoMode::Subtract);
                let keep_out = std::mem::take(&mut g.panes);
                g.panes = shapes;
                g.panes.extend(keep_out);
            }
        });
        self.glass_fit_view(id);
        Ok(n)
    }

    /// A tracked mask video of something in front of the glass.
    pub fn glass_add_front(&mut self, id: LayerId, media: MediaId) {
        self.edit("Keep in front", |d| {
            if let Some(g) = d.layer_mut(id).and_then(|l| l.glass.as_mut()) {
                g.front.push(media);
            }
        });
    }
}

impl App {
    /// Make the view look shot by the same camera as the room: same softness and sensor noise.
    pub fn glass_match_camera(&mut self, id: LayerId) -> Result<String> {
        use crate::analysis::{Luma, blur_to_match};
        let layer = self.doc.layer(id).context("layer missing")?.clone();
        let g = layer.glass.clone().context("set up Replace window first")?;
        let view = g.view.and_then(|m| self.doc.media(m)).cloned().context("choose what to see through the glass first")?;
        let t = self.time;
        let room = self.layer_frame(id).context("this layer has no pixels yet")?;
        let vt = match view.kind {
            ph_core::MediaKind::Video { duration, .. } if g.loop_view && duration > 0.0 => (t - layer.start).max(0.0) % duration,
            _ => (t - layer.start).max(0.0),
        };
        let vf = self.media.frame(&view, vt).map(|(_, f)| f).context("the view isn't loaded yet; try again in a moment")?;
        let kr = self.doc.layer_matrix(&layer, t).matrix2.x_axis.length();
        let kv = g.view_transform.scale.at(t)[0];
        let one = |_: f32, _: f32| 1.0;
        let lr = Luma::from_frame(&room, kr, &one);
        let lv = Luma::from_frame(&vf, kv, &one);
        let lim = 640.0 / (vf.width.max(vf.height) as f32 * kv);
        let blur = blur_to_match(&lv, lr.detail()) / lim.clamp(1e-3, 1.0);
        let (nv, nr) = (lv.noise(), lr.noise());
        let grain = ((nr * nr - nv * nv).max(0.0)).sqrt() / 0.408;
        // A camera focused on the room leaves the view at least a little soft.
        let defocus = blur.max(self.doc.canvas.width.max(self.doc.canvas.height) as f32 * 0.002).min(40.0);
        let auto = self.auto_key;
        self.edit("Match the camera", |d| {
            if let Some(g) = d.layer_mut(id).and_then(|l| l.glass.as_mut()) {
                g.defocus.set(t, defocus, auto);
                g.grain.set(t, grain.min(0.15), auto);
            }
        });
        Ok(format!("View matched to the room's camera: out of focus {defocus:.1}px, grain {grain:.3}."))
    }

    /// Set the focus of a layer's depth of field to the depth under a canvas point.
    pub fn pick_focus(&mut self, id: LayerId, canvas: Vec2) -> Result<String> {
        let layer = self.doc.layer(id).context("layer missing")?;
        let depth = self.depths.get(&id).context("estimate the depth first")?;
        let src = self.doc.layer_matrix(layer, self.time).inverse().transform_point2(canvas);
        let size = self.doc.layer_source_size(layer);
        let (u, v) = (src.x / size.x, src.y / size.y);
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            anyhow::bail!("click inside the layer");
        }
        let x = ((u * depth.width as f32) as u32).min(depth.width - 1);
        let y = ((v * depth.height as f32) as u32).min(depth.height - 1);
        let f = depth.data[(y * depth.width + x) as usize][0];
        let (t, auto) = (self.time, self.auto_key);
        self.edit("Pick focus", |d| {
            if let Some(l) = d.layer_mut(id) {
                l.fx.lens.focus.set(t, f, auto);
                l.fx.lens.dof = true;
            }
        });
        Ok(format!("Focus set to what you clicked (depth {f:.2})."))
    }
}
