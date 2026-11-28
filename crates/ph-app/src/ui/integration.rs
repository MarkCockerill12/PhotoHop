//! Inspector sections "Shadow & light" (ground shadow, scene shadows on the layer, relight) and
//! "Camera & lens look" (grain, blur, motion blur, haze, edge light, depth of field, bloom) plus
//! "Game look".

use super::widgets::{ACCENT, anim_row, colour_row, hint, plain_row, section};
use crate::app::App;
use ph_core::{Layer, LayerId};

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

pub fn shadow_light_section(app: &mut App, ui: &mut egui::Ui, layer: &Layer, step: u8) {
    let id = layer.id;
    let t = app.time;
    let fx = layer.fx.clone();
    let what = "A shadow on the ground under this layer, shadows from the scene falling on it, and the direction light hits it from.";
    section(app, ui, step, "Shadow & light", what, "shadow", fx.shadow.enabled, |app, ui| {
        if ui
            .button("Aim from the scene's light")
            .on_hover_text("Estimates where the light in the background comes from and its colour, then points the shadow away from it (no AI)")
            .clicked()
        {
            app.status = app.match_light().unwrap_or_else(|e| format!("Match failed: {e:#}"));
        }
        let s = &fx.shadow;
        let mut on = s.enabled;
        ui.horizontal(|ui| {
            if ui.checkbox(&mut on, egui::RichText::new("Shadow on the ground").strong()).on_hover_text("This layer casts a shadow onto the background").changed() {
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
            if s.enabled && ui.button("Find feet").on_hover_text("Put the feet line at the lowest solid part of the layer").clicked()
                && let Some(g) = app.find_ground(id)
            {
                app.edit("Find feet", |d| d.layer_mut(id).map_or((), |l| l.fx.shadow.ground = g));
            }
        });
        if s.enabled {
            hint(ui, "Aim it on the canvas: with the Move tool (V), drag the orange dot to where the shadow of the top of the head should land. Copy a shadow that is already in the scene.");
            egui::Grid::new("shadow_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                anim_row(app, ui, id, "Direction", "Which way the shadow falls, in degrees (0 = right, 90 = towards you)", s.angle.at(t), -180.0..=180.0, |l| &mut l.fx.shadow.angle);
                anim_row(app, ui, id, "Length", "Shadow length compared to the layer's height (long = low sun)", s.length.at(t), 0.0..=3.0, |l| &mut l.fx.shadow.length);
                anim_row(app, ui, id, "Thickness", "Widens a shadow that looks paper-thin when the light comes from the side", s.thickness.at(t), 0.0..=0.5, |l| &mut l.fx.shadow.thickness);
                anim_row(app, ui, id, "Softness", "Blur of the shadow edge in pixels (cloudy day = soft)", s.softness.at(t), 0.0..=80.0, |l| &mut l.fx.shadow.softness);
                anim_row(app, ui, id, "Darkness", "How dark the shadow is", s.opacity.at(t), 0.0..=1.0, |l| &mut l.fx.shadow.opacity);
                anim_row(app, ui, id, "Contact darkness", "The small dark patch right where the feet touch the ground", s.contact.at(t), 0.0..=1.0, |l| &mut l.fx.shadow.contact);
                plain_row(app, ui, id, "Feet line", "Where the layer touches the ground, as a fraction of its height (1 = bottom edge). Find feet sets it.", s.ground, 0.5..=1.2, |l, v| l.fx.shadow.ground = v);
                colour_row(app, ui, id, "Shadow colour", s.color, |l, v| l.fx.shadow.color = v);
            });
            ui.horizontal(|ui| {
                let label = if layer.body_mesh.is_some() { "Rebuild 3D body (AI)" } else { "Body-shaped shadow (AI)" };
                if ui
                    .add_enabled(!app.ai_running(), egui::Button::new(label))
                    .on_hover_text("For people: AI rebuilds the body in 3D from the photo so the shadow has its real shape")
                    .clicked()
                    && let Err(e) = app.start_ai(crate::ai::AiTask::BodyShape)
                {
                    app.status = format!("AI: {e:#}");
                }
                if layer.body_mesh.is_some() {
                    let mut on = s.body_shape;
                    if ui.checkbox(&mut on, "Use 3D body").changed() {
                        app.edit("Body-shaped shadow", |d| d.layer_mut(id).map_or((), |l| l.fx.shadow.body_shape = on));
                    }
                }
            });
        }

        ui.separator();
        let dp = &fx.dapple;
        let mut on = dp.enabled;
        let tip = "Shadows in the scene (sun through leaves, window frames, blinds) fall on this layer too. PhotoHop reads where the ground is sunlit and lays the same pattern on the layer along the shadow direction.";
        if ui.checkbox(&mut on, egui::RichText::new("Scene shadows fall on me").strong()).on_hover_text(tip).changed() {
            let ground = if on && !s.enabled { app.find_ground(id) } else { None };
            app.edit("Shadows on me", |d| {
                if let Some(l) = d.layer_mut(id) {
                    l.fx.dapple.enabled = on;
                    if let Some(g) = ground {
                        l.fx.shadow.ground = g;
                    }
                }
            });
        }
        if dp.enabled {
            hint(ui, "Uses the ground shadow's direction and length: line the ground shadow up with the scene's shadows first (it can stay invisible at darkness 0).");
            egui::Grid::new("dapple_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                anim_row(app, ui, id, "Strength", "How dark the scene's shadows are on the layer", dp.strength.at(t), 0.0..=1.0, |l| &mut l.fx.dapple.strength);
                anim_row(app, ui, id, "Softness", "Blur of the shadow pattern", dp.softness.at(t), 0.0..=30.0, |l| &mut l.fx.dapple.softness);
                plain_row(app, ui, id, "Sun / shade split", "Moves the line between what counts as sunlit and shaded ground", dp.threshold, -1.0..=1.0, |l, v| l.fx.dapple.threshold = v);
            });
            let mut follow = dp.shadow_follows;
            if ui.checkbox(&mut follow, "My ground shadow only darkens sunlit ground").changed() {
                app.edit("Shadow follows light", |d| d.layer_mut(id).map_or((), |l| l.fx.dapple.shadow_follows = follow));
            }
        }

        ui.separator();
        let r = &fx.relight;
        let mut on = r.enabled;
        ui.horizontal(|ui| {
            if ui.checkbox(&mut on, egui::RichText::new("Relight").strong()).on_hover_text("Change which side of this layer is lit, to match the scene's sun").changed() {
                app.edit("Toggle relight", |d| d.layer_mut(id).map_or((), |l| l.fx.relight.enabled = on));
            }
            let label = if layer.has_normals { "Re-analyse shape (AI)" } else { "Analyse shape (AI)" };
            if ui
                .add_enabled(!app.ai_running(), egui::Button::new(label))
                .on_hover_text("For people: AI works out which way each part of the body faces (a normal map), so light can wrap around it")
                .clicked()
                && let Err(e) = app.start_ai(crate::ai::AiTask::Normals)
            {
                app.status = format!("AI: {e:#}");
            }
        });
        if r.enabled {
            if !layer.has_normals {
                ui.colored_label(ACCENT, "Click Analyse shape first: without it the light is flat.");
            }
            ui.horizontal(|ui| {
                light_pad(app, ui, id, r.azimuth.at(t), r.elevation.at(t));
                ui.vertical(|ui| {
                    egui::Grid::new("relight_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                        anim_row(app, ui, id, "Brightness", "Strength of the light", r.intensity.at(t), 0.0..=3.0, |l| &mut l.fx.relight.intensity);
                        anim_row(app, ui, id, "Fill light", "Light on the side facing away (0 = black shadows)", r.ambient.at(t), 0.0..=1.5, |l| &mut l.fx.relight.ambient);
                        anim_row(app, ui, id, "Rim light", "A bright edge from behind", r.rim.at(t), 0.0..=2.0, |l| &mut l.fx.relight.rim);
                        anim_row(app, ui, id, "Amount", "Blend between the original light (0) and the new light (1)", r.amount.at(t), 0.0..=1.0, |l| &mut l.fx.relight.amount);
                    });
                });
            });
            hint(ui, "Drag the sun on the pad: its direction is where the light comes from, the centre means straight from the camera.");
            egui::Grid::new("relight_grid2").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                plain_row(app, ui, id, "Skin softness", "How softly light wraps around skin", r.wrap, 0.0..=1.0, |l, v| l.fx.relight.wrap = v);
                colour_row(app, ui, id, "Light colour", r.color, |l, v| l.fx.relight.color = v);
                colour_row(app, ui, id, "Rim colour", r.rim_color, |l, v| l.fx.relight.rim_color = v);
            });
        }
    });
}

