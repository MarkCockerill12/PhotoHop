//! Canvas viewer: composite display, zoom/pan, and the Move / Crop / Brush / Eraser / Hand tools.

use crate::app::{App, PixelUndo, Tool};
use crate::render::{self, Drag, DragMode};
use glam::Vec2;
use ph_core::doc::LayerContent;
use ph_core::LayerId;

const HANDLE_R: f32 = 6.0;
const ROTATE_OFFSET: f32 = 28.0;

fn to_pos(v: Vec2) -> egui::Pos2 {
    egui::pos2(v.x, v.y)
}

fn to_vec(p: egui::Pos2) -> Vec2 {
    Vec2::new(p.x, p.y)
}

/// Screen-space corners (TL, TR, BR, BL) of a layer at the current time.
fn layer_corners(app: &App, id: LayerId) -> Option<[egui::Pos2; 4]> {
    let l = app.doc.layer(id)?;
    let size = app.doc.layer_source_size(l);
    let m = l.transform.matrix(app.time, size);
    let pts = [Vec2::ZERO, Vec2::new(size.x, 0.0), size, Vec2::new(0.0, size.y)];
    Some(pts.map(|p| app.viewer.canvas_to_screen(m.transform_point2(p))))
}

fn rotate_handle(c: &[egui::Pos2; 4]) -> egui::Pos2 {
    let top_mid = c[0] + (c[1] - c[0]) * 0.5;
    let centre = c[0] + (c[2] - c[0]) * 0.5;
    let out = (top_mid - centre).normalized();
    top_mid + out * ROTATE_OFFSET
}

fn point_in_quad(p: egui::Pos2, q: &[egui::Pos2; 4]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let cross = (b - a).x * (p - a).y - (b - a).y * (p - a).x;
        if cross.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// Topmost visible, unlocked layer under a screen point.
fn pick_layer(app: &App, p: egui::Pos2) -> Option<LayerId> {
    app.doc
        .layers
        .iter()
        .rev()
        .filter(|l| l.visible && !l.locked && l.active_at(app.time))
        .find(|l| layer_corners(app, l.id).is_some_and(|c| point_in_quad(p, &c)))
        .map(|l| l.id)
}

pub fn viewer(app: &mut App, ui: &mut egui::Ui, rs: &egui_wgpu::RenderState) {
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    let cs = egui::vec2(app.doc.canvas.width as f32, app.doc.canvas.height as f32);

    if app.viewer.fit_pending && rect.width() > 10.0 {
        app.viewer.zoom = (rect.width() / cs.x).min(rect.height() / cs.y) * 0.95;
        app.viewer.pan = egui::Vec2::ZERO;
        app.viewer.fit_pending = false;
    }
    if let Some(hover) = resp.hover_pos() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        let pinch = ui.input(|i| i.zoom_delta());
        let factor = pinch * (scroll / 400.0).exp();
        if (factor - 1.0).abs() > 1e-4 {
            let old = app.viewer.zoom;
            let new = (old * factor).clamp(0.02, 64.0);
            let anchor = hover - (rect.center() + app.viewer.pan);
            app.viewer.pan += anchor - anchor * (new / old);
            app.viewer.zoom = new;
        }
    }
    let canvas_rect = egui::Rect::from_center_size(rect.center() + app.viewer.pan, cs * app.viewer.zoom);
    app.viewer.canvas_rect = canvas_rect;

    // Composite at the playhead (non-blocking: videos show the nearest decoded frame).
    let t = app.time;
    let media = &mut app.media;
    render::composite(&mut app.viewer.gpu, &app.doc, t, &app.paint, &app.masks, &app.normals, |m, mt| media.frame(m, mt));
    app.viewer.sync_texture(rs);
    if app.scopes.show {
        if let Some(img) = app.viewer.gpu.take_scope() {
            app.scopes.set_image(img);
        }
        app.viewer.gpu.request_scope(480);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
    }

    let painter = ui.painter_at(rect);
    if let Some(id) = app.viewer.tex_id {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(id, canvas_rect, uv, egui::Color32::WHITE);
    }
    painter.rect_stroke(canvas_rect, 0.0, (1.0, egui::Color32::from_gray(70)), egui::StrokeKind::Outside);

    handle_tools(app, ui, &resp);
    draw_overlays(app, &painter, &resp);
}

