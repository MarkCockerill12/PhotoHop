//! UI layout: menu + toolbar on top, layers left, inspector right, timeline bottom, viewer centre.

mod colour;
mod curves;
mod glass;
mod inspector;
mod integration;
mod layers;
mod menu;
pub mod roto;
mod timeline;
mod viewer;

use crate::app::{App, Tool};
pub use curves::CurveChannel;

pub fn draw(app: &mut App, ui: &mut egui::Ui, rs: &egui_wgpu::RenderState) {
    handle_shortcuts(app, ui.ctx());
    handle_dropped_files(app, ui.ctx());
    app.media.poll();
    app.poll_ai();
    app.poll_camera_job();
    app.advance_playback();
    if app.export.job.is_some() && app.pump_export() {
        ui.ctx().request_repaint();
    }
    if app.blender_job.is_some() && app.pump_blender_export() {
        ui.ctx().request_repaint();
    }

    egui::Panel::top("menu").show(ui, |ui| {
        menu::menu_bar(app, ui);
        menu::toolbar(app, ui);
    });
    egui::Panel::bottom("status").exact_size(22.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(&app.status);
            if let Some(job) = &app.export.job {
                ui.separator();
                ui.add(egui::ProgressBar::new(job.frame as f32 / job.total.max(1) as f32).desired_width(200.0).show_percentage());
            }
            if let Some(job) = &app.blender_job {
                ui.separator();
                ui.label("Blender bundle");
                ui.add(egui::ProgressBar::new(job.progress()).desired_width(200.0).show_percentage());
            }
        });
    });
    egui::Panel::bottom("timeline").resizable(true).default_size(190.0).min_size(110.0).show(ui, |ui| {
        timeline::timeline(app, ui);
    });
    egui::Panel::left("layers").resizable(true).default_size(250.0).show(ui, |ui| {
        layers::layers_panel(app, ui);
    });
    egui::Panel::right("inspector").resizable(true).default_size(290.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| inspector::inspector(app, ui));
    });
    egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::from_gray(24))).show(ui, |ui| {
        viewer::viewer(app, ui, rs);
    });
    menu::export_window(app, ui.ctx());
    app.scopes.window(ui.ctx());
    menu::ai_window(app, ui.ctx());
    glass::window(app, ui.ctx());

    if app.playing {
        ui.ctx().request_repaint();
    }
}

fn handle_shortcuts(app: &mut App, ctx: &egui::Context) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
    let pressed = |k: egui::Key| ctx.input(|i| i.key_pressed(k));
    if cmd {
        if pressed(egui::Key::Z) {
            if shift { app.redo() } else { app.undo() }
        }
        if pressed(egui::Key::Y) {
            app.redo();
        }
        if pressed(egui::Key::S) {
            menu::save(app, shift);
        }
        if pressed(egui::Key::O) {
            menu::open_dialog(app);
        }
        if pressed(egui::Key::I) {
            menu::import_dialog(app);
        }
        if pressed(egui::Key::D) {
            app.duplicate_selected();
        }
        if pressed(egui::Key::Num0) {
            app.viewer.fit_pending = true;
        }
        return;
    }
    for (key, tool) in [
        (egui::Key::V, Tool::Move),
        (egui::Key::M, Tool::Select),
        (egui::Key::C, Tool::Crop),
        (egui::Key::B, Tool::Brush),
        (egui::Key::E, Tool::Eraser),
        (egui::Key::P, Tool::Roto),
        (egui::Key::H, Tool::Hand),
    ] {
        if pressed(key) {
            app.tool = tool;
        }
    }
    if pressed(egui::Key::Space) {
        app.toggle_play();
    }
    if app.tool == Tool::Roto && roto::keys(app, ctx) {
        // Enter / Escape / Delete went to the roto shape.
    } else if pressed(egui::Key::Delete) {
        app.delete_selected();
    }
    let fps = app.doc.canvas.fps;
    if pressed(egui::Key::ArrowLeft) {
        app.set_time(app.time - 1.0 / fps);
    }
    if pressed(egui::Key::ArrowRight) {
        app.set_time(app.time + 1.0 / fps);
    }
    if pressed(egui::Key::Home) {
        app.set_time(0.0);
    }
    if pressed(egui::Key::S) {
        app.scopes.show = !app.scopes.show;
    }
    if pressed(egui::Key::K) {
        crate::ui::inspector::toggle_all_keys(app);
    }
}

fn handle_dropped_files(app: &mut App, ctx: &egui::Context) {
    let files: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
    for path in files {
        let res = if path.extension().is_some_and(|e| e == ph_core::project_file::EXTENSION) {
            app.open(&path)
        } else {
            app.import(&path)
        };
        if let Err(e) = res {
            app.status = format!("Could not open {}: {e:#}", path.display());
        }
    }
}
