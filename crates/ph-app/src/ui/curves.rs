//! Curves editor and HSL qualifier controls (part of the inspector's Colour section).

use crate::app::App;
use ph_core::grade::{IDENTITY_CURVE, eval_curve};
use ph_core::{Grade, LayerId};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum CurveChannel {
    #[default]
    Master,
    Red,
    Green,
    Blue,
}

impl CurveChannel {
    const ALL: [CurveChannel; 4] = [Self::Master, Self::Red, Self::Green, Self::Blue];

    fn label(self) -> &'static str {
        match self {
            Self::Master => "Master",
            Self::Red => "R",
            Self::Green => "G",
            Self::Blue => "B",
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Self::Master => egui::Color32::from_gray(220),
            Self::Red => egui::Color32::from_rgb(235, 80, 80),
            Self::Green => egui::Color32::from_rgb(90, 210, 90),
            Self::Blue => egui::Color32::from_rgb(90, 140, 240),
        }
    }

    fn points(self, g: &mut Grade) -> &mut Vec<[f32; 2]> {
        match self {
            Self::Master => &mut g.curves.master,
            Self::Red => &mut g.curves.red,
            Self::Green => &mut g.curves.green,
            Self::Blue => &mut g.curves.blue,
        }
    }
}

const PICK_R: f32 = 8.0;

fn set_points(app: &mut App, id: LayerId, ch: CurveChannel, pts: Vec<[f32; 2]>) {
    if let Some(l) = app.gesture("Curves").layer_mut(id) {
        *ch.points(&mut l.grade) = pts;
    }
}

pub fn curves_editor(app: &mut App, ui: &mut egui::Ui, id: LayerId, grade: &Grade) {
    ui.horizontal(|ui| {
        for ch in CurveChannel::ALL {
            ui.selectable_value(&mut app.curve_channel, ch, egui::RichText::new(ch.label()).color(ch.color()));
        }
        if ui.small_button("Reset").clicked() {
            let ch = app.curve_channel;
            app.edit("Reset curve", |d| {
                if let Some(l) = d.layer_mut(id) {
                    *ch.points(&mut l.grade) = IDENTITY_CURVE.to_vec();
                }
            });
        }
    });
    let ch = app.curve_channel;
    let mut g = grade.clone();
    let mut pts = ch.points(&mut g).clone();

    let side = ui.available_width().min(240.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, egui::Color32::from_gray(22));
    let grid = egui::Stroke::new(1.0, egui::Color32::from_gray(45));
    for q in 1..4 {
        let f = q as f32 / 4.0;
        p.line_segment([egui::pos2(rect.left() + f * side, rect.top()), egui::pos2(rect.left() + f * side, rect.bottom())], grid);
        p.line_segment([egui::pos2(rect.left(), rect.top() + f * side), egui::pos2(rect.right(), rect.top() + f * side)], grid);
    }
    let to_screen = |q: [f32; 2]| egui::pos2(rect.left() + q[0] * side, rect.bottom() - q[1] * side);
    let from_screen = |s: egui::Pos2| [((s.x - rect.left()) / side).clamp(0.0, 1.0), ((rect.bottom() - s.y) / side).clamp(0.0, 1.0)];

    // Other channels faintly, then the active one.
    for other in CurveChannel::ALL.into_iter().filter(|c| *c != ch) {
        let op = other.points(&mut g).clone();
        if op.as_slice() != IDENTITY_CURVE {
            let line: Vec<egui::Pos2> = (0..=64).map(|i| i as f32 / 64.0).map(|x| to_screen([x, eval_curve(&op, x)])).collect();
            p.add(egui::Shape::line(line, egui::Stroke::new(1.0, other.color().gamma_multiply(0.35))));
        }
    }
    let line: Vec<egui::Pos2> = (0..=128).map(|i| i as f32 / 128.0).map(|x| to_screen([x, eval_curve(&pts, x)])).collect();
    p.add(egui::Shape::line(line, egui::Stroke::new(1.5, ch.color())));

    // Interaction: drag nearest point, click empty space to add, double-click a point to delete.
    let hovered = resp.hover_pos().and_then(|h| pts.iter().position(|q| to_screen(*q).distance(h) < PICK_R));
    if resp.drag_started() {
        let pos = resp.interact_pointer_pos().unwrap_or_default();
        app.curve_drag = match pts.iter().position(|q| to_screen(*q).distance(pos) < PICK_R) {
            Some(i) => Some(i),
            None => {
                let q = from_screen(pos);
                let i = pts.partition_point(|e| e[0] < q[0]);
                pts.insert(i, q);
                Some(i)
            }
        };
    }
    if resp.dragged()
        && let (Some(i), Some(pos)) = (app.curve_drag, resp.interact_pointer_pos())
        && i < pts.len()
    {
        let mut q = from_screen(pos);
        let lo = if i > 0 { pts[i - 1][0] + 0.01 } else { 0.0 };
        let hi = if i + 1 < pts.len() { pts[i + 1][0] - 0.01 } else { 1.0 };
        q[0] = q[0].clamp(lo, hi.max(lo));
        pts[i] = q;
        set_points(app, id, ch, pts.clone());
    }
    if resp.drag_stopped() {
        app.curve_drag = None;
        app.end_gesture();
    }
    if resp.double_clicked()
        && let Some(i) = hovered
        && pts.len() > 2
    {
        pts.remove(i);
        app.edit("Delete curve point", |d| {
            if let Some(l) = d.layer_mut(id) {
                *ch.points(&mut l.grade) = pts.clone();
            }
        });
    } else if resp.clicked() && hovered.is_none() {
        let q = from_screen(resp.interact_pointer_pos().unwrap_or_default());
        let i = pts.partition_point(|e| e[0] < q[0]);
        pts.insert(i, [q[0], eval_curve(&pts, q[0])]);
        app.edit("Add curve point", |d| {
            if let Some(l) = d.layer_mut(id) {
                *ch.points(&mut l.grade) = pts.clone();
            }
        });
    }
    for (i, q) in pts.iter().enumerate() {
        let hot = Some(i) == hovered || Some(i) == app.curve_drag;
        p.circle(to_screen(*q), if hot { 5.0 } else { 4.0 }, ch.color(), egui::Stroke::new(1.0, egui::Color32::BLACK));
    }
    resp.on_hover_text("Click to add a point, drag to move, double-click a point to delete");
}