pub fn camera_section(app: &mut App, ui: &mut egui::Ui, layer: &Layer, step: u8) {
    let id = layer.id;
    let t = app.time;
    let fx = layer.fx.clone();
    let what = "Make this layer look shot with the same camera as the background: grain, softness, motion blur, haze, glow.";
    section(app, ui, step, "Camera & lens look", what, "camera", false, |app, ui| {
        if ui
            .button("Copy the camera's blur & grain")
            .on_hover_text("Measures the grain, softness and motion blur of the background around this layer and copies them (no AI)")
            .clicked()
        {
            app.status = app.match_detail().unwrap_or_else(|e| format!("Match failed: {e:#}"));
        }
        egui::Grid::new("fx_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
            anim_row(app, ui, id, "Grain", "Film / sensor noise", fx.grain.at(t), 0.0..=0.15, |l| &mut l.fx.grain);
            plain_row(app, ui, id, "Grain size", "Size of the grain specks", fx.grain_size, 0.5..=6.0, |l, v| l.fx.grain_size = v);
            anim_row(app, ui, id, "Blur", "Soften the layer (pixels)", fx.blur.at(t), 0.0..=40.0, |l| &mut l.fx.blur);
            anim_row(app, ui, id, "Sharpen", "Crisper edges", fx.sharpen.at(t), 0.0..=2.0, |l| &mut l.fx.sharpen);
            anim_row(app, ui, id, "Motion blur", "Streak length in pixels, like a moving camera", fx.motion_blur.at(t), 0.0..=120.0, |l| &mut l.fx.motion_blur);
            plain_row(app, ui, id, "Motion direction", "Angle of the streak in degrees", fx.motion_angle, -180.0..=180.0, |l, v| l.fx.motion_angle = v);
            anim_row(app, ui, id, "Haze", "Distance fog: far-away things look paler", fx.haze.at(t), 0.0..=1.0, |l| &mut l.fx.haze);
            colour_row(app, ui, id, "Haze colour", fx.haze_color, |l, v| l.fx.haze_color = v);
            anim_row(app, ui, id, "Edge light", "Light from the background spilling over the layer's edges (light wrap)", fx.light_wrap.at(t), 0.0..=1.0, |l| &mut l.fx.light_wrap);
            plain_row(app, ui, id, "Edge light width", "How far the edge light reaches in (pixels)", fx.light_wrap_width, 1.0..=120.0, |l, v| l.fx.light_wrap_width = v);
        });
        let mut mono = fx.grain_mono;
        if ui.checkbox(&mut mono, "Black-and-white grain").changed() {
            app.edit("Grain colour", |d| d.layer_mut(id).map_or((), |l| l.fx.grain_mono = mono));
        }
        let mut follow = fx.follow_scene_focus;
        if ui
            .checkbox(&mut follow, "Blur me like the background's depth of field")
            .on_hover_text("If the background has depth of field turned on, this layer gets the same blur as the ground where it stands")
            .changed()
        {
            app.edit("Follow scene focus", |d| d.layer_mut(id).map_or((), |l| l.fx.follow_scene_focus = follow));
        }

        egui::CollapsingHeader::new("Lens: depth of field, bloom, colour fringing").id_salt("lens").default_open(fx.lens.is_active(t)).show(ui, |ui| {
            hint(ui, "Mostly for backgrounds that look too clean (game screenshots, 3D renders).");
            let lens = &fx.lens;
            ui.horizontal(|ui| {
                let mut on = lens.dof;
                if ui.checkbox(&mut on, egui::RichText::new("Depth of field").strong()).on_hover_text("Blur what's far from the focus, like a real lens").changed() {
                    app.edit("Depth of field", |d| d.layer_mut(id).map_or((), |l| l.fx.lens.dof = on));
                }
                let label = if layer.has_depth { "Re-measure depth (AI)" } else { "Measure depth (AI)" };
                if ui.add_enabled(!app.ai_running(), egui::Button::new(label)).on_hover_text("AI estimates how far away each pixel is").clicked()
                    && let Err(e) = app.start_ai(crate::ai::AiTask::Depth)
                {
                    app.status = format!("AI: {e:#}");
                }
            });
            if lens.dof {
                if !layer.has_depth {
                    ui.colored_label(ACCENT, "Needs a depth map: click Measure depth.");
                } else if ui.button("Pick focus").on_hover_text("Then click the thing that should be sharp").clicked() {
                    app.picking_focus = Some(id);
                    app.status = "Click what should be in focus.".into();
                }
                egui::Grid::new("dof_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                    anim_row(app, ui, id, "Blur amount", "Maximum blur for things far from the focus", lens.dof_amount.at(t), 0.0..=60.0, |l| &mut l.fx.lens.dof_amount);
                    anim_row(app, ui, id, "Focus distance", "0 = near, 1 = far (Pick focus sets it)", lens.focus.at(t), 0.0..=1.0, |l| &mut l.fx.lens.focus);
                    plain_row(app, ui, id, "Sharp range", "How much around the focus stays sharp", lens.focus_range, 0.0..=0.5, |l, v| l.fx.lens.focus_range = v);
                });
            }
            egui::Grid::new("lens_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                anim_row(app, ui, id, "Bloom", "Glow around bright lights", lens.bloom.at(t), 0.0..=2.0, |l| &mut l.fx.lens.bloom);
                plain_row(app, ui, id, "Bloom size", "How far the glow spreads (pixels)", lens.bloom_size, 2.0..=150.0, |l, v| l.fx.lens.bloom_size = v);
                plain_row(app, ui, id, "Bloom from", "Only things brighter than this glow", lens.bloom_threshold, 0.0..=4.0, |l, v| l.fx.lens.bloom_threshold = v);
                anim_row(app, ui, id, "Colour fringing", "Red/blue edges like a cheap lens (chromatic aberration)", lens.chroma.at(t), 0.0..=12.0, |l| &mut l.fx.lens.chroma);
            });
        });
    });
}

pub fn game_section(app: &mut App, ui: &mut egui::Ui, layer: &Layer) {
    let id = layer.id;
    let fx = layer.fx.clone();
    let active = fx.pixelate > 0.0 || fx.posterize > 0.0;
    section(app, ui, 0, "Game look", "Make a real photo look like low-resolution game graphics.", "game", active, |app, ui| {
        egui::Grid::new("game_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
            plain_row(app, ui, id, "Pixel size", "Bigger, blockier pixels (0 = off)", fx.pixelate, 0.0..=32.0, |l, v| l.fx.pixelate = v);
            plain_row(app, ui, id, "Colour steps", "Fewer colour levels, like old games (posterize; 0 = off)", fx.posterize, 0.0..=32.0, |l, v| l.fx.posterize = v);
        });
    });
}
