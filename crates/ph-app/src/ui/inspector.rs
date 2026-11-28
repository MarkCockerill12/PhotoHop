//! Inspector: selected layer's properties (all keyframable values have a key toggle), or the
//! canvas settings when nothing is selected.

use super::widgets::{hint, section};
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

use super::widgets::track;

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
    let is_background = app.doc.layer_index(id) == Some(0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Layer").weak());
        let mut name = layer.name.clone();
        let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
        if r.changed() {
            with_layer(app, id, "Rename", |l, _, _| l.name = name);
        }
        track(app, &r);
    });
    if is_background {
        hint(ui, "This is the bottom layer: your background. Import yourself (Ctrl+I) to put a layer on top of it; the cut-out, fit and shadow steps appear for that layer.");
    } else {
        hint(ui, "Work down the numbered steps. Hover any slider for what it does; \"? Help\" opens the manual.");
    }
    ui.separator();
    if is_background {
        position_section(app, ui, &layer, 0);
        crate::ui::colour::colour_section(app, ui, &layer, 0);
        crate::ui::integration::camera_section(app, ui, &layer, 0);
        crate::ui::integration::game_section(app, ui, &layer);
        crate::ui::glass::section(app, ui, &layer);
    } else {
        crate::ui::cutout::cutout_section(app, ui, &layer, 1);
        position_section(app, ui, &layer, 2);
        crate::ui::fit::fit_section(app, ui, &layer, 3);
        crate::ui::colour::colour_section(app, ui, &layer, 4);
        crate::ui::integration::shadow_light_section(app, ui, &layer, 5);
        crate::ui::integration::camera_section(app, ui, &layer, 6);
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Extras").weak());
        crate::ui::integration::game_section(app, ui, &layer);
        crate::ui::glass::section(app, ui, &layer);
    }
    timing_section(app, ui, &layer);
}

fn position_section(app: &mut App, ui: &mut egui::Ui, layer: &Layer, step: u8) {
    let id = layer.id;
    let t = app.time;
    let what = "Where the layer sits, how big it is and how it mixes with the layers below. Easiest: drag it on the canvas with the Move tool (V).";
    section(app, ui, step, "Position & size", what, "position", false, |app, ui| {
    egui::Grid::new("layer_grid").num_columns(3).spacing([6.0, 6.0]).show(ui, |ui| {
        ui.label("Opacity").on_hover_text("0% = invisible, 100% = solid");
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

        ui.label("Size");
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

        ui.label("Blend mode").on_hover_text("How this layer mixes with the layers below. Normal for almost everything.");
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
    ui.horizontal(|ui| {
        if ui.button("Reset").on_hover_text("Centre the layer at its original size").clicked() {
            app.edit("Reset transform", |d| {
                let canvas = d.canvas.clone();
                if let Some(l) = d.layer_mut(id) {
                    l.transform = ph_core::LayerTransform::centered(&canvas);
                }
            });
        }
        if ui.button("Fit to canvas").on_hover_text("Scale the layer so it fills the canvas").clicked() {
            let size = app.doc.layer_source_size(layer);
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
    });
    });
}

fn timing_section(app: &mut App, ui: &mut egui::Ui, layer: &Layer) {
    let id = layer.id;
    let what = "When the layer appears on the timeline, and animation keyframes (◆ buttons).";
    section(app, ui, 0, "Timing & animation", what, "timing", false, |app, ui| {
    ui.checkbox(&mut app.auto_key, "Animate: every change adds a keyframe").on_hover_text("Auto-key. Off: changes apply to the whole clip. On: each change is keyed at the playhead.");
    egui::Grid::new("timing_grid").num_columns(2).show(ui, |ui| {
        ui.label("Starts at").on_hover_text("Time on the timeline where this layer begins");
        let mut start = layer.start;
        let r = ui.add(egui::DragValue::new(&mut start).speed(0.04).range(0.0..=f64::MAX).suffix(" s"));
        if r.changed() {
            with_layer(app, id, "Layer start", |l, _, _| l.start = start);
        }
        track(app, &r);
        ui.end_row();
        if let LayerContent::Media { trim_in, .. } = layer.content {
            ui.label("Skip first").on_hover_text("Start the video this many seconds in (trim)");
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
    let tracked: Vec<(LayerId, String)> = app
        .doc
        .layers
        .iter()
        .filter(|l| l.id != id && l.mask.as_ref().is_some_and(|m| m.video.is_some()))
        .map(|l| (l.id, l.name.clone()))
        .collect();
    if !tracked.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label("Follow the tracked region of");
            for (tid, name) in tracked {
                if ui.button(&name).on_hover_text("Keyframe this layer to move with that layer's tracked mask").clicked() {
                    app.status = match app.follow_track(id, tid) {
                        Ok(n) => format!("Following \"{name}\": {n} keyframes set."),
                        Err(e) => format!("Follow failed: {e:#}"),
                    };
                }
            }
        });
    }
    });
}

fn canvas_settings(app: &mut App, ui: &mut egui::Ui) {
    if app.doc.layers.is_empty() {
        ui.heading("Getting started");
        for (n, text) in [
            (1, "Import the background (a game screenshot, photo or video): drag it onto the window, or File > Import (Ctrl+I)."),
            (2, "Import the photo or video of yourself the same way. It lands on top."),
            (3, "Select your layer and follow its numbered steps: Cut out, Position & size, Fit into the scene."),
            (4, "File > Export saves the finished image or video."),
        ] {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(format!("{n}.")).strong());
                ui.label(text);
            });
        }
        if ui.button("Open the manual (F1)").clicked() {
            app.help.open_page("start");
        }
        ui.separator();
    }
    ui.heading("Canvas");
    hint(ui, "The size and length of the final picture or video. Select a layer in the Layers list to edit that layer instead.");
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
