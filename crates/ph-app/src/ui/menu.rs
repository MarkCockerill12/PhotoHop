//! Menu bar, toolbar, file dialogs and the export window.

use crate::app::{App, Tool};
use crate::export::ImageFormat;
use ph_media::VideoCodec;

pub const MEDIA_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "tif", "tiff", "exr", "mp4", "mov", "mkv", "webm", "avi", "m4v"];

pub fn import_dialog(app: &mut App) {
    let scripted = app.dialog_answers.pop_front().map(|p| vec![p]);
    if let Some(paths) = scripted.or_else(|| rfd::FileDialog::new().add_filter("Images & video", MEDIA_EXTS).pick_files()) {
        for p in paths {
            if let Err(e) = app.import(&p) {
                app.status = format!("Import failed: {e:#}");
            }
        }
    }
}

pub fn open_dialog(app: &mut App) {
    if let Some(p) = app.dialog(|| rfd::FileDialog::new().add_filter("PhotoHop project", &["phop"]).pick_file())
        && let Err(e) = app.open(&p)
    {
        app.status = format!("Open failed: {e:#}");
    }
}

pub fn save(app: &mut App, save_as: bool) {
    let path = match (&app.project_path, save_as) {
        (Some(p), false) => Some(p.clone()),
        _ => {
            let name = format!("{}.phop", app.doc.name);
            app.dialog(|| rfd::FileDialog::new().add_filter("PhotoHop project", &["phop"]).set_file_name(name).save_file())
        }
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
            ui.separator();
            if ui.button("Export to Blender…").on_hover_text("Writes a bundle for the PhotoHop Bridge add-on (selected layer = you)").clicked()
                && let Some(dir) = app.dialog(|| rfd::FileDialog::new().set_title("Choose an empty folder for the Blender bundle").pick_folder())
                && let Err(e) = app.start_blender_export(&dir)
            {
                app.status = format!("Blender export failed: {e:#}");
            }
            if ui.button("Import Blender render…").clicked()
                && let Some(dir) = app.dialog(|| rfd::FileDialog::new().set_title("Pick the Blender bundle folder").pick_folder())
                && let Err(e) = app.import_blender_render(&dir)
            {
                app.status = format!("Import failed: {e:#}");
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
            if ui.button("Import photo / video…  (Ctrl+I)").clicked() {
                import_dialog(app);
            }
            if ui.button("New paint layer").on_hover_text("An empty layer to paint on with the Brush").clicked() {
                app.add_paint_layer();
            }
            if ui.button("New colour layer").on_hover_text("A layer filled with one colour (e.g. a tint with a blend mode)").clicked() {
                app.add_solid_layer();
            }
            if ui.button("Duplicate  (Ctrl+D)").clicked() {
                app.duplicate_selected();
            }
            if ui.button("Delete  (Del)").clicked() {
                app.delete_selected();
            }
            ui.separator();
            let sel = app.selected;
            let busy = app.ai_running();
            if ui.add_enabled(sel.is_some() && !busy, egui::Button::new("Remove background (AI)")).clicked()
                && let Err(e) = app.start_ai(crate::ai::AiTask::Matte)
            {
                app.status = format!("AI: {e:#}");
            }
            if ui.add_enabled(sel.is_some(), egui::Button::new("Split into separate objects")).on_hover_text("One layer per object, e.g. three figures in one picture").clicked()
                && let Some(id) = sel
                && let Err(e) = app.split_objects(id)
            {
                app.status = format!("Split: {e:#}");
            }
            if ui.add_enabled(sel.is_some(), egui::Button::new("One-click fit into the scene")).on_hover_text("Colour match + ground shadow + grain").clicked()
                && let Some(id) = sel
            {
                app.status = app.auto_fit(id).unwrap_or_else(|e| format!("Fit failed: {e:#}"));
            }
        });
        ui.menu_button("AI", |ui| {
            let busy = app.ai_running();
            let run = |app: &mut App, task: crate::ai::AiTask| {
                if let Err(e) = app.start_ai(task) {
                    app.status = format!("AI: {e:#}");
                }
            };
            ui.label(egui::RichText::new("Photos").small().weak());
            if ui.add_enabled(!busy, egui::Button::new("Remove background")).on_hover_text("Keep the main subject of the selected layer, hide the rest").clicked() {
                run(app, crate::ai::AiTask::Matte);
            }
            if ui.add_enabled(!busy, egui::Button::new("Keep only… (describe it)")).on_hover_text("Type what to keep, e.g. \"person\" or \"toy gun\"").clicked() {
                app.ai.show_dialog = true;
            }
            if ui.add_enabled(!busy, egui::Button::new("Remove an object…")).on_hover_text("Erase something from the picture; AI fills the gap (the only AI tool that paints)").clicked() {
                app.ai.show_dialog = true;
            }
            if ui.add_enabled(!busy, egui::Button::new("Match colours with AI")).on_hover_text("AI suggests how the selected layer should look in the scene; applied as normal colour sliders").clicked() {
                run(app, crate::ai::AiTask::AutoColour);
            }
            ui.separator();
            ui.label(egui::RichText::new("Video").small().weak());
            if ui.add_enabled(!busy, egui::Button::new("Remove background (whole clip)")).clicked() {
                run(app, crate::ai::AiTask::VideoCutout);
            }
            if ui.add_enabled(!busy, egui::Button::new("Track a region… (draw a box)")).on_hover_text("Follow something through the clip, e.g. a window, and make a moving mask of it").clicked() {
                app.ai.picking_box = true;
                app.status = "Drag a box around the region to track (e.g. the window glass) on the selected video layer.".into();
            }
            if ui.add_enabled(!busy, egui::Button::new("Replace a window or screen…")).on_hover_text("Show another clip through a window, TV or phone screen").clicked() {
                if let Some(id) = app.selected {
                    app.glass_setup(id);
                    app.glass_panel = true;
                    app.status = "Replace a window or screen: follow the steps in the panel.".into();
                } else {
                    app.status = "Select the layer with the window or screen first.".into();
                }
            }
            ui.separator();
            ui.label(egui::RichText::new("For Blender").small().weak());
            if ui.add_enabled(!busy, egui::Button::new("Work out the camera movement")).on_hover_text("3D camera solve (Depth Anything 3): recovers how the phone moved and where the floor is, for Blender").clicked() {
                run(app, crate::ai::AiTask::CameraSolve);
            }
            if ui.add_enabled(!busy, egui::Button::new("Capture body motion")).on_hover_text("SAM 3D Body: your 3D body every frame, for real shadows and props in your hands in Blender. Remove the background of the clip first.").clicked() {
                run(app, crate::ai::AiTask::BodyMotion);
            }
            ui.separator();
            if ui.button("AI settings…").clicked() {
                app.ai.show_dialog = true;
            }
            ui.label(egui::RichText::new("AI makes masks and colour/light settings. Only \"Remove an object\" paints, and only inside that object.").small().weak());
        });
        ui.menu_button("View", |ui| {
            ui.checkbox(&mut app.scopes.show, "Scopes  (S)");
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
        ui.menu_button("Help", |ui| {
            if ui.button("Manual  (F1)").clicked() {
                app.help.open_page("start");
            }
            if ui.button("Your first composite (step by step)").clicked() {
                app.help.open_page("first-composite");
            }
            if ui.button("Keyboard shortcuts").clicked() {
                app.help.open_page("shortcuts");
            }
            if ui.button("Open the manual in your web browser").clicked() {
                app.status = match crate::ui::help::open_in_browser() {
                    Ok(p) => format!("Manual opened in your browser ({}).", p.display()),
                    Err(e) => format!("Could not open the browser: {e:#}"),
                };
            }
            ui.separator();
            ui.label(egui::RichText::new(format!("PhotoHop {}", env!("CARGO_PKG_VERSION"))).weak());
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
            Tool::Roto => {
                ui.label(egui::RichText::new("Click: add point · drag: curve · click first point / Enter: close · Del: remove point · Alt-drag handle: break · Esc: done").weak());
            }
            Tool::Brush | Tool::Eraser => {
                if app.edit_mask && app.selected_layer().is_some_and(|l| l.mask.is_some()) {
                    ui.label(egui::RichText::new("Editing mask").color(egui::Color32::from_rgb(255, 196, 64)))
                        .on_hover_text("Brush hides, eraser reveals. Turn off in the inspector's Mask section.");
                }
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
            Tool::Select => {
                for s in crate::selection::SelectShape::ALL {
                    ui.selectable_value(&mut app.select_shape, s, s.label());
                }
                ui.separator();
                if app.select_shape == crate::selection::SelectShape::Wand {
                    ui.label("Tolerance");
                    ui.add(egui::Slider::new(&mut app.wand_threshold, 0.01..=0.6));
                }
                ui.label("Feather");
                ui.add(egui::DragValue::new(&mut app.select_feather).range(0.0..=200.0).suffix(" px"));
                ui.label(egui::RichText::new("Selection becomes the layer mask · Shift add · Alt subtract").weak());
            }
            Tool::Move => {
                ui.label("Drag to move · corner handles scale (Shift = free) · top handle rotates.");
            }
            Tool::Hand => {
                ui.label("Drag to pan · scroll to zoom.");
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.toggle_value(&mut app.scopes.show, "Scopes").on_hover_text("Waveform, parade, vectorscope, histogram (S)");
        });
    });
}

pub fn ai_window(app: &mut App, ctx: &egui::Context) {
    let mut open = app.ai.show_dialog;
    egui::Window::new("AI: keep or remove by description").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.label("Keep only this (describe it, e.g. person, dog, toy gun):");
        let r = ui.text_edit_singleline(&mut app.ai.prompt);
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let busy = app.ai_running();
        ui.horizontal(|ui| {
            if (ui.add_enabled(!busy, egui::Button::new("Keep only this")).clicked() || enter) && !busy {
                let p = app.ai.prompt.trim().to_owned();
                if let Err(e) = app.start_ai(crate::ai::AiTask::Segment(p)) {
                    app.status = format!("AI: {e:#}");
                }
            }
            if ui.add_enabled(!busy, egui::Button::new("Remove background instead")).clicked()
                && let Err(e) = app.start_ai(crate::ai::AiTask::Matte)
            {
                app.status = format!("AI: {e:#}");
            }
            if busy {
                ui.spinner();
            }
        });
        ui.separator();
        ui.label("Remove an object from the picture (AI fills only that area, as a separate patch layer you can hide):");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut app.ai.remove_prompt).hint_text("e.g. toy gun"));
            if ui.add_enabled(!busy, egui::Button::new("Remove")).clicked() {
                let p = app.ai.remove_prompt.trim().to_owned();
                if let Err(e) = app.start_ai(crate::ai::AiTask::Remove(p)) {
                    app.status = format!("AI: {e:#}");
                }
            }
        });
        ui.separator();
        ui.label("AI settings: the Python that has the PhotoHop AI tools installed (see the manual, AI tools):");
        ui.text_edit_singleline(&mut app.ai.python);
    });
    app.ai.show_dialog = open;
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
            let name = format!("{}.{ext}", app.doc.name);
            if let Some(p) = app.dialog(|| rfd::FileDialog::new().add_filter(ext, &[ext]).set_file_name(name).save_file())
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
            let name = format!("{}.{ext}", app.doc.name);
            if let Some(p) = app.dialog(|| rfd::FileDialog::new().add_filter(ext, &[ext]).set_file_name(name).save_file())
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
