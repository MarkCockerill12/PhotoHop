//! Inspector "Integration" section: light wrap, blur/sharpen, grain, cast shadow and relighting.

use crate::app::App;
use ph_core::{Animated, Layer, LayerId};

fn track(app: &mut App, r: &egui::Response) {
    if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus()) {
        app.end_gesture();
    }
}

/// Slider row for a keyframable scalar inside the layer.
fn anim_row(
    app: &mut App,
    ui: &mut egui::Ui,
    id: LayerId,
    label: &str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    pick: fn(&mut Layer) -> &mut Animated<f32>,
) {
    ui.label(label);
    let mut v = value;
    let r = ui.add(egui::Slider::new(&mut v, range).max_decimals(2));
    if r.changed() {
        let (t, auto) = (app.time, app.auto_key);
        if let Some(l) = app.gesture(label).layer_mut(id) {
            pick(l).set(t, v, auto);
        }
    }
    track(app, &r);
    ui.end_row();
}

/// Plain (non-keyframed) value row.
fn plain_row(app: &mut App, ui: &mut egui::Ui, id: LayerId, label: &str, value: f32, range: std::ops::RangeInclusive<f32>, set: fn(&mut Layer, f32)) {
    ui.label(label);
    let mut v = value;
    let r = ui.add(egui::Slider::new(&mut v, range).max_decimals(2));
    if r.changed()
        && let Some(l) = app.gesture(label).layer_mut(id)
    {
        set(l, v);
    }
    track(app, &r);
    ui.end_row();
}

fn colour_row(app: &mut App, ui: &mut egui::Ui, id: LayerId, label: &str, value: [f32; 3], set: fn(&mut Layer, [f32; 3])) {
    ui.label(label);
    let mut rgba = egui::Rgba::from_rgb(value[0], value[1], value[2]);
    let r = egui::widgets::color_picker::color_edit_button_rgba(ui, &mut rgba, egui::color_picker::Alpha::Opaque);
    if r.changed()
        && let Some(l) = app.gesture(label).layer_mut(id)
    {
        set(l, [rgba.r(), rgba.g(), rgba.b()]);
    }
    track(app, &r);
    ui.end_row();
}

/// Drag pad for light direction: angle = where the light comes from, distance from centre = how
/// low it is (centre = straight from the camera, rim = grazing).
fn light_pad(app: &mut App, ui: &mut egui::Ui, id: LayerId, az: f32, el: f32) {
    let size = 110.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click_and_drag());
    let c = rect.center();
    let r = size * 0.5 - 6.0;
    let p = ui.painter_at(rect);
    p.circle_filled(c, r, egui::Color32::from_gray(28));
    for k in [0.33, 0.66, 1.0] {
        p.circle_stroke(c, r * k, egui::Stroke::new(1.0, egui::Color32::from_gray(55)));
    }
    let d = 1.0 - el.clamp(0.0, 90.0) / 90.0;
    let (a, b) = (az.to_radians().cos(), az.to_radians().sin());
    let sun = c + egui::vec2(a, -b) * d * r;
    p.line_segment([c, sun], egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 200, 90)));
    p.circle(sun, 7.0, egui::Color32::from_rgb(255, 210, 90), egui::Stroke::new(1.0, egui::Color32::BLACK));
    if resp.dragged()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let v = (pos - c) / r;
        let len = v.length().min(1.0);
        let new_az = (-v.y).atan2(v.x).to_degrees();
        let new_el = (1.0 - len) * 90.0;
        let (t, auto) = (app.time, app.auto_key);
        if let Some(l) = app.gesture("Light direction").layer_mut(id) {
            l.fx.relight.azimuth.set(t, new_az, auto);
            l.fx.relight.elevation.set(t, new_el, auto);
        }
    }
    if resp.drag_stopped() {
        app.end_gesture();
    }
    resp.on_hover_text(format!("Light from {az:.0}°, {el:.0}° up — drag the sun"));
}

