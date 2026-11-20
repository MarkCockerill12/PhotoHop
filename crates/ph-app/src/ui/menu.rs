//! Menu bar, toolbar, file dialogs and the export window.

use crate::app::{App, Tool};
use crate::export::ImageFormat;
use ph_media::VideoCodec;

const MEDIA_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "tif", "tiff", "exr", "mp4", "mov", "mkv", "webm", "avi", "m4v"];

pub fn import_dialog(app: &mut App) {
    if let Some(paths) = rfd::FileDialog::new().add_filter("Images & video", MEDIA_EXTS).pick_files() {
        for p in paths {
            if let Err(e) = app.import(&p) {
                app.status = format!("Import failed: {e:#}");
            }
        }
    }
}

pub fn open_dialog(app: &mut App) {
    if let Some(p) = rfd::FileDialog::new().add_filter("PhotoHop project", &["phop"]).pick_file()
        && let Err(e) = app.open(&p)
    {
        app.status = format!("Open failed: {e:#}");
    }
}

pub fn save(app: &mut App, save_as: bool) {
    let path = match (&app.project_path, save_as) {
        (Some(p), false) => Some(p.clone()),
        _ => rfd::FileDialog::new()
            .add_filter("PhotoHop project", &["phop"])
            .set_file_name(format!("{}.phop", app.doc.name))
            .save_file(),
    };
    if let Some(p) = path
        && let Err(e) = app.save_to(&p)
    {
        app.status = format!("Save failed: {e:#}");
    }
}

pub fn menu_bar(app: &mut App, ui: &mut egui::Ui) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            ui.menu_button("New project", |ui| {
                for (label, w, h, fps) in [
                    ("1920×1080 landscape", 1920, 1080, 24.0),
                    ("1080×1920 vertical", 1080, 1920, 24.0),
                    ("3840×2160 4K", 3840, 2160, 24.0),
                    ("2160×3840 4K vertical", 2160, 3840, 24.0),
                ] {
                    if ui.button(label).clicked() {
                        app.new_project(w, h, fps, 10.0);
                    }
                }
            });
            if ui.button("Open…  (Ctrl+O)").clicked() {
                open_dialog(app);
            }
            if ui.button("Save  (Ctrl+S)").clicked() {
                save(app, false);
            }
            if ui.button("Save as…  (Ctrl+Shift+S)").clicked() {
                save(app, true);
            }
            ui.separator();
            if ui.button("Import photo / video…  (Ctrl+I)").clicked() {
                import_dialog(app);
            }
            if ui.button("Export…").clicked() {
                app.export.open = true;
            }
        });
        ui.menu_button("Edit", |ui| {
            let undo = app.history.undo_label().map(|l| format!("Undo {l}")).unwrap_or_else(|| "Undo".into());
            if ui.button(format!("{undo}  (Ctrl+Z)")).clicked() {
                app.undo();
            }
            if ui.button("Redo  (Ctrl+Shift+Z)").clicked() {
                app.redo();
            }
        });
        ui.menu_button("Layer", |ui| {
            if ui.button("New paint layer").clicked() {
                app.add_paint_layer();
            }
            if ui.button("New colour layer").clicked() {
                app.add_solid_layer();
            }
            if ui.button("Duplicate  (Ctrl+D)").clicked() {
                app.duplicate_selected();
            }
            if ui.button("Delete  (Del)").clicked() {
                app.delete_selected();
            }
        });
        ui.menu_button("View", |ui| {
            if ui.button("Fit to window  (Ctrl+0)").clicked() {
                app.viewer.fit_pending = true;
            }
            if ui.button("100%").clicked() {
                app.viewer.zoom = 1.0;
                app.viewer.pan = egui::Vec2::ZERO;
            }
            ui.separator();
            ui.label("Video preview quality");
            let mut div = app.media.proxy_div;
            for (label, d) in [("Full", 1), ("Half (default)", 2), ("Quarter", 4)] {
                ui.radio_value(&mut div, d, label);
            }
            if div != app.media.proxy_div {
                app.media.proxy_div = div;
                app.media.clear();
            }
        });
    });
}

pub fn toolbar(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        for t in Tool::ALL {
            let r = ui.selectable_label(app.tool == t, t.icon().rich_text().size(18.0)).on_hover_text(t.label());
            if r.clicked() {
                app.tool = t;
            }
        }
        ui.separator();
        match app.tool {
            Tool::Brush | Tool::Eraser => {
                let b = &mut app.brush;
                ui.label("Size");
                ui.add(egui::DragValue::new(&mut b.size).range(1.0..=2000.0).speed(1.0).suffix(" px"));
                ui.label("Hardness");
                ui.add(egui::Slider::new(&mut b.hardness, 0.0..=1.0).show_value(true));
                ui.label("Opacity");
                ui.add(egui::Slider::new(&mut b.opacity, 0.0..=1.0));
                if app.tool == Tool::Brush {
                    ui.color_edit_button_srgba(&mut b.color);
                }
            }
            Tool::Crop => {
                ui.label("Drag on the canvas to choose the crop, then press Enter (Esc cancels).");
            }
            Tool::Move => {
                ui.label("Drag to move · corner handles scale (Shift = free) · top handle rotates.");
            }
            Tool::Hand => {
                ui.label("Drag to pan · scroll to zoom.");
            }
        }
    });
}

pub fn export_window(app: &mut App, ctx: &egui::Context) {
    let mut open = app.export.open;
    egui::Window::new("Export").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.heading("Current frame as image");
        egui::ComboBox::from_id_salt("imgfmt").selected_text(app.export.image_format.label()).show_ui(ui, |ui| {
            for f in ImageFormat::ALL {
                ui.selectable_value(&mut app.export.image_format, f, f.label());
            }
        });
        if ui.button("Export image…").clicked() {
            let ext = app.export.image_format.extension();
            if let Some(p) = rfd::FileDialog::new().add_filter(ext, &[ext]).set_file_name(format!("{}.{ext}", app.doc.name)).save_file()
                && let Err(e) = app.export_image(p)
            {
                app.status = format!("Export failed: {e:#}");
            }
        }
        ui.separator();
        ui.heading("Timeline as video");
        egui::ComboBox::from_id_salt("codec").selected_text(app.export.codec.label()).show_ui(ui, |ui| {
            for c in [VideoCodec::H264, VideoCodec::H265, VideoCodec::ProRes4444] {
                ui.selectable_value(&mut app.export.codec, c, c.label());
            }
        });
        ui.checkbox(&mut app.export.audio, "Include audio from the first video layer");
        let c = &app.doc.canvas;
        ui.label(format!("{}×{} · {:.3} fps · {} frames", c.width, c.height, c.fps, c.frame_count()));
        let busy = app.export.job.is_some();
        if ui.add_enabled(!busy, egui::Button::new("Export video…")).clicked() {
            let ext = app.export.codec.extension();
            if let Some(p) = rfd::FileDialog::new().add_filter(ext, &[ext]).set_file_name(format!("{}.{ext}", app.doc.name)).save_file()
                && let Err(e) = app.start_video_export(p)
            {
                app.status = format!("Export failed: {e:#}");
            }
        }
        if busy && ui.button("Cancel export").clicked() {
            app.export.job = None;
            app.status = "Export cancelled".into();
        }
    });
    app.export.open = open;
}