fn handle_tools(app: &mut App, ui: &egui::Ui, resp: &egui::Response) {
    let (shift, space, middle) = ui.input(|i| {
        (i.modifiers.shift, i.key_down(egui::Key::Space), i.pointer.button_down(egui::PointerButton::Middle))
    });
    let pointer = resp.interact_pointer_pos().or(resp.hover_pos());

    // Panning works from any tool (middle mouse or Space-drag).
    if resp.dragged() && (app.tool == Tool::Hand || middle || space) {
        app.viewer.pan += resp.drag_delta();
        return;
    }

    if resp.drag_started()
        && let Some(p) = pointer
    {
        app.viewer.drag = start_drag(app, p);
    }

    if let (Some(drag), Some(p)) = (app.viewer.drag, pointer)
        && resp.dragged()
    {
        update_drag(app, drag, to_vec(p), shift);
    }

    if resp.drag_stopped() {
        finish_drag(app);
    }

    if resp.clicked() && let Some(p) = pointer {
        match app.tool {
            Tool::Move => app.selected = pick_layer(app, p),
            Tool::Select if app.select_shape == crate::selection::SelectShape::Wand => {
                use crate::selection::Combine;
                let mode = if shift { Combine::Add } else if ui.input(|i| i.modifiers.alt) { Combine::Subtract } else { Combine::Replace };
                let seed = app.viewer.screen_to_canvas(p);
                if let Err(e) = app.apply_selection(None, Some(seed), mode) {
                    app.status = format!("Magic wand: {e:#}");
                }
            }
            Tool::Brush | Tool::Eraser | Tool::Roto => {
                if let Some(d) = start_drag(app, p) {
                    update_drag(app, d, to_vec(p), shift);
                    app.viewer.drag = Some(d);
                    finish_drag(app);
                }
            }
            _ => {}
        }
    }

    if app.tool == Tool::Crop && app.viewer.crop.is_some() {
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            apply_crop(app);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.viewer.crop = None;
        }
    }
}

fn start_drag(app: &mut App, p: egui::Pos2) -> Option<Drag> {
    let start = app.viewer.screen_to_canvas(p);
    let base = |mode, layer: Option<LayerId>| {
        let l = layer.and_then(|id| app.doc.layer(id));
        Drag {
            mode,
            layer,
            target_mask: false,
            start,
            position: l.map_or([0.0; 2], |l| l.transform.position.at(app.time)),
            scale: l.map_or([1.0; 2], |l| l.transform.scale.at(app.time)),
            rotation: l.map_or(0.0, |l| l.transform.rotation.at(app.time)),
        }
    };
    if app.ai.picking_box {
        app.viewer.ai_box = Some((start, start));
        return Some(base(DragMode::AiBox, app.selected));
    }
    match app.tool {
        Tool::Hand => None,
        Tool::Roto => crate::ui::roto::start(app, p),
        Tool::Crop => {
            app.viewer.crop = Some((start, start));
            Some(base(DragMode::Crop, None))
        }
        Tool::Select => {
            if app.select_shape == crate::selection::SelectShape::Wand {
                return None;
            }
            app.viewer.select_pts = vec![start, start];
            Some(base(DragMode::Select, app.selected))
        }
        Tool::Move => {
            if let Some(id) = app.selected.filter(|id| app.doc.layer(*id).is_some_and(|l| !l.locked))
                && let Some(c) = layer_corners(app, id)
            {
                if rotate_handle(&c).distance(p) <= HANDLE_R * 1.6 {
                    return Some(base(DragMode::Rotate, Some(id)));
                }
                if c.iter().any(|q| q.distance(p) <= HANDLE_R * 1.6) {
                    return Some(base(DragMode::Scale, Some(id)));
                }
                if point_in_quad(p, &c) {
                    return Some(base(DragMode::Move, Some(id)));
                }
            }
            let id = pick_layer(app, p)?;
            app.selected = Some(id);
            Some(base(DragMode::Move, Some(id)))
        }
        Tool::Brush | Tool::Eraser => {
            let id = app.selected?;
            let layer = app.doc.layer(id)?;
            if layer.locked {
                app.status = "This layer is locked.".into();
                return None;
            }
            if app.edit_mask && layer.mask.is_some() {
                app.masks.get_mut(&id)?.begin_stroke();
                return Some(Drag { target_mask: true, ..base(DragMode::Paint, Some(id)) });
            }
            if !matches!(layer.content, LayerContent::Paint { .. }) {
                app.status = "Brush paints on paint layers or masks: add a paint layer, or Add mask in the inspector.".into();
                return None;
            }
            app.paint.get_mut(&id)?.begin_stroke();
            Some(base(DragMode::Paint, Some(id)))
        }
    }
}

