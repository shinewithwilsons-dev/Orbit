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
    // App icon (taskbar / dock / window title).
    if let Ok(img) =
        image::load_from_memory(include_bytes!("../../../packaging/icons/icon-256.png"))
    {
        let img = img.to_rgba8();
        viewport = viewport.with_icon(eframe::egui::IconData {
            width: img.width(),
            height: img.height(),
            rgba: img.into_raw(),
        });
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "OrbitOS",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, &cfg);
            Box::new(app::Desktop::new(cfg, cc.egui_ctx.clone()))
        }),
    )
}
