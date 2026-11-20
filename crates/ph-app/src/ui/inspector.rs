//! Inspector: selected layer's properties (all keyframable values have a key toggle), or the
//! canvas settings when nothing is selected.

use crate::app::App;
use ph_core::doc::LayerContent;
use ph_core::{Animated, BlendMode, Layer, LayerId};

/// Keyframe toggle: bright = key at playhead, dim gold = animated elsewhere, grey = not animated.
fn key_button<T: ph_core::anim::Lerp>(ui: &mut egui::Ui, a: &Animated<T>, t: f64) -> bool {
    let color = if a.has_key_at(t) {
        egui::Color32::from_rgb(255, 196, 64)
    } else if a.is_animated() {
        egui::Color32::from_rgb(140, 110, 50)
    } else {
        egui::Color32::from_gray(90)
    };
    let icon = egui_material_icons::icons::ICON_DIAMOND.rich_text().color(color);
    ui.add(egui::Button::new(icon).frame(false))
        .on_hover_text("Toggle keyframe at the playhead")
        .clicked()
}

/// Track a widget response as part of an undoable gesture.
fn track(app: &mut App, r: &egui::Response) {
    if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus()) {
        app.end_gesture();
    }
}

fn with_layer(app: &mut App, id: LayerId, label: &str, f: impl FnOnce(&mut Layer, f64, bool)) {
    let (t, auto) = (app.time, app.auto_key);
    if let Some(l) = app.gesture(label).layer_mut(id) {
        f(l, t, auto);
    }
}

/// Press K: toggle a transform+opacity key on the selected layer at the playhead.
pub fn toggle_all_keys(app: &mut App) {
    let Some(id) = app.selected else { return };
    let t = app.time;
    app.edit("Keyframe", |d| {
        if let Some(l) = d.layer_mut(id) {
            l.transform.position.toggle_key(t);
            l.transform.scale.toggle_key(t);
            l.transform.rotation.toggle_key(t);
            l.opacity.toggle_key(t);
        }
    });
}

