//! Shared inspector building blocks: numbered sections with a help link, and slider rows that
//! edit a layer as one undoable gesture.

use crate::app::App;
use ph_core::{Animated, Layer, LayerId};

pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(255, 196, 64);

/// Finish the undo gesture when a widget is released.
pub fn track(app: &mut App, r: &egui::Response) {
    if r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus()) {
        app.end_gesture();
    }
}

#[allow(clippy::too_many_arguments)]
/// A collapsible inspector section: `step` number (0 = none), plain title, one-line "what it's
/// for" and a link to its page in the manual.
pub fn section(app: &mut App, ui: &mut egui::Ui, step: u8, title: &str, what: &str, help: &str, default_open: bool, body: impl FnOnce(&mut App, &mut egui::Ui)) {
    let head = if step > 0 { format!("{step}  {title}") } else { title.to_owned() };
    let r = egui::CollapsingHeader::new(egui::RichText::new(head).strong().size(14.5)).id_salt(("section", title)).default_open(default_open).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(what).weak().small());
            if ui.small_button(egui::RichText::new("? Help").small()).on_hover_text("Open this topic in the manual (F1)").clicked() {
                app.help.open_page(help);
            }
        });
        ui.add_space(2.0);
        body(app, ui);
        ui.add_space(4.0);
    });
    r.header_response.on_hover_text(what);
}

/// Small grey hint text that wraps.
pub fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).weak().small()).wrap());
}

/// Slider row for a keyframable scalar inside the layer (inside a 2-column grid).
#[allow(clippy::too_many_arguments)]
pub fn anim_row(
    app: &mut App,
    ui: &mut egui::Ui,
    id: LayerId,
    label: &str,
    tip: &str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    pick: fn(&mut Layer) -> &mut Animated<f32>,
) {
    ui.label(label).on_hover_text(tip);
    let mut v = value;
    let r = ui.add(egui::Slider::new(&mut v, range).max_decimals(2)).on_hover_text(tip);
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
#[allow(clippy::too_many_arguments)]
pub fn plain_row(app: &mut App, ui: &mut egui::Ui, id: LayerId, label: &str, tip: &str, value: f32, range: std::ops::RangeInclusive<f32>, set: fn(&mut Layer, f32)) {
    ui.label(label).on_hover_text(tip);
    let mut v = value;
    let r = ui.add(egui::Slider::new(&mut v, range).max_decimals(2)).on_hover_text(tip);
    if r.changed()
        && let Some(l) = app.gesture(label).layer_mut(id)
    {
        set(l, v);
    }
    track(app, &r);
    ui.end_row();
}

pub fn colour_row(app: &mut App, ui: &mut egui::Ui, id: LayerId, label: &str, value: [f32; 3], set: fn(&mut Layer, [f32; 3])) {
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

/// A wide, prominent action button.
pub fn big_button(ui: &mut egui::Ui, enabled: bool, text: &str, tip: &str) -> bool {
    ui.add_enabled(enabled, egui::Button::new(egui::RichText::new(text).strong()).min_size(egui::vec2(ui.available_width(), 26.0))).on_hover_text(tip).clicked()
}