pub fn qualifier_controls(app: &mut App, ui: &mut egui::Ui, id: LayerId, grade: &Grade) {
    let mut q = grade.qualifier.clone();
    let before = q.clone();
    ui.horizontal(|ui| {
        ui.checkbox(&mut q.enabled, "Enabled");
        ui.checkbox(&mut q.invert, "Invert");
        ui.checkbox(&mut q.show_matte, "Show matte");
    });
    let mut dragging = false;
    fn row(ui: &mut egui::Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
        ui.label(label);
        let r = ui.add(egui::Slider::new(v, range).max_decimals(3));
        ui.end_row();
        r.dragged()
    }
    egui::Grid::new("qual_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
        ui.label("Hue");
        let hue = egui::ecolor::Hsva::new(q.hue_center, 1.0, 1.0, 1.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 2.0, egui::Color32::from(hue));
            let s = ui.add(egui::Slider::new(&mut q.hue_center, 0.0..=1.0).max_decimals(3));
            dragging |= s.dragged();
        });
        ui.end_row();
        dragging |= row(ui, "Hue width", &mut q.hue_width, 0.0..=0.5);
        dragging |= row(ui, "Sat low", &mut q.sat_range[0], 0.0..=1.0);
        dragging |= row(ui, "Sat high", &mut q.sat_range[1], 0.0..=1.0);
        dragging |= row(ui, "Lum low", &mut q.lum_range[0], 0.0..=1.0);
        dragging |= row(ui, "Lum high", &mut q.lum_range[1], 0.0..=1.0);
        dragging |= row(ui, "Softness", &mut q.softness, 0.0..=0.3);
        dragging |= row(ui, "Hue shift", &mut q.hue_shift, -0.5..=0.5);
        dragging |= row(ui, "Saturation", &mut q.saturation, 0.0..=2.0);
        dragging |= row(ui, "Brightness", &mut q.gain, 0.0..=2.0);
    });
    if q != before {
        if let Some(l) = app.gesture("Qualifier").layer_mut(id) {
            l.grade.qualifier = q;
        }
        if !dragging {
            app.end_gesture();
        }
    } else if !dragging && app.history.in_gesture() {
        app.end_gesture();
    }
}