pub fn inspector(app: &mut App, ui: &mut egui::Ui) {
    let Some(layer) = app.selected_layer().cloned() else {
        canvas_settings(app, ui);
        return;
    };
    let id = layer.id;
    let t = app.time;
    ui.heading("Layer");
    let mut name = layer.name.clone();
    let r = ui.text_edit_singleline(&mut name);
    if r.changed() {
        with_layer(app, id, "Rename", |l, _, _| l.name = name);
    }
    track(app, &r);
    ui.checkbox(&mut app.auto_key, "Auto-key (every change adds a keyframe)");

    ui.separator();
    egui::Grid::new("layer_grid").num_columns(3).spacing([6.0, 6.0]).show(ui, |ui| {
        ui.label("Blend");
        let mut blend = layer.blend;
        egui::ComboBox::from_id_salt("blend").selected_text(blend.label()).show_ui(ui, |ui| {
            for m in BlendMode::ALL {
                ui.selectable_value(&mut blend, m, m.label());
            }
        });
        if blend != layer.blend {
            app.edit("Blend mode", |d| {
                if let Some(l) = d.layer_mut(id) {
                    l.blend = blend;
                }
            });
        }
        ui.end_row();

        ui.label("Opacity");
        let mut op = layer.opacity.at(t) * 100.0;
        let r = ui.add(egui::Slider::new(&mut op, 0.0..=100.0).suffix("%"));
        if r.changed() {
            with_layer(app, id, "Opacity", |l, t, auto| l.opacity.set(t, op / 100.0, auto));
        }
        track(app, &r);
        if key_button(ui, &layer.opacity, t) {
            app.edit("Keyframe", |d| d.layer_mut(id).map_or((), |l| l.opacity.toggle_key(t)));
        }
        ui.end_row();

        let tr = &layer.transform;
        ui.label("Position");
        let mut p = tr.position.at(t);
        let r = ui.horizontal(|ui| {
            let a = ui.add(egui::DragValue::new(&mut p[0]).speed(1.0).prefix("x "));
            let b = ui.add(egui::DragValue::new(&mut p[1]).speed(1.0).prefix("y "));
            a | b
        });
        if r.inner.changed() {
            with_layer(app, id, "Move", |l, t, auto| l.transform.position.set(t, p, auto));
        }
        track(app, &r.inner);
        if key_button(ui, &tr.position, t) {
            app.edit("Keyframe", |d| d.layer_mut(id).map_or((), |l| l.transform.position.toggle_key(t)));
        }
        ui.end_row();

        ui.label("Scale");
        let mut s = tr.scale.at(t).map(|v| v * 100.0);
        let r = ui.horizontal(|ui| {
            let a = ui.add(egui::DragValue::new(&mut s[0]).speed(0.5).suffix("%"));
            let b = ui.add(egui::DragValue::new(&mut s[1]).speed(0.5).suffix("%"));
            a | b
        });
        if r.inner.changed() {
            with_layer(app, id, "Scale", |l, t, auto| l.transform.scale.set(t, s.map(|v| v / 100.0), auto));
        }
        track(app, &r.inner);
        if key_button(ui, &tr.scale, t) {
            app.edit("Keyframe", |d| d.layer_mut(id).map_or((), |l| l.transform.scale.toggle_key(t)));
        }
        ui.end_row();

        ui.label("Rotation");
        let mut rot = tr.rotation.at(t);
        let r = ui.add(egui::DragValue::new(&mut rot).speed(0.5).suffix("°"));
        if r.changed() {
            with_layer(app, id, "Rotate", |l, t, auto| l.transform.rotation.set(t, rot, auto));
        }
        track(app, &r);
        if key_button(ui, &tr.rotation, t) {
            app.edit("Keyframe", |d| d.layer_mut(id).map_or((), |l| l.transform.rotation.toggle_key(t)));
        }
        ui.end_row();

        if let LayerContent::Solid { color, .. } = &layer.content {
            ui.label("Colour");
            let c = color.at(t);
            let mut rgba = egui::Rgba::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            let r = egui::widgets::color_picker::color_edit_button_rgba(ui, &mut rgba, egui::color_picker::Alpha::OnlyBlend);
            if r.changed() {
                let v = [rgba.r(), rgba.g(), rgba.b(), rgba.a()];
                with_layer(app, id, "Colour", |l, t, auto| {
                    if let LayerContent::Solid { color, .. } = &mut l.content {
                        color.set(t, v, auto);
                    }
                });
            }
            track(app, &r);
            ui.end_row();
        }
    });

    ui.separator();
    ui.heading("Timing");
    egui::Grid::new("timing_grid").num_columns(2).show(ui, |ui| {
        ui.label("Starts at");
        let mut start = layer.start;
        let r = ui.add(egui::DragValue::new(&mut start).speed(0.04).range(0.0..=f64::MAX).suffix(" s"));
        if r.changed() {
            with_layer(app, id, "Layer start", |l, _, _| l.start = start);
        }
        track(app, &r);
        ui.end_row();
        if let LayerContent::Media { trim_in, .. } = layer.content {
            ui.label("Source in");
            let mut trim = trim_in;
            let r = ui.add(egui::DragValue::new(&mut trim).speed(0.04).range(0.0..=f64::MAX).suffix(" s"));
            if r.changed() {
                with_layer(app, id, "Trim", |l, _, _| {
                    if let LayerContent::Media { trim_in, .. } = &mut l.content {
                        *trim_in = trim;
                    }
                });
            }
            track(app, &r);
            ui.end_row();
        }
    });
    if ui.button("Reset transform").clicked() {
        app.edit("Reset transform", |d| {
            let canvas = d.canvas.clone();
            if let Some(l) = d.layer_mut(id) {
                l.transform = ph_core::LayerTransform::centered(&canvas);
            }
        });
    }
    if ui.button("Fit layer to canvas").clicked() {
        let size = app.doc.layer_source_size(&layer);
        let c = &app.doc.canvas;
        let k = (c.width as f32 / size.x).min(c.height as f32 / size.y);
        app.edit("Fit to canvas", |d| {
            let canvas = d.canvas.clone();
            if let Some(l) = d.layer_mut(id) {
                l.transform = ph_core::LayerTransform::centered(&canvas);
                l.transform.scale = Animated::new([k, k]);
            }
        });
    }
}

fn canvas_settings(app: &mut App, ui: &mut egui::Ui) {
    ui.heading("Canvas");
    ui.label("Select a layer to edit it.");
    let c = app.doc.canvas.clone();
    egui::Grid::new("canvas_grid").num_columns(2).show(ui, |ui| {
        ui.label("Size");
        let (mut w, mut h) = (c.width, c.height);
        let r = ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut w).range(1..=16384).suffix(" w"))
                | ui.add(egui::DragValue::new(&mut h).range(1..=16384).suffix(" h"))
        });
        if r.inner.changed() {
            let d = app.gesture("Canvas size");
            d.canvas.width = w;
            d.canvas.height = h;
        }
        track(app, &r.inner);
        ui.end_row();
        ui.label("Frame rate");
        let mut fps = c.fps;
        let r = ui.add(egui::DragValue::new(&mut fps).range(1.0..=240.0).speed(0.1).suffix(" fps"));
        if r.changed() {
            app.gesture("Frame rate").canvas.fps = fps;
        }
        track(app, &r);
        ui.end_row();
        ui.label("Duration");
        let mut dur = c.duration;
        let r = ui.add(egui::DragValue::new(&mut dur).range(0.04..=36000.0).speed(0.1).suffix(" s"));
        if r.changed() {
            app.gesture("Duration").canvas.duration = dur;
        }
        track(app, &r);
        ui.end_row();
        ui.label("Background");
        let b = c.background;
        let mut rgba = egui::Rgba::from_rgba_unmultiplied(b[0], b[1], b[2], b[3]);
        let r = egui::widgets::color_picker::color_edit_button_rgba(ui, &mut rgba, egui::color_picker::Alpha::OnlyBlend);
        if r.changed() {
            app.gesture("Background").canvas.background = [rgba.r(), rgba.g(), rgba.b(), rgba.a()];
        }
        track(app, &r);
        ui.end_row();
    });
}
