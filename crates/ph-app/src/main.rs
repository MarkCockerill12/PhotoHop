//! PhotoHop — put yourself into any scene, photo or video.

// GUI app: no console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ai;
mod app;
mod export;
mod matching;
mod media_cache;
mod paint;
mod render;
mod scopes;
mod ui;

use app::App;

struct PhotoHop {
    app: App,
}

impl eframe::App for PhotoHop {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let Some(rs) = frame.wgpu_render_state().cloned() else {
            ui.label("PhotoHop needs a GPU (wgpu) renderer.");
            return;
        };
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(self.app.title()));
        ui::draw(&mut self.app, ui, &rs);
    }
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,photohop=info")).init();
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PhotoHop")
            .with_inner_size([1600.0, 1000.0])
            .with_min_inner_size([900.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    if let egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.power_preference = wgpu::PowerPreference::HighPerformance;
    }
    eframe::run_native(
        "PhotoHop",
        options,
        Box::new(|cc| {
            let rs = cc.wgpu_render_state.as_ref().ok_or("PhotoHop needs the wgpu renderer")?;
            egui_material_icons::initialize(&cc.egui_ctx);
            let mut app = App::new(&cc.egui_ctx, &rs.device, &rs.queue);
            // Open the scopes window at startup (also handy for scripted screenshots).
            app.scopes.show = std::env::var_os("PHOTOHOP_SHOW_SCOPES").is_some();
            if let Some(path) = std::env::args_os().nth(1).map(std::path::PathBuf::from) {
                let res = if path.extension().is_some_and(|e| e == "phop") { app.open(&path) } else { app.import(&path) };
                if let Err(e) = res {
                    app.status = format!("Could not open {}: {e:#}", path.display());
                }
            }
            Ok(Box::new(PhotoHop { app }))
        }),
    )
}
