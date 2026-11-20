//! Timeline: transport, ruler with playhead, one row per layer with its clip bar and keyframes.

use crate::app::App;
use egui_material_icons::icons::*;
use ph_core::doc::LayerContent;

const ROW_H: f32 = 22.0;
const NAME_W: f32 = 130.0;
const RULER_H: f32 = 20.0;

fn timecode(t: f64, fps: f64) -> String {
    let total = (t * fps).round() as u64;
    let f = (fps.round() as u64).max(1);
    let (frames, secs) = (total % f, total / f);
    format!("{:02}:{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60, frames)
}

pub fn timeline(app: &mut App, ui: &mut egui::Ui) {
    let fps = app.doc.canvas.fps;
    let dur = app.doc.canvas.duration.max(1.0 / fps);
    ui.horizontal(|ui| {
        if ui.button(ICON_SKIP_PREVIOUS.rich_text()).on_hover_text("Start (Home)").clicked() {
            app.set_time(0.0);
        }
        if ui.button(ICON_NAVIGATE_BEFORE.rich_text()).on_hover_text("Previous frame (Left arrow)").clicked() {
            app.set_time(app.time - 1.0 / fps);
        }
        if ui.button(if app.playing { ICON_PAUSE } else { ICON_PLAY_ARROW }.rich_text()).on_hover_text("Play / pause (Space)").clicked() {
            app.toggle_play();
        }
        if ui.button(ICON_NAVIGATE_NEXT.rich_text()).on_hover_text("Next frame (Right arrow)").clicked() {
            app.set_time(app.time + 1.0 / fps);
        }
        ui.monospace(format!("{} / {}", timecode(app.time, fps), timecode(dur, fps)));
        ui.label(format!("{fps:.3} fps"));
        if ui.button(ICON_DIAMOND.rich_text()).on_hover_text("Toggle keyframes on the selected layer (K)").clicked() {
            crate::ui::inspector::toggle_all_keys(app);
        }
    });

    let avail = ui.available_size();
    let rows = app.doc.layers.len().max(1) as f32;
    let height = (RULER_H + rows * ROW_H).max(avail.y);
    egui::ScrollArea::vertical().show(ui, |ui| {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(avail.x, height), egui::Sense::click_and_drag());
        let p = ui.painter_at(rect);
        let track = egui::Rect::from_min_max(egui::pos2(rect.left() + NAME_W, rect.top()), rect.right_bottom());
        let x_of = |t: f64| track.left() + (t / dur) as f32 * track.width();
        let t_of = |x: f32| ((x - track.left()) / track.width()) as f64 * dur;

        // Ruler.
        let ruler = egui::Rect::from_min_size(track.min, egui::vec2(track.width(), RULER_H));
        p.rect_filled(ruler, 0.0, egui::Color32::from_gray(40));
        let step = [0.04, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0]
            .into_iter()
            .find(|s| (track.width() as f64 / (dur / s)) > 60.0)
            .unwrap_or(60.0);
        let mut tick = 0.0;
        while tick <= dur + 1e-9 {
            let x = x_of(tick);
            p.line_segment([egui::pos2(x, ruler.bottom() - 6.0), egui::pos2(x, ruler.bottom())], (1.0, egui::Color32::GRAY));
            p.text(egui::pos2(x + 2.0, ruler.top() + 2.0), egui::Align2::LEFT_TOP, format!("{tick:.1}s"), egui::FontId::monospace(10.0), egui::Color32::GRAY);
            tick += step;
        }

        // Layer rows, top of stack first.
        let ids: Vec<_> = app.doc.layers.iter().rev().map(|l| l.id).collect();
        for (row, id) in ids.iter().enumerate() {
            let Some(layer) = app.doc.layer(*id) else { continue };
            let y = rect.top() + RULER_H + row as f32 * ROW_H;
            let row_rect = egui::Rect::from_min_size(egui::pos2(rect.left(), y), egui::vec2(rect.width(), ROW_H));
            let selected = app.selected == Some(*id);
            if selected {
                p.rect_filled(row_rect, 0.0, egui::Color32::from_rgba_unmultiplied(80, 120, 200, 40));
            }
            p.text(egui::pos2(rect.left() + 4.0, y + ROW_H * 0.5), egui::Align2::LEFT_CENTER, &layer.name, egui::FontId::proportional(12.0), egui::Color32::LIGHT_GRAY);
            let end = layer.length.map_or(dur, |l| layer.start + l).min(dur);
            let color = match layer.content {
                LayerContent::Media { .. } => egui::Color32::from_rgb(70, 110, 170),
                LayerContent::Paint { .. } => egui::Color32::from_rgb(150, 100, 60),
                LayerContent::Solid { .. } => egui::Color32::from_rgb(90, 90, 90),
            };
            let bar = egui::Rect::from_min_max(egui::pos2(x_of(layer.start), y + 3.0), egui::pos2(x_of(end), y + ROW_H - 3.0));
            p.rect_filled(bar, 3.0, if layer.visible { color } else { color.gamma_multiply(0.4) });
            for kt in layer.key_times() {
                let c = egui::pos2(x_of(kt), y + ROW_H * 0.5);
                p.add(egui::Shape::convex_polygon(
                    vec![c + egui::vec2(0.0, -5.0), c + egui::vec2(5.0, 0.0), c + egui::vec2(0.0, 5.0), c + egui::vec2(-5.0, 0.0)],
                    egui::Color32::from_rgb(255, 196, 64),
                    egui::Stroke::new(1.0, egui::Color32::BLACK),
                ));
            }
            // Click selects; dragging a clip bar moves it in time.
            let bar_resp = ui.interact(bar, ui.scope_id().with(("clip", *id)), egui::Sense::click_and_drag());
            if bar_resp.clicked() || bar_resp.drag_started() {
                app.selected = Some(*id);
            }
            if bar_resp.dragged() {
                let dt = (bar_resp.drag_delta().x / track.width()) as f64 * dur;
                let snap = app.doc.canvas.fps;
                if let Some(l) = app.gesture("Move clip").layer_mut(*id) {
                    l.start = ((l.start + dt).max(0.0) * snap).round() / snap;
                }
            }
            if bar_resp.drag_stopped() {
                app.end_gesture();
            }
            let name_rect = egui::Rect::from_min_size(row_rect.min, egui::vec2(NAME_W, ROW_H));
            if ui.interact(name_rect, ui.scope_id().with(("name", *id)), egui::Sense::click()).clicked() {
                app.selected = Some(*id);
            }
        }

        // Scrub on the ruler or empty track space.
        if let Some(pos) = resp.interact_pointer_pos()
            && (resp.dragged() || resp.clicked())
            && pos.x >= track.left()
        {
            app.set_time(t_of(pos.x));
        }

        let px = x_of(app.time);
        p.line_segment([egui::pos2(px, rect.top()), egui::pos2(px, rect.bottom())], (2.0, egui::Color32::from_rgb(230, 70, 70)));
    });
}
