//! Layer stack panel (top of the list = top of the stack).

use crate::app::App;
use egui_material_icons::icons::*;
use egui_material_icons::MaterialIcon;
use ph_core::doc::LayerContent;

fn icon_btn(ui: &mut egui::Ui, icon: MaterialIcon, tip: &str) -> egui::Response {
    ui.add(egui::Button::new(icon.rich_text().size(16.0)).frame(false)).on_hover_text(tip)
}

pub fn layers_panel(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Layers");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_btn(ui, ICON_DELETE, "Delete layer").clicked() {
                app.delete_selected();
            }
            if icon_btn(ui, ICON_CONTENT_COPY, "Duplicate layer").clicked() {
                app.duplicate_selected();
            }
            if icon_btn(ui, ICON_SQUARE, "New colour layer").clicked() {
                app.add_solid_layer();
            }
            if icon_btn(ui, ICON_BRUSH, "New paint layer").clicked() {
                app.add_paint_layer();
            }
            if icon_btn(ui, ICON_ADD_PHOTO_ALTERNATE, "Import photo / video").clicked() {
                crate::ui::menu::import_dialog(app);
            }
        });
    });
    ui.separator();
    if app.doc.layers.is_empty() {
        ui.label("No layers yet.\nDrop a photo or video onto the window, or use the import button above.");
        return;
    }
    let ids: Vec<_> = app.doc.layers.iter().rev().map(|l| l.id).collect();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for id in ids {
            let Some(layer) = app.doc.layer(id) else { continue };
            let (visible, locked, name) = (layer.visible, layer.locked, layer.name.clone());
            let kind = match &layer.content {
                LayerContent::Media { media, .. } => {
                    if app.doc.media(*media).is_some_and(|m| m.is_video()) { ICON_MOVIE } else { ICON_IMAGE }
                }
                LayerContent::Paint { .. } => ICON_BRUSH,
                LayerContent::Solid { .. } => ICON_SQUARE,
            };
            let media_err = match &layer.content {
                LayerContent::Media { media, .. } => app.media.error(*media).map(str::to_owned),
                _ => None,
            };
            let selected = app.selected == Some(id);
            ui.horizontal(|ui| {
                if icon_btn(ui, if visible { ICON_VISIBILITY } else { ICON_VISIBILITY_OFF }, "Show / hide").clicked() {
                    app.edit("Toggle visibility", |d| {
                        if let Some(l) = d.layer_mut(id) {
                            l.visible = !l.visible;
                        }
                    });
                }
                if icon_btn(ui, if locked { ICON_LOCK } else { ICON_LOCK_OPEN }, "Lock").clicked() {
                    app.edit("Toggle lock", |d| {
                        if let Some(l) = d.layer_mut(id) {
                            l.locked = !l.locked;
                        }
                    });
                }
                ui.label(kind.rich_text().size(14.0));
                let label = egui::RichText::new(name);
                let label = if media_err.is_some() { label.color(egui::Color32::LIGHT_RED) } else { label };
                let r = ui.selectable_label(selected, label);
                let r = match &media_err {
                    Some(e) => r.on_hover_text(e),
                    None => r,
                };
                if r.clicked() {
                    app.selected = Some(id);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_btn(ui, ICON_ARROW_DOWNWARD, "Move down").clicked() {
                        app.move_layer(id, -1);
                    }
                    if icon_btn(ui, ICON_ARROW_UPWARD, "Move up").clicked() {
                        app.move_layer(id, 1);
                    }
                });
            });
        }
    });
}
