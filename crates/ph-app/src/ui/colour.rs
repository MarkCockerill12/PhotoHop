//! Colour section of the inspector: Resolve-style lift/gamma/gain wheels, basic grade sliders,
//! non-AI colour matching and the layer mask controls.

use crate::app::App;
use crate::matching::MatchMode;
use ph_core::{Animated, Grade, Layer, LayerId};
use std::f32::consts::TAU;

type Pick = fn(&mut Grade) -> &mut Animated<[f32; 4]>;
/// (label, current value, rim range, field accessor, master range)
type WheelSpec = (&'static str, [f32; 4], f32, Pick, std::ops::RangeInclusive<f32>);

/// Opponent-plane basis: red at 0°, green at 120°, blue at 240° (|a| = |b|, a ⟂ b).
const AX: [f32; 3] = [1.0, -0.5, -0.5];
const BX: [f32; 3] = [0.0, 0.866_025_4, -0.866_025_4];

fn rgb_to_plane(rgb: [f32; 3]) -> egui::Vec2 {
    let m = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
    let z = rgb.map(|v| v - m);
    let dot = |a: [f32; 3]| (z[0] * a[0] + z[1] * a[1] + z[2] * a[2]) / 1.5;
    egui::vec2(dot(AX), dot(BX))
}

fn plane_to_rgb(v: egui::Vec2, mean: f32) -> [f32; 3] {
    std::array::from_fn(|i| mean + v.x * AX[i] + v.y * BX[i])
}

/// A colour wheel editing the colour (zero-mean) part of `value[0..3]`; `range` is the offset at
/// the rim. Double-click resets the colour. Returns true while changed.
fn wheel(ui: &mut egui::Ui, id: &str, value: &mut [f32; 4], range: f32) -> egui::Response {
    let size = 92.0;
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click_and_drag());
    let c = rect.center();
    let r = size * 0.5 - 4.0;
    let p = ui.painter_at(rect);
    p.circle_filled(c, r, egui::Color32::from_gray(30));
    let n = 72;
    for i in 0..n {
        let (a0, a1) = (i as f32 / n as f32 * TAU, (i + 1) as f32 / n as f32 * TAU);
        let hue = egui::ecolor::Hsva::new(i as f32 / n as f32, 0.85, 0.9, 1.0);
        let pt = |a: f32| c + egui::vec2(a.cos(), -a.sin()) * r;
        p.line_segment([pt(a0), pt(a1)], egui::Stroke::new(3.0, egui::Color32::from(hue)));
    }
    p.line_segment([c - egui::vec2(6.0, 0.0), c + egui::vec2(6.0, 0.0)], (1.0, egui::Color32::GRAY));
    p.line_segment([c - egui::vec2(0.0, 6.0), c + egui::vec2(0.0, 6.0)], (1.0, egui::Color32::GRAY));

    let rgb = [value[0], value[1], value[2]];
    let mean = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
    let v = rgb_to_plane(rgb) / range;
    let puck = c + egui::vec2(v.x, -v.y) * r;
    if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let mut d = (pos - c) / r;
        if d.length() > 1.0 {
            d = d.normalized();
        }
        let out = plane_to_rgb(egui::vec2(d.x, -d.y) * range, mean);
        value[..3].copy_from_slice(&out);
        resp.mark_changed();
    }
    if resp.double_clicked() {
        value[..3].copy_from_slice(&[mean; 3]);
        resp.mark_changed();
    }
    p.circle(puck, 5.0, egui::Color32::WHITE, egui::Stroke::new(1.0, egui::Color32::BLACK));
    resp.on_hover_text(format!("{id}: drag to tint, double-click to reset"))
}

fn grade_edit(app: &mut App, id: LayerId, label: &str, f: impl FnOnce(&mut Grade, f64, bool)) {
    let (t, auto) = (app.time, app.auto_key);
    if let Some(l) = app.gesture(label).layer_mut(id) {
        f(&mut l.grade, t, auto);
    }
}

fn track(app: &mut App, r: &egui::Response) {
    if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus()) {
        app.end_gesture();
    }
}

/// Slider row for a scalar grade parameter.
fn scalar(
    app: &mut App,
    ui: &mut egui::Ui,
    id: LayerId,
    label: &str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    pick: fn(&mut Grade) -> &mut Animated<f32>,
) {
    ui.label(label);
    let mut v = value;
    let r = ui.add(egui::Slider::new(&mut v, range).max_decimals(3));
    if r.changed() {
        grade_edit(app, id, label, |g, t, auto| pick(g).set(t, v, auto));
    }
    track(app, &r);
    ui.end_row();
}

