// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! OrbitOS desktop: top bar, app launcher, dock, and windows for Files,
//! Terminal, Claude, System Monitor and Settings.

mod app;
mod theme;

use orbit_core::Config;

fn main() -> eframe::Result<()> {
    // One Orbit per user: a second launch passes its request to the running
    // copy (show by default, or --toggle / --hide / --search / --quit) and exits.
    let args: Vec<String> = std::env::args().collect();
    // Windows only lets the foreground app hand focus on: allow the running
    // copy to come to the front if this launch turns out to be the second.
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
            windows_sys::Win32::UI::WindowsAndMessaging::ASFW_ANY,
        );
    }
    let cmd = orbit_core::instance::Command::from_args(&args)
        .unwrap_or(orbit_core::instance::Command::Show);
    let primary = match orbit_core::instance::start(cmd) {
        Ok(orbit_core::instance::Start::Forwarded) => return Ok(()),
        Ok(orbit_core::instance::Start::Primary(p)) => Some(p),
        Err(e) => {
            eprintln!("Orbit: single-instance check unavailable ({e}); starting anyway");
            None
        }
    };
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
            Box::new(app::Desktop::new(cfg, cc.egui_ctx.clone(), primary))
        }),
    )
}
