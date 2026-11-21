//! Inspector section "Replace window / screen": four plain steps (glass, view, look, in front).

use crate::app::App;
use crate::ui::roto::{RotoTarget, shape_list};
use ph_core::{Animated, Glass, Layer, LayerContent, LayerId};

fn end_gesture_on(app: &mut App, r: &egui::Response) {
    if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus()) {
        app.end_gesture();
    }
}

fn glass_slider(app: &mut App, ui: &mut egui::Ui, id: LayerId, label: &str, value: f32, range: std::ops::RangeInclusive<f32>, pick: fn(&mut Glass) -> &mut Animated<f32>) {
    ui.label(label);
    let mut v = value;
    let r = ui.add(egui::Slider::new(&mut v, range).max_decimals(2));
    if r.changed() {
        let (t, auto) = (app.time, app.auto_key);
        if let Some(g) = app.gesture(label).layer_mut(id).and_then(|l| l.glass.as_mut()) {
            pick(g).set(t, v, auto);
        }
    }
    end_gesture_on(app, &r);
    ui.end_row();
}

fn step(ui: &mut egui::Ui, n: u32, title: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(format!("{n}. {title}")).strong());
}

/// Inspector entry: a short description and a button that opens the panel.
pub fn section(app: &mut App, ui: &mut egui::Ui, layer: &Layer) {
    if !matches!(layer.content, LayerContent::Media { .. }) {
        return;
    }
    egui::CollapsingHeader::new("Replace window / screen").default_open(false).show(ui, |ui| {
        ui.label(egui::RichText::new("Show another clip through a window, TV or phone screen.").weak());
        if ui.button("Open panel").clicked() {
            app.glass_setup(layer.id);
            app.glass_panel = true;
        }
    });
}

/// The floating "Replace window / screen" panel for the selected layer.
pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.glass_panel {
        return;
    }
    let Some(layer) = app.selected_layer().cloned() else { return };
    let mut open = true;
    egui::Window::new("Replace window / screen")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(340.0)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 60.0))
        .show(ctx, |ui| panel(app, ui, &layer));
    if !open {
        app.glass_panel = false;
    }
}

