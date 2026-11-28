//! Inspector section "Cut out": remove the background (AI), select by description, paint or draw
//! the cut-out by hand, and split a picture with several objects into separate layers.

use super::widgets::{ACCENT, big_button, hint, section};
use crate::app::{App, Tool};
use ph_core::Layer;
use ph_core::doc::LayerContent;

pub fn cutout_section(app: &mut App, ui: &mut egui::Ui, layer: &Layer, step: u8) {
    let id = layer.id;
    let has_cut = layer.mask.is_some() || !layer.roto.is_empty();
    let is_video = match &layer.content {
        LayerContent::Media { media, .. } => app.doc.media(*media).is_some_and(ph_core::MediaRef::is_video),
        _ => false,
    };
    let what = "Keep only the person or object and hide the rest of this photo or video.";
    section(app, ui, step, "Cut out", what, "cutout", !has_cut, |app, ui| {
        let busy = app.ai_running();
        let (task, label, tip) = if is_video {
            (crate::ai::AiTask::VideoCutout, "Remove background (AI, whole video)", "AI finds the main subject in every frame and hides everything else. Takes a while for long clips.")
        } else {
            (crate::ai::AiTask::Matte, "Remove background (AI)", "AI finds the main subject and hides everything else. You can fix the edges afterwards with the brush or eraser.")
        };
        if big_button(ui, !busy, label, tip)
            && let Err(e) = app.start_ai(task)
        {
            app.status = format!("AI: {e:#}");
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Keep only… (describe it)")).on_hover_text("Type what to keep, e.g. \"person\", \"dog\", \"toy gun\", and AI selects just that.").clicked() {
                app.ai.show_dialog = true;
            }
            if busy {
                ui.spinner();
            }
        });

        ui.add_space(4.0);
        ui.label(egui::RichText::new("Fix it by hand").strong());
        match &layer.mask {
            None => {
                if ui.button("Paint the cut-out").on_hover_text("Adds a mask you paint on: Brush hides, Eraser brings back.").clicked() {
                    app.add_mask();
                    app.tool = Tool::Brush;
                }
            }
            Some(m) => {
                let (mut enabled, mut invert) = (m.enabled, m.invert);
                if m.video.is_some() {
                    hint(ui, "This cut-out comes from AI video tracking (one mask per frame).");
                }
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut enabled, "Cut-out on").on_hover_text("Turn off to see the whole picture again").changed() {
                        app.edit("Toggle mask", |d| d.layer_mut(id).and_then(|l| l.mask.as_mut()).map_or((), |m| m.enabled = enabled));
                    }
                    if ui.checkbox(&mut invert, "Invert").on_hover_text("Keep the outside instead (e.g. cut a hole)").changed() {
                        app.edit("Invert mask", |d| d.layer_mut(id).and_then(|l| l.mask.as_mut()).map_or((), |m| m.invert = invert));
                    }
                });
                let mut painting = app.edit_mask;
                if ui.checkbox(&mut painting, "Paint on the cut-out (Brush hides, Eraser brings back)").changed() {
                    app.edit_mask = painting;
                    if painting && !matches!(app.tool, Tool::Brush | Tool::Eraser) {
                        app.tool = Tool::Brush;
                    }
                }
                if app.edit_mask {
                    ui.colored_label(ACCENT, "Painting the cut-out, not the picture.");
                }
                if ui.small_button("Remove the cut-out").on_hover_text("Deletes the mask: the whole picture shows again").clicked() {
                    app.delete_mask();
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Select an area (M)").on_hover_text("Rectangle, ellipse, lasso or magic wand: the selection becomes the cut-out").clicked() {
                app.tool = Tool::Select;
            }
        });

        egui::CollapsingHeader::new(format!("Pen shapes ({})", layer.roto.len())).id_salt("pen_shapes").default_open(!layer.roto.is_empty()).show(ui, |ui| {
            hint(ui, "Draw exact outlines with the Pen (P). Add keeps the inside, Subtract cuts it away. On video, key the shape (◆) at a few frames so it follows.");
            crate::ui::roto::shape_list(app, ui, layer, crate::ui::roto::RotoTarget::Mask);
        });

        if !is_video && matches!(layer.content, LayerContent::Media { .. }) {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Several objects in one picture?").strong());
            if ui
                .button("Split into separate objects")
                .on_hover_text("One layer per object (e.g. three figures in one render), each with its own feet, shadow and colour match. The original layer is kept, hidden.")
                .clicked()
            {
                match app.split_objects(id) {
                    Ok(_) => {}
                    Err(e) => app.status = format!("Split: {e:#}"),
                }
            }
        }
    });
}