pub fn colour_section(app: &mut App, ui: &mut egui::Ui, layer: &Layer) {
    let id = layer.id;
    let t = app.time;
    let g = layer.grade.clone();
    egui::CollapsingHeader::new("Colour").default_open(true).show(ui, |ui| {
        ui.horizontal(|ui| {
            let mut enabled = g.enabled;
            if ui.checkbox(&mut enabled, "Enabled").changed() {
                app.edit("Toggle grade", |d| d.layer_mut(id).map_or((), |l| l.grade.enabled = enabled));
            }
            if ui.button("Reset colour").clicked() {
                app.edit("Reset colour", |d| d.layer_mut(id).map_or((), |l| l.grade = Grade::default()));
            }
        });

        // Wheels: lift / gamma / gain, each with a master slider underneath.
        ui.horizontal(|ui| {
            let wheels: [WheelSpec; 3] = [
                ("Lift", g.lift.at(t), 0.25, |g| &mut g.lift, -0.5..=0.5),
                ("Gamma", g.gamma.at(t), 0.5, |g| &mut g.gamma, -1.0..=1.0),
                ("Gain", g.gain.at(t), 0.5, |g| &mut g.gain, 0.0..=4.0),
            ];
            for (name, mut val, range, pick, master_range) in wheels {
                ui.vertical(|ui| {
                    ui.label(name);
                    let r = wheel(ui, name, &mut val, range);
                    if r.changed() {
                        grade_edit(app, id, name, |g, t, auto| pick(g).set(t, val, auto));
                    }
                    if r.drag_stopped() || r.double_clicked() {
                        app.end_gesture();
                    }
                    let r = ui.add(egui::DragValue::new(&mut val[3]).range(master_range).speed(0.005).max_decimals(3));
                    if r.changed() {
                        grade_edit(app, id, name, |g, t, auto| pick(g).set(t, val, auto));
                    }
                    track(app, &r);
                });
            }
        });

        egui::Grid::new("grade_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
            scalar(app, ui, id, "Exposure", g.exposure.at(t), -5.0..=5.0, |g| &mut g.exposure);
            scalar(app, ui, id, "Temperature", g.temperature.at(t), -1.0..=1.0, |g| &mut g.temperature);
            scalar(app, ui, id, "Tint", g.tint.at(t), -1.0..=1.0, |g| &mut g.tint);
            scalar(app, ui, id, "Contrast", g.contrast.at(t), 0.0..=2.0, |g| &mut g.contrast);
            scalar(app, ui, id, "Pivot", g.pivot.at(t), 0.0..=1.0, |g| &mut g.pivot);
            scalar(app, ui, id, "Saturation", g.saturation.at(t), 0.0..=2.0, |g| &mut g.saturation);
        });

        ui.separator();
        ui.label(egui::RichText::new("Match colour (no AI)").strong());
        let others: Vec<(LayerId, String)> =
            app.doc.layers.iter().filter(|l| l.id != id).map(|l| (l.id, l.name.clone())).collect();
        let current = app
            .match_reference
            .and_then(|r| others.iter().find(|(o, _)| *o == r))
            .map_or("Layer below".to_owned(), |(_, n)| n.clone());
        egui::ComboBox::from_id_salt("match_ref").selected_text(format!("Reference: {current}")).show_ui(ui, |ui| {
            ui.selectable_value(&mut app.match_reference, None, "Layer below");
            for (oid, name) in &others {
                ui.selectable_value(&mut app.match_reference, Some(*oid), name);
            }
        });
        egui::ComboBox::from_id_salt("match_mode").selected_text(app.match_mode.label()).show_ui(ui, |ui| {
            for m in MatchMode::ALL {
                ui.selectable_value(&mut app.match_mode, m, m.label());
            }
        });
        if ui.button("Match").on_hover_text("Measure both layers and set the match to fit").clicked()
            && let Err(e) = app.match_colour()
        {
            app.status = format!("Match failed: {e:#}");
        }
        egui::Grid::new("match_grid").num_columns(2).show(ui, |ui| {
            scalar(app, ui, id, "Match amount", g.match_amount.at(t), 0.0..=1.0, |g| &mut g.match_amount);
        });
    });

    egui::CollapsingHeader::new("Mask").default_open(true).show(ui, |ui| match &layer.mask {
        None => {
            ui.label("Paint to hide parts of this layer (e.g. cut yourself out).");
            if ui.button("Add mask").clicked() {
                app.add_mask();
            }
        }
        Some(m) => {
            let (mut enabled, mut invert) = (m.enabled, m.invert);
            ui.horizontal(|ui| {
                if ui.checkbox(&mut enabled, "Enabled").changed() {
                    app.edit("Toggle mask", |d| {
                        if let Some(mask) = d.layer_mut(id).and_then(|l| l.mask.as_mut()) {
                            mask.enabled = enabled;
                        }
                    });
                }
                if ui.checkbox(&mut invert, "Invert").changed() {
                    app.edit("Invert mask", |d| {
                        if let Some(mask) = d.layer_mut(id).and_then(|l| l.mask.as_mut()) {
                            mask.invert = invert;
                        }
                    });
                }
            });
            ui.checkbox(&mut app.edit_mask, "Edit mask (Brush hides, Eraser reveals)");
            if ui.button("Delete mask").clicked() {
                app.delete_mask();
            }
        }
    });
}