fn panel(app: &mut App, ui: &mut egui::Ui, layer: &Layer) {
    let id = layer.id;
    let t = app.time;
    {
        let Some(g) = layer.glass.clone().filter(|g| g.enabled) else {
            ui.label(egui::RichText::new("Show another clip through a window, TV or phone screen. Reflections on the glass and anything in front of it stay.").weak());
            if ui.button("Set up").clicked() {
                app.glass_setup(id);
            }
            return;
        };
        let busy = app.ai_running();

        step(ui, 1, "Glass");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut app.ai.glass_prompt).desired_width(140.0).hint_text("window pane, tv screen…"));
            if ui
                .add_enabled(!busy, egui::Button::new("Find with AI"))
                .on_hover_text("SAM 3 finds the glass at the current frame and turns each pane into an editable shape")
                .clicked()
            {
                let prompt = app.ai.glass_prompt.trim().to_owned();
                if let Err(e) = app.start_ai(crate::ai::AiTask::FindGlass(if prompt.is_empty() { "window pane".into() } else { prompt })) {
                    app.status = format!("AI: {e:#}");
                }
            }
        });
        shape_list(app, ui, layer, RotoTarget::Glass);
        ui.label(egui::RichText::new("Set a shape to Subtract to keep something in front of the glass.").small().weak());

        step(ui, 2, "What you see through it");
        let view = g.view.and_then(|m| app.doc.media(m)).map(|m| m.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
        ui.horizontal(|ui| {
            if ui.button(if view.is_some() { "Change…" } else { "Choose clip or image…" }).clicked()
                && let Some(p) = app.dialog(|| rfd::FileDialog::new().add_filter("Images & video", crate::ui::menu::MEDIA_EXTS).pick_file())
                && let Err(e) = app.glass_set_view(id, &p)
            {
                app.status = format!("Could not use that file: {e:#}");
            }
            if let Some(name) = &view {
                ui.label(egui::RichText::new(name).weak());
            }
        });
        if view.is_some() {
            ui.horizontal(|ui| {
                if ui.button("Fit to glass").on_hover_text("Scale and centre the view to cover every pane").clicked() {
                    app.glass_fit_view(id);
                }
                let mut lp = g.loop_view;
                if ui.checkbox(&mut lp, "Loop").changed() {
                    app.edit("Loop view", |d| d.layer_mut(id).and_then(|l| l.glass.as_mut()).map_or((), |g| g.loop_view = lp));
                }
            });
            let vt = &g.view_transform;
            let (mut pos, mut sc) = (vt.position.at(t), vt.scale.at(t)[0] * 100.0);
            ui.horizontal(|ui| {
                ui.label("Position");
                let rx = ui.add(egui::DragValue::new(&mut pos[0]).speed(2.0).prefix("x "));
                let ry = ui.add(egui::DragValue::new(&mut pos[1]).speed(2.0).prefix("y "));
                ui.label("Scale");
                let rs = ui.add(egui::DragValue::new(&mut sc).speed(0.5).range(1.0..=2000.0).suffix(" %"));
                if rx.changed() || ry.changed() || rs.changed() {
                    let auto = app.auto_key;
                    if let Some(g) = app.gesture("Move view").layer_mut(id).and_then(|l| l.glass.as_mut()) {
                        g.view_transform.position.set(t, pos, auto);
                        g.view_transform.scale.set(t, [sc / 100.0; 2], auto);
                    }
                }
                for r in [rx, ry, rs] {
                    end_gesture_on(app, &r);
                }
            });
        }

        step(ui, 3, "Glass and camera look");
        if ui
            .add_enabled(g.view.is_some(), egui::Button::new("Match the camera"))
            .on_hover_text("Measure how soft and noisy the shot is and make the view the same, so it doesn't look too clean")
            .clicked()
        {
            app.status = app.glass_match_camera(id).unwrap_or_else(|e| format!("Match the camera: {e:#}"));
        }
        egui::Grid::new("glass_look").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
            glass_slider(app, ui, id, "Reflections", g.reflection.at(t), 0.0..=1.0, |g| &mut g.reflection);
            glass_slider(app, ui, id, "Out of focus", g.defocus.at(t), 0.0..=40.0, |g| &mut g.defocus);
            glass_slider(app, ui, id, "Haze / dirt", g.haze.at(t), 0.0..=1.0, |g| &mut g.haze);
            glass_slider(app, ui, id, "Grain", g.grain.at(t), 0.0..=0.15, |g| &mut g.grain);
            glass_slider(app, ui, id, "Glow", g.bloom.at(t), 0.0..=2.0, |g| &mut g.bloom);
            glass_slider(app, ui, id, "Brightness", g.brightness.at(t), 0.0..=2.0, |g| &mut g.brightness);
            ui.label("Tint");
            let mut rgba = egui::Rgba::from_rgb(g.tint[0], g.tint[1], g.tint[2]);
            if egui::widgets::color_picker::color_edit_button_rgba(ui, &mut rgba, egui::color_picker::Alpha::Opaque).changed() {
                let tint = [rgba.r(), rgba.g(), rgba.b()];
                if let Some(g) = app.gesture("Glass tint").layer_mut(id).and_then(|l| l.glass.as_mut()) {
                    g.tint = tint;
                }
            }
            ui.end_row();
        });

        let is_video = layer_is_video(app, layer);
        step(ui, 4, "Keep in front");
        if is_video {
            ui.label(egui::RichText::new("Something moving between you and the glass (a laptop, your hand)? Draw a box around it and AI follows it through the clip.").small().weak());
            if ui.add_enabled(!busy, egui::Button::new("Draw a box around it")).clicked() {
                app.ai.picking_box = true;
                app.ai.box_for_glass = true;
                app.tool = crate::app::Tool::Move;
                app.status = "Drag a box around the object in front of the glass.".into();
            }
        } else {
            ui.label(egui::RichText::new("For a photo, add a pane shape set to Subtract around it.").small().weak());
        }
        let mut remove = None;
        for (i, _) in g.front.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("Tracked object {}", i + 1));
                if ui.small_button(egui_material_icons::icons::ICON_DELETE.rich_text()).on_hover_text("Stop keeping it in front").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            app.edit("Remove tracked object", |d| {
                if let Some(g) = d.layer_mut(id).and_then(|l| l.glass.as_mut())
                    && i < g.front.len()
                {
                    g.front.remove(i);
                }
            });
        }

        ui.add_space(6.0);
        if ui.button("Turn off").on_hover_text("Settings are kept; Set up turns it back on").clicked() {
            app.edit("Turn off window replacement", |d| d.layer_mut(id).and_then(|l| l.glass.as_mut()).map_or((), |g| g.enabled = false));
        }
        if g.view.is_none() || !g.has_glass() {
            ui.label(
                egui::RichText::new(match (g.has_glass(), g.view.is_some()) {
                    (false, _) => "Next: find or draw the glass.",
                    (true, false) => "Next: choose what to see through it.",
                    _ => "",
                })
                .color(egui::Color32::from_rgb(255, 196, 64)),
            );
        }
    }
}

fn layer_is_video(app: &App, layer: &Layer) -> bool {
    match &layer.content {
        LayerContent::Media { media, .. } => app.doc.media(*media).is_some_and(|m| m.is_video()),
        _ => false,
    }
}