pub fn integration_section(app: &mut App, ui: &mut egui::Ui, layer: &Layer) {
    let id = layer.id;
    let t = app.time;
    let fx = layer.fx.clone();
    egui::CollapsingHeader::new("Integration").default_open(true).show(ui, |ui| {
        ui.label(egui::RichText::new("Make the layer sit in the scene").weak());
        ui.horizontal(|ui| {
            if ui.button("Match grain & sharpness").on_hover_text("Measure the background's noise and softness and copy them (no AI)").clicked() {
                app.status = app.match_detail().unwrap_or_else(|e| format!("Match failed: {e:#}"));
            }
            if ui.button("Match light").on_hover_text("Estimate the scene's light colour, ambient and direction for Relight (no AI)").clicked() {
                app.status = app.match_light().unwrap_or_else(|e| format!("Match failed: {e:#}"));
            }
        });
        egui::Grid::new("fx_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
            anim_row(app, ui, id, "Light wrap", fx.light_wrap.at(t), 0.0..=1.0, |l| &mut l.fx.light_wrap);
            plain_row(app, ui, id, "Wrap width", fx.light_wrap_width, 1.0..=120.0, |l, v| l.fx.light_wrap_width = v);
            anim_row(app, ui, id, "Blur", fx.blur.at(t), 0.0..=40.0, |l| &mut l.fx.blur);
            anim_row(app, ui, id, "Sharpen", fx.sharpen.at(t), 0.0..=2.0, |l| &mut l.fx.sharpen);
            anim_row(app, ui, id, "Grain", fx.grain.at(t), 0.0..=0.15, |l| &mut l.fx.grain);
            plain_row(app, ui, id, "Grain size", fx.grain_size, 0.5..=6.0, |l, v| l.fx.grain_size = v);
            anim_row(app, ui, id, "Haze", fx.haze.at(t), 0.0..=1.0, |l| &mut l.fx.haze);
            colour_row(app, ui, id, "Haze colour", fx.haze_color, |l, v| l.fx.haze_color = v);
        });
        let mut mono = fx.grain_mono;
        if ui.checkbox(&mut mono, "Monochrome grain").changed() {
            app.edit("Grain colour", |d| d.layer_mut(id).map_or((), |l| l.fx.grain_mono = mono));
        }

        egui::CollapsingHeader::new("Game-ify").default_open(false).show(ui, |ui| {
            ui.label(egui::RichText::new("Make yourself look like part of the game's graphics").weak());
            egui::Grid::new("game_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                plain_row(app, ui, id, "Pixel size", fx.pixelate, 0.0..=32.0, |l, v| l.fx.pixelate = v);
                plain_row(app, ui, id, "Posterize", fx.posterize, 0.0..=32.0, |l, v| l.fx.posterize = v);
            });
        });

        ui.separator();
        let s = &fx.shadow;
        let mut on = s.enabled;
        ui.horizontal(|ui| {
            if ui.checkbox(&mut on, egui::RichText::new("Cast shadow").strong()).changed() {
                let ground = if on { app.find_ground(id) } else { None };
                app.edit("Toggle shadow", |d| {
                    if let Some(l) = d.layer_mut(id) {
                        l.fx.shadow.enabled = on;
                        if let Some(g) = ground {
                            l.fx.shadow.ground = g;
                        }
                    }
                });
            }
            if s.enabled && ui.button("Find feet").on_hover_text("Put the ground line at the lowest solid pixels").clicked()
                && let Some(g) = app.find_ground(id)
            {
                app.edit("Find feet", |d| d.layer_mut(id).map_or((), |l| l.fx.shadow.ground = g));
            }
        });
        if s.enabled {
            egui::Grid::new("shadow_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                anim_row(app, ui, id, "Direction", s.angle.at(t), -180.0..=180.0, |l| &mut l.fx.shadow.angle);
                anim_row(app, ui, id, "Length", s.length.at(t), 0.0..=3.0, |l| &mut l.fx.shadow.length);
                anim_row(app, ui, id, "Softness", s.softness.at(t), 0.0..=80.0, |l| &mut l.fx.shadow.softness);
                anim_row(app, ui, id, "Opacity", s.opacity.at(t), 0.0..=1.0, |l| &mut l.fx.shadow.opacity);
                anim_row(app, ui, id, "Contact", s.contact.at(t), 0.0..=1.0, |l| &mut l.fx.shadow.contact);
                plain_row(app, ui, id, "Ground line", s.ground, 0.5..=1.2, |l, v| l.fx.shadow.ground = v);
                colour_row(app, ui, id, "Shadow tint", s.color, |l, v| l.fx.shadow.color = v);
            });
        }

        ui.separator();
        let r = &fx.relight;
        ui.horizontal(|ui| {
            let mut on = r.enabled;
            if ui.checkbox(&mut on, egui::RichText::new("Relight").strong()).changed() {
                app.edit("Toggle relight", |d| d.layer_mut(id).map_or((), |l| l.fx.relight.enabled = on));
            }
            let label = if layer.has_normals { "Re-estimate normals (AI)" } else { "Estimate normals (AI)" };
            if ui.add_enabled(!app.ai_running(), egui::Button::new(label)).clicked()
                && let Err(e) = app.start_ai(crate::ai::AiTask::Normals)
            {
                app.status = format!("AI: {e:#}");
            }
        });
        if r.enabled {
            if !layer.has_normals {
                ui.label(egui::RichText::new("No normal map yet: lighting is flat until you estimate normals.").small().color(egui::Color32::from_rgb(255, 196, 64)));
            }
            ui.horizontal(|ui| {
                light_pad(app, ui, id, r.azimuth.at(t), r.elevation.at(t));
                ui.vertical(|ui| {
                    egui::Grid::new("relight_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                        anim_row(app, ui, id, "Intensity", r.intensity.at(t), 0.0..=3.0, |l| &mut l.fx.relight.intensity);
                        anim_row(app, ui, id, "Ambient", r.ambient.at(t), 0.0..=1.5, |l| &mut l.fx.relight.ambient);
                        anim_row(app, ui, id, "Rim", r.rim.at(t), 0.0..=2.0, |l| &mut l.fx.relight.rim);
                        anim_row(app, ui, id, "Amount", r.amount.at(t), 0.0..=1.0, |l| &mut l.fx.relight.amount);
                    });
                });
            });
            egui::Grid::new("relight_grid2").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                plain_row(app, ui, id, "Skin softness", r.wrap, 0.0..=1.0, |l, v| l.fx.relight.wrap = v);
                colour_row(app, ui, id, "Light colour", r.color, |l, v| l.fx.relight.color = v);
                colour_row(app, ui, id, "Rim colour", r.rim_color, |l, v| l.fx.relight.rim_color = v);
            });
        }
    });
}
