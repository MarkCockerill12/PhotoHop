//! Inspector section "Fit into the scene": one-click fit, colour match (smart or AI) with a
//! strength slider that moves the colour sliders between the original and the matched look.

use super::widgets::{big_button, hint, section, track};
use crate::app::App;
use ph_core::look::MatchMethod;
use ph_core::{Layer, LayerId};

pub fn fit_section(app: &mut App, ui: &mut egui::Ui, layer: &Layer, step: u8) {
    let id = layer.id;
    let has_below = app.doc.layer_index(id).is_some_and(|i| i > 0);
    section(
        app,
        ui,
        step,
        "Fit into the scene",
        "Make this layer look like it belongs: colours, shadow and film grain matched to the background.",
        "fit",
        true,
        |app, ui| {
            if !has_below && app.match_reference.is_none() {
                ui.colored_label(super::widgets::ACCENT, "Move this layer above a background first (arrows in the Layers list).");
            }
            if big_button(ui, !app.ai_running(), &format!("{}  One-click fit", egui_material_icons::icons::ICON_AUTO_FIX_HIGH.codepoint), "Matches colour to the scene, adds a shadow on the ground (same sun as other shadows in the scene) and copies the background's grain. Undo with Ctrl+Z.") {
                app.status = app.auto_fit(id).unwrap_or_else(|e| format!("Fit failed: {e:#}"));
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Colour match").strong());
            ui.horizontal(|ui| {
                if ui
                    .button("Match colours")
                    .on_hover_text("Instant, no AI. Matches the light colour, black/white levels and saturation of the scene around this layer. Keeps the object's own colours: a blue object stays blue.")
                    .clicked()
                {
                    app.status = app.match_to_scene(id).unwrap_or_else(|e| format!("Match failed: {e:#}"));
                }
                if ui
                    .add_enabled(!app.ai_running(), egui::Button::new("Match with AI"))
                    .on_hover_text("An AI (Harmonizer) suggests how this layer should look in the scene; PhotoHop turns that into the same colour sliders. Slower; needs the AI tools installed.")
                    .clicked()
                    && let Err(e) = app.start_ai(crate::ai::AiTask::AutoColour)
                {
                    app.status = format!("AI: {e:#}");
                }
                if app.ai_running() {
                    ui.spinner();
                }
            });
            strength(app, ui, layer);
            reference_picker(app, ui, id);
            legacy_match(app, ui, layer);
        },
    );
}

fn strength(app: &mut App, ui: &mut egui::Ui, layer: &Layer) {
    let id = layer.id;
    let Some(m) = layer.grade.scene_match else {
        hint(ui, "After matching, a Match strength slider appears here: drag it to compare Original ↔ Matched.");
        return;
    };
    let t = app.time;
    ui.horizontal(|ui| {
        ui.label("Original");
        let mut s = m.strength * 100.0;
        let r = ui
            .add(egui::Slider::new(&mut s, 0.0..=100.0).suffix("%").show_value(true))
            .on_hover_text("Match strength: moves the Colour sliders (exposure, temperature, tint, contrast, saturation, shadow level) between your original look and the matched look.");
        ui.label("Matched");
        if r.changed() {
            let auto = app.auto_key;
            if let Some(l) = app.gesture("Match strength").layer_mut(id) {
                l.grade.set_match_strength(s / 100.0, t, auto);
            }
        }
        track(app, &r);
    });
    let how = if m.method == MatchMethod::Ai { "AI match" } else { "Smart match" };
    let changes = crate::scenematch::describe(&m.original, &m.matched);
    hint(ui, &format!("{how}: {changes}. The sliders in Colour adjustments show the result; tweak any of them and the strength slider keeps your tweak."));
    if ui.small_button("Forget the match (keep current sliders)").clicked() {
        app.edit("Forget match", |d| d.layer_mut(id).map_or((), |l| l.grade.scene_match = None));
    }
}

fn reference_picker(app: &mut App, ui: &mut egui::Ui, id: LayerId) {
    let others: Vec<(LayerId, String)> = app.doc.layers.iter().filter(|l| l.id != id).map(|l| (l.id, l.name.clone())).collect();
    let current = app.match_reference.and_then(|r| others.iter().find(|(o, _)| *o == r)).map_or("the scene around this layer".to_owned(), |(_, n)| format!("all of \"{n}\""));
    ui.horizontal(|ui| {
        ui.label("Match to:");
        egui::ComboBox::from_id_salt("match_ref").selected_text(current).width(170.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut app.match_reference, None, "The scene around this layer (best)");
            for (oid, name) in &others {
                ui.selectable_value(&mut app.match_reference, Some(*oid), format!("All of \"{name}\""));
            }
        })
        .response
        .on_hover_text("Where the colours to match come from. Usually the background right around the layer.");
    });
}

/// Projects made before v0.9 may carry the old match (scale/offset + amount); keep it adjustable.
fn legacy_match(app: &mut App, ui: &mut egui::Ui, layer: &Layer) {
    let t = app.time;
    let amount = layer.grade.match_amount.at(t);
    if amount <= 0.0 && !layer.grade.match_amount.is_animated() {
        return;
    }
    let id = layer.id;
    ui.horizontal(|ui| {
        ui.label("Older match").on_hover_text("A colour match made with an older PhotoHop version. Set to 0 and use Match colours instead.");
        let mut v = amount;
        let r = ui.add(egui::Slider::new(&mut v, 0.0..=1.0));
        if r.changed() {
            let auto = app.auto_key;
            if let Some(l) = app.gesture("Older match").layer_mut(id) {
                l.grade.match_amount.set(t, v, auto);
            }
        }
        track(app, &r);
    });
}