fn update_drag(app: &mut App, d: Drag, screen: Vec2, shift: bool) {
    let cur = app.viewer.screen_to_canvas(to_pos(screen));
    let (t, auto) = (app.time, app.auto_key);
    match d.mode {
        DragMode::Roto(r) => crate::ui::roto::update(app, &d, r, to_pos(screen)),
        DragMode::Crop => {
            if let Some((a, _)) = app.viewer.crop {
                app.viewer.crop = Some((a, cur));
            }
        }
        DragMode::AiBox => {
            if let Some((a, _)) = app.viewer.ai_box {
                app.viewer.ai_box = Some((a, cur));
            }
        }
        DragMode::Select => {
            let pts = &mut app.viewer.select_pts;
            if app.select_shape == crate::selection::SelectShape::Lasso {
                if pts.last().is_none_or(|l| l.distance(cur) > 1.5 / app.viewer.zoom) {
                    pts.push(cur);
                }
            } else if pts.len() >= 2 {
                pts[1] = cur;
            }
        }
        DragMode::Move => {
            let delta = cur - d.start;
            let p = [d.position[0] + delta.x, d.position[1] + delta.y];
            if let Some(l) = d.layer.and_then(|id| app.gesture("Move").layer_mut(id)) {
                l.transform.position.set(t, p, auto);
            }
        }
        DragMode::Scale => {
            let c = Vec2::from(d.position);
            let (a, b) = (d.start - c, cur - c);
            let s = if shift {
                // Free scale along the layer's own axes.
                let rot = glam::Mat2::from_angle(-d.rotation.to_radians());
                let (la, lb) = (rot * a, rot * b);
                let ratio = |n: f32, o: f32| if o.abs() > 1e-3 { n / o } else { 1.0 };
                [d.scale[0] * ratio(lb.x, la.x), d.scale[1] * ratio(lb.y, la.y)]
            } else {
                let k = b.length() / a.length().max(1e-3);
                [d.scale[0] * k, d.scale[1] * k]
            };
            if let Some(l) = d.layer.and_then(|id| app.gesture("Scale").layer_mut(id)) {
                l.transform.scale.set(t, s, auto);
            }
        }
        DragMode::Rotate => {
            let c = Vec2::from(d.position);
            let (a, b) = (d.start - c, cur - c);
            let mut deg = d.rotation + (b.y.atan2(b.x) - a.y.atan2(a.x)).to_degrees();
            if shift {
                deg = (deg / 15.0).round() * 15.0;
            }
            if let Some(l) = d.layer.and_then(|id| app.gesture("Rotate").layer_mut(id)) {
                l.transform.rotation.set(t, deg, auto);
            }
        }
        DragMode::Paint => {
            let Some(id) = d.layer else { return };
            let Some(layer) = app.doc.layer(id) else { return };
            let inv = app.doc.layer_matrix(layer, t).inverse();
            let local = inv.transform_point2(cur);
            let scale = app.doc.layer_matrix(layer, t).matrix2.x_axis.length().max(1e-3);
            let mut brush = app.brush;
            brush.size /= scale;
            let eraser = app.tool == Tool::Eraser;
            if d.target_mask {
                // Masks: brush hides (black), eraser reveals (white).
                brush.color = if eraser { egui::Color32::WHITE } else { egui::Color32::BLACK };
                if let Some(r) = app.masks.get_mut(&id) {
                    r.stroke_to(local.x, local.y, &brush, false);
                }
            } else if let Some(r) = app.paint.get_mut(&id) {
                r.stroke_to(local.x, local.y, &brush, eraser);
            }
        }
    }
}

