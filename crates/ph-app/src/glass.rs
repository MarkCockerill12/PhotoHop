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
