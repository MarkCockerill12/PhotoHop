//! Roto pen tool and the inspector's "Roto shapes" section. Click to add points, drag while
//! adding to pull smooth handles, click the first point (or Enter) to close. On a closed shape,
//! drag anchors and handles (Alt breaks the handle symmetry). Delete removes the hovered point.

use crate::app::App;
use crate::render::{Drag, DragMode};
use glam::{Affine2, Vec2};
use ph_core::{Animated, LayerId, RotoMode, RotoPath, RotoPoint, RotoShape};

const HIT_R: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RotoDrag {
    Anchor(usize),
    In(usize),
    Out(usize),
    /// A point just added: dragging pulls symmetric handles out of it.
    NewHandles(usize),
}

fn layer_xf(app: &App, id: LayerId) -> Option<Affine2> {
    let l = app.doc.layer(id)?;
    Some(l.transform.matrix(app.time, app.doc.layer_source_size(l)))
}

fn to_screen(app: &App, m: &Affine2, p: [f32; 2]) -> egui::Pos2 {
    app.viewer.canvas_to_screen(m.transform_point2(Vec2::from(p)))
}

fn to_layer(app: &App, m: &Affine2, s: egui::Pos2) -> [f32; 2] {
    m.inverse().transform_point2(app.viewer.screen_to_canvas(s)).into()
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

/// The shape being edited on the selected layer.
fn active(app: &App) -> Option<(LayerId, usize)> {
    let id = app.selected?;
    let idx = app.roto_edit?;
    (idx < app.doc.layer(id)?.roto.len()).then_some((id, idx))
}

fn hit(app: &App, m: &Affine2, path: &RotoPath, s: egui::Pos2) -> Option<RotoDrag> {
    for (i, pt) in path.0.iter().enumerate() {
        if pt.h_out != [0.0; 2] && to_screen(app, m, add(pt.p, pt.h_out)).distance(s) <= HIT_R {
            return Some(RotoDrag::Out(i));
        }
        if pt.h_in != [0.0; 2] && to_screen(app, m, add(pt.p, pt.h_in)).distance(s) <= HIT_R {
            return Some(RotoDrag::In(i));
        }
    }
    path.0.iter().position(|pt| to_screen(app, m, pt.p).distance(s) <= HIT_R).map(RotoDrag::Anchor)
}

/// Change the path at the current time (keys it when auto-key is on or it is already animated).
fn edit_now(app: &mut App, id: LayerId, idx: usize, f: impl FnOnce(&mut RotoPath)) {
    let (t, auto) = (app.time, app.auto_key);
    if let Some(l) = app.gesture("Edit roto shape").layer_mut(id)
        && let Some(s) = l.roto.get_mut(idx)
    {
        let mut p = s.path.at(t);
        f(&mut p);
        s.path.set(t, p, auto);
    }
}

/// Structural change (add / remove points) applied to every key so they keep matching.
fn edit_all(app: &mut App, id: LayerId, idx: usize, label: &str, f: impl Fn(&mut RotoPath)) {
    app.edit(label, |d| {
        if let Some(s) = d.layer_mut(id).and_then(|l| l.roto.get_mut(idx)) {
            f(&mut s.path.base);
            for k in &mut s.path.keys {
                f(&mut k.value);
            }
        }
    });
}

fn make_drag(app: &App, id: LayerId, r: RotoDrag, p: egui::Pos2) -> Drag {
    Drag {
        mode: DragMode::Roto(r),
        layer: Some(id),
        target_mask: false,
        start: app.viewer.screen_to_canvas(p),
        position: [0.0; 2],
        scale: [1.0; 2],
        rotation: 0.0,
    }
}

pub fn start(app: &mut App, p: egui::Pos2) -> Option<Drag> {
    let id = app.selected?;
    let layer = app.doc.layer(id)?;
    if layer.locked {
        app.status = "This layer is locked.".into();
        return None;
    }
    let m = layer_xf(app, id)?;
    let lp = to_layer(app, &m, p);
    let t = app.time;
    if let Some((_, idx)) = active(app) {
        let shape = &app.doc.layer(id)?.roto[idx];
        let path = shape.path.at(t);
        let closed = shape.closed;
        if let Some(h) = hit(app, &m, &path, p) {
            if !closed && h == RotoDrag::Anchor(0) && path.0.len() >= 3 {
                close(app, id, idx);
                return None;
            }
            return Some(make_drag(app, id, h, p));
        }
        if !closed {
            let n = path.0.len();
            edit_all(app, id, idx, "Add roto point", |path| path.0.push(RotoPoint::corner(lp)));
            return Some(make_drag(app, id, RotoDrag::NewHandles(n), p));
        }
    }
    let n = layer.roto.len();
    app.edit("New roto shape", |d| {
        if let Some(l) = d.layer_mut(id) {
            let mut s = RotoShape::new(format!("Shape {}", n + 1));
            s.path.base = RotoPath(vec![RotoPoint::corner(lp)]);
            l.roto.push(s);
        }
    });
    app.roto_edit = Some(n);
    app.status = "Roto: click to add points, drag to curve, click the first point (or Enter) to close.".into();
    Some(make_drag(app, id, RotoDrag::NewHandles(0), p))
}

pub fn update(app: &mut App, d: &Drag, r: RotoDrag, s: egui::Pos2) {
    let (Some(id), Some((_, idx))) = (d.layer, active(app)) else { return };
    let Some(m) = layer_xf(app, id) else { return };
    let lp = to_layer(app, &m, s);
    let alt = app.egui_ctx.input(|i| i.modifiers.alt);
    edit_now(app, id, idx, |path| {
        let Some(pt) = path.0.get_mut(match r {
            RotoDrag::Anchor(i) | RotoDrag::In(i) | RotoDrag::Out(i) | RotoDrag::NewHandles(i) => i,
        }) else {
            return;
        };
        match r {
            RotoDrag::Anchor(_) => pt.p = lp,
            RotoDrag::In(_) => {
                pt.h_in = sub(lp, pt.p);
                if !alt {
                    pt.h_out = [-pt.h_in[0], -pt.h_in[1]];
                }
            }
            RotoDrag::Out(_) | RotoDrag::NewHandles(_) => {
                pt.h_out = sub(lp, pt.p);
                if !alt || matches!(r, RotoDrag::NewHandles(_)) {
                    pt.h_in = [-pt.h_out[0], -pt.h_out[1]];
                }
            }
        }
    });
}

pub fn finish(app: &mut App) {
    app.end_gesture();
}

fn close(app: &mut App, id: LayerId, idx: usize) {
    app.edit("Close roto shape", |d| {
        if let Some(s) = d.layer_mut(id).and_then(|l| l.roto.get_mut(idx)) {
            s.closed = true;
        }
    });
    app.status = "Shape closed: drag points to adjust; key the shape (diamond button) to animate it.".into();
}

/// Enter / Escape / Delete while the Roto tool is active. Returns true when a key was used.
pub fn keys(app: &mut App, ctx: &egui::Context) -> bool {
    let Some((id, idx)) = active(app) else { return false };
    let (enter, esc, del) = ctx.input(|i| {
        (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
    });
    let Some(shape) = app.doc.layer(id).map(|l| l.roto[idx].clone()) else { return false };
    let path = shape.path.at(app.time);
    if enter && !shape.closed && path.0.len() >= 3 {
        close(app, id, idx);
        return true;
    }
    if esc {
        app.roto_edit = None;
        return true;
    }
    if del {
        let hovered = ctx.input(|i| i.pointer.hover_pos()).and_then(|p| layer_xf(app, id).and_then(|m| hit(app, &m, &path, p)));
        let target = match hovered {
            Some(RotoDrag::Anchor(i)) => Some(i),
            _ if !shape.closed => path.0.len().checked_sub(1),
            _ => None,
        };
        if let Some(i) = target {
            edit_all(app, id, idx, "Delete roto point", |p| {
                if i < p.0.len() {
                    p.0.remove(i);
                }
            });
            return true;
        }
    }
    false
}

pub fn draw(app: &App, painter: &egui::Painter, hover: Option<egui::Pos2>) {
    let Some(id) = app.selected else { return };
    let Some(layer) = app.doc.layer(id) else { return };
    let Some(m) = layer_xf(app, id) else { return };
    let editing = active(app).map(|a| a.1);
    let accent = egui::Color32::from_rgb(255, 120, 200);
    for (i, s) in layer.roto.iter().enumerate() {
        let is_active = editing == Some(i);
        if !is_active && !(app.tool == crate::app::Tool::Roto && s.enabled) {
            continue;
        }
        let poly: Vec<egui::Pos2> = s.polygon(app.time, 24).into_iter().map(|p| to_screen(app, &m, p)).collect();
        let stroke = egui::Stroke::new(if is_active { 1.5 } else { 1.0 }, if is_active { accent } else { egui::Color32::from_gray(170) });
        if s.closed && poly.len() >= 3 {
            painter.add(egui::Shape::closed_line(poly, stroke));
        } else if poly.len() >= 2 {
            painter.add(egui::Shape::line(poly, stroke));
        }
        if !is_active {
            continue;
        }
        let path = s.path.at(app.time);
        if !s.closed
            && let (Some(last), Some(h)) = (path.0.last(), hover)
        {
            painter.line_segment([to_screen(app, &m, last.p), h], egui::Stroke::new(1.0, accent.gamma_multiply(0.5)));
        }
        for (k, pt) in path.0.iter().enumerate() {
            let a = to_screen(app, &m, pt.p);
            for h in [pt.h_in, pt.h_out] {
                if h != [0.0; 2] {
                    let hp = to_screen(app, &m, add(pt.p, h));
                    painter.line_segment([a, hp], egui::Stroke::new(1.0, accent));
                    painter.circle_filled(hp, 3.5, accent);
                }
            }
            let fill = if k == 0 && !s.closed { accent } else { egui::Color32::WHITE };
            painter.rect_filled(egui::Rect::from_center_size(a, egui::vec2(7.0, 7.0)), 1.0, fill);
            painter.rect_stroke(egui::Rect::from_center_size(a, egui::vec2(7.0, 7.0)), 1.0, egui::Stroke::new(1.0, egui::Color32::BLACK), egui::StrokeKind::Outside);
        }
    }
}

fn shape_slider(app: &mut App, ui: &mut egui::Ui, (id, idx): (LayerId, usize), label: &str, value: f32, range: std::ops::RangeInclusive<f32>, pick: fn(&mut RotoShape) -> &mut Animated<f32>) {
    ui.label(label);
    let mut v = value;
    let r = ui.add(egui::Slider::new(&mut v, range).max_decimals(1));
    if r.changed() {
        let (t, auto) = (app.time, app.auto_key);
        if let Some(s) = app.gesture(label).layer_mut(id).and_then(|l| l.roto.get_mut(idx)) {
            pick(s).set(t, v, auto);
        }
    }
    if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus()) {
        app.end_gesture();
    }
    ui.end_row();
}

pub fn section(app: &mut App, ui: &mut egui::Ui, layer: &ph_core::Layer) {
    let id = layer.id;
    let t = app.time;
    egui::CollapsingHeader::new(format!("Roto shapes ({})", layer.roto.len())).default_open(!layer.roto.is_empty()).show(ui, |ui| {
        ui.label(egui::RichText::new("Pen (P): click points, drag to curve, click the first point to close. The diamond button keys the shape so it can follow you.").weak().small());
        if ui.button("New shape").clicked() {
            app.tool = crate::app::Tool::Roto;
            app.roto_edit = None;
            app.status = "Roto: click on the canvas to place the first point.".into();
        }
        let mut remove = None;
        for (i, s) in layer.roto.iter().enumerate() {
            let editing = app.roto_edit == Some(i);
            ui.horizontal(|ui| {
                if ui.selectable_label(editing, &s.name).on_hover_text("Edit this shape with the Roto pen").clicked() {
                    app.roto_edit = Some(i);
                    app.tool = crate::app::Tool::Roto;
                }
                let mut on = s.enabled;
                if ui.checkbox(&mut on, "").on_hover_text("Enabled").changed() {
                    app.edit("Toggle roto shape", |d| d.layer_mut(id).and_then(|l| l.roto.get_mut(i)).map_or((), |s| s.enabled = on));
                }
                let mut mode = s.mode;
                egui::ComboBox::from_id_salt(("roto_mode", i)).width(78.0).selected_text(if mode == RotoMode::Add { "Add" } else { "Subtract" }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut mode, RotoMode::Add, "Add");
                    ui.selectable_value(&mut mode, RotoMode::Subtract, "Subtract");
                });
                if mode != s.mode {
                    app.edit("Roto mode", |d| d.layer_mut(id).and_then(|l| l.roto.get_mut(i)).map_or((), |s| s.mode = mode));
                }
                let mut inv = s.invert;
                if ui.checkbox(&mut inv, "Inv").changed() {
                    app.edit("Invert roto shape", |d| d.layer_mut(id).and_then(|l| l.roto.get_mut(i)).map_or((), |s| s.invert = inv));
                }
                let keyed = s.path.key_index_at(t).is_some();
                let key_txt = egui_material_icons::icons::ICON_DIAMOND.rich_text().color(if keyed { egui::Color32::from_rgb(255, 196, 64) } else { egui::Color32::GRAY });
                if ui.button(key_txt).on_hover_text("Keyframe the shape here").clicked() {
                    app.edit("Key roto shape", |d| d.layer_mut(id).and_then(|l| l.roto.get_mut(i)).map_or((), |s| s.path.toggle_key(t)));
                }
                if ui.button(egui_material_icons::icons::ICON_DELETE.rich_text()).on_hover_text("Delete shape").clicked() {
                    remove = Some(i);
                }
            });
            if editing {
                if !s.closed {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Open — still drawing").small().color(egui::Color32::from_rgb(255, 196, 64)));
                        if s.path.at(t).0.len() >= 3 && ui.small_button("Close shape").clicked() {
                            close(app, id, i);
                        }
                    });
                }
                egui::Grid::new(("roto_grid", i)).num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                    shape_slider(app, ui, (id, i), "Feather", s.feather.at(t), 0.0..=200.0, |s| &mut s.feather);
                    shape_slider(app, ui, (id, i), "Opacity", s.opacity.at(t), 0.0..=1.0, |s| &mut s.opacity);
                });
            }
        }
        if let Some(i) = remove {
            app.edit("Delete roto shape", |d| {
                if let Some(l) = d.layer_mut(id)
                    && i < l.roto.len()
                {
                    l.roto.remove(i);
                }
            });
            app.roto_edit = None;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ph_core::{Animated, Layer, LayerContent};

    fn app() -> Option<App> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        let mut app = App::new(&egui::Context::default(), &device, &queue);
        let canvas = app.doc.canvas.clone();
        let l = Layer::new("Solid", LayerContent::Solid { color: Animated::new([1.0; 4]), width: canvas.width, height: canvas.height }, &canvas);
        app.selected = Some(l.id);
        app.doc.layers.push(l);
        app.viewer.zoom = 1.0;
        app.viewer.canvas_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(canvas.width as f32, canvas.height as f32));
        app.tool = crate::app::Tool::Roto;
        Some(app)
    }

    fn click(app: &mut App, x: f32, y: f32, drag_to: Option<(f32, f32)>) {
        if let Some(d) = start(app, egui::pos2(x, y)) {
            let (tx, ty) = drag_to.unwrap_or((x, y));
            if let DragMode::Roto(r) = d.mode {
                update(app, &d, r, egui::pos2(tx, ty));
            }
            finish(app);
        }
    }

    #[test]
    fn pen_draws_curves_closes_and_animates() {
        let Some(mut app) = app() else { return };
        click(&mut app, 100.0, 100.0, None);
        click(&mut app, 300.0, 100.0, Some((340.0, 100.0))); // smooth point with handles
        click(&mut app, 300.0, 300.0, None);
        click(&mut app, 100.0, 300.0, None);
        click(&mut app, 101.0, 101.0, None); // first point closes
        let id = app.selected.unwrap();
        let s = &app.doc.layer(id).unwrap().roto[0];
        assert!(s.closed, "shape should be closed");
        assert_eq!(s.path.base.0.len(), 4);
        assert_eq!(s.path.base.0[1].h_out, [40.0, 0.0]);
        assert_eq!(s.path.base.0[1].h_in, [-40.0, 0.0]);
        // Key at 0, move a point at 1 s with auto-key: the shape animates.
        app.edit("key", |d| d.layer_mut(id).unwrap().roto[0].path.toggle_key(0.0));
        app.time = 1.0;
        app.auto_key = true;
        click(&mut app, 100.0, 300.0, Some((60.0, 340.0)));
        let s = &app.doc.layer(id).unwrap().roto[0];
        assert_eq!(s.path.at(1.0).0[3].p, [60.0, 340.0]);
        assert_eq!(s.path.at(0.5).0[3].p, [80.0, 320.0]);
        // Undo restores the unmoved shape.
        app.undo();
        assert_eq!(app.doc.layer(id).unwrap().roto[0].path.at(1.0).0[3].p, [100.0, 300.0]);
    }
}