fn finish_drag(app: &mut App) {
    let Some(d) = app.viewer.drag.take() else { return };
    match d.mode {
        DragMode::Roto(_) => crate::ui::roto::finish(app),
        DragMode::Paint => {
            let rasters = if d.target_mask { &mut app.masks } else { &mut app.paint };
            if let Some(id) = d.layer
                && let Some(patch) = rasters.get_mut(&id).and_then(|r| r.end_stroke())
            {
                app.pixel_undo.push(PixelUndo::Stroke { layer: id, mask: d.target_mask, patch });
                app.pixel_redo.clear();
                app.dirty = true;
            }
        }
        DragMode::Crop => {}
        DragMode::Select => {
            use crate::selection::{Combine, SelectShape};
            let pts = std::mem::take(&mut app.viewer.select_pts);
            let (shift, alt) = app.egui_ctx.input(|i| (i.modifiers.shift, i.modifiers.alt));
            let mode = if shift { Combine::Add } else if alt { Combine::Subtract } else { Combine::Replace };
            let poly: Vec<Vec2> = match app.select_shape {
                SelectShape::Rect if pts.len() >= 2 => {
                    let (a, b) = (pts[0], pts[1]);
                    vec![a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)]
                }
                SelectShape::Ellipse if pts.len() >= 2 => {
                    let (c, r) = ((pts[0] + pts[1]) * 0.5, (pts[1] - pts[0]) * 0.5);
                    (0..96).map(|i| { let t = i as f32 / 96.0 * std::f32::consts::TAU; c + Vec2::new(t.cos() * r.x, t.sin() * r.y) }).collect()
                }
                SelectShape::Lasso => pts,
                _ => Vec::new(),
            };
            if poly.len() >= 3
                && let Err(e) = app.apply_selection(Some(&poly), None, mode)
            {
                app.status = format!("Selection: {e:#}");
            }
        }
        DragMode::AiBox => {
            app.ai.picking_box = false;
            if let (Some((a, b)), Some(id)) = (app.viewer.ai_box.take(), d.layer)
                && let Some(layer) = app.doc.layer(id)
            {
                let inv = app.doc.layer_matrix(layer, app.time).inverse();
                let (p, q) = (inv.transform_point2(a), inv.transform_point2(b));
                let (lo, hi) = (p.min(q), p.max(q));
                if (hi - lo).min_element() < 4.0 {
                    app.status = "Box too small — drag a rectangle around the region to track.".into();
                } else {
                    let b = [lo.x, lo.y, hi.x, hi.y];
                    let task = if std::mem::take(&mut app.ai.box_for_glass) { crate::ai::AiTask::GlassFront(b) } else { crate::ai::AiTask::TrackBox(b) };
                    if let Err(e) = app.start_ai(task) {
                        app.status = format!("AI: {e:#}");
                    }
                }
            }
        }
        _ => app.end_gesture(),
    }
}

