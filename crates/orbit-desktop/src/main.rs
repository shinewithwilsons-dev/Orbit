// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! OrbitOS desktop: top bar, app launcher, dock, and windows for Files,
//! Terminal, Claude, System Monitor and Settings.

mod app;
mod theme;

use orbit_core::Config;

fn main() -> eframe::Result<()> {
    let cfg = Config::load();
    let fullscreen = cfg.fullscreen || std::env::args().any(|a| a == "--fullscreen");

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("OrbitOS")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([900.0, 600.0]);
    if fullscreen {
        viewport = viewport.with_fullscreen(true);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "OrbitOS",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Box::new(app::Desktop::new(cfg, cc.egui_ctx.clone()))
        }),
    )
}