fn apply_crop(app: &mut App) {
    let Some((a, b)) = app.viewer.crop.take() else { return };
    let (cw, ch) = (app.doc.canvas.width as f32, app.doc.canvas.height as f32);
    let min = a.min(b).max(Vec2::ZERO).round();
    let max = a.max(b).min(Vec2::new(cw, ch)).round();
    let size = max - min;
    if size.x < 2.0 || size.y < 2.0 {
        return;
    }
    app.edit("Crop", |d| d.crop_canvas(min.x, min.y, size.x as u32, size.y as u32));
    app.viewer.fit_pending = true;
    app.status = format!("Cropped to {}×{}", size.x, size.y);
}

fn draw_overlays(app: &App, painter: &egui::Painter, resp: &egui::Response) {
    let accent = egui::Color32::from_rgb(90, 170, 255);
    crate::ui::roto::draw(app, painter, resp.hover_pos());
    if app.tool == Tool::Move
        && let Some(id) = app.selected
        && let Some(c) = layer_corners(app, id)
    {
        painter.add(egui::Shape::closed_line(c.to_vec(), egui::Stroke::new(1.5, accent)));
        for q in c {
            painter.rect_filled(egui::Rect::from_center_size(q, egui::vec2(HANDLE_R * 2.0, HANDLE_R * 2.0)), 1.0, egui::Color32::WHITE);
        }
        let top_mid = c[0] + (c[1] - c[0]) * 0.5;
        let rh = rotate_handle(&c);
        painter.line_segment([top_mid, rh], egui::Stroke::new(1.0, accent));
        painter.circle(rh, HANDLE_R, egui::Color32::WHITE, egui::Stroke::new(1.0, accent));
    }
    if app.viewer.select_pts.len() >= 2 {
        let pts: Vec<egui::Pos2> = app.viewer.select_pts.iter().map(|p| app.viewer.canvas_to_screen(*p)).collect();
        let stroke = egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 196, 64));
        match app.select_shape {
            crate::selection::SelectShape::Lasso => {
                painter.add(egui::Shape::line(pts, stroke));
            }
            crate::selection::SelectShape::Ellipse => {
                let r = egui::Rect::from_two_pos(pts[0], pts[1]);
                painter.add(egui::Shape::ellipse_stroke(r.center(), r.size() * 0.5, stroke));
            }
            _ => {
                painter.rect_stroke(egui::Rect::from_two_pos(pts[0], pts[1]), 0.0, stroke, egui::StrokeKind::Middle);
            }
        }
    }
    if let Some((a, b)) = app.viewer.ai_box {
        let r = egui::Rect::from_two_pos(app.viewer.canvas_to_screen(a), app.viewer.canvas_to_screen(b));
        painter.rect_stroke(r, 0.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 196, 64)), egui::StrokeKind::Middle);
    }
    if let Some((a, b)) = app.viewer.crop {
        let r = egui::Rect::from_two_pos(app.viewer.canvas_to_screen(a), app.viewer.canvas_to_screen(b));
        let full = app.viewer.canvas_rect;
        let shade = egui::Color32::from_black_alpha(140);
        for s in [
            egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, r.min.y)),
            egui::Rect::from_min_max(egui::pos2(full.min.x, r.max.y), full.max),
            egui::Rect::from_min_max(egui::pos2(full.min.x, r.min.y), egui::pos2(r.min.x, r.max.y)),
            egui::Rect::from_min_max(egui::pos2(r.max.x, r.min.y), egui::pos2(full.max.x, r.max.y)),
        ] {
            painter.rect_filled(s, 0.0, shade);
        }
        painter.rect_stroke(r, 0.0, egui::Stroke::new(1.5, egui::Color32::WHITE), egui::StrokeKind::Middle);
    }
    if matches!(app.tool, Tool::Brush | Tool::Eraser)
        && let Some(p) = resp.hover_pos()
    {
        let r = app.brush.size * 0.5 * app.viewer.zoom;
        painter.circle_stroke(p, r.max(1.0), egui::Stroke::new(1.0, egui::Color32::WHITE));
        painter.circle_stroke(p, (r - 1.0).max(1.0), egui::Stroke::new(1.0, egui::Color32::BLACK));
    }
}
