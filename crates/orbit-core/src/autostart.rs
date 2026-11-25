//! Start Orbit when you sign in, hidden in the tray (`--background`).
//!
//! * Windows: an entry under HKCU\Software\Microsoft\Windows\CurrentVersion\Run
//! * macOS: a LaunchAgent in ~/Library/LaunchAgents
//! * Linux: ~/.config/autostart/OrbitOS.desktop

use anyhow::{Context, Result};

fn launcher() -> Result<auto_launch::AutoLaunch> {
    let exe = std::env::current_exe().context("can't find Orbit's program file")?;
    // On macOS, register the .app bundle when running from one.
    let path = exe.to_string_lossy().to_string();
    let path = match path.find(".app/Contents/MacOS/") {
        Some(i) if cfg!(target_os = "macos") => path[..i + 4].to_string(),
        _ => path,
    };
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name("OrbitOS")
        .set_app_path(&path)
        .set_args(&["--background"])
        .set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent)
        .build()
        .context("can't set up start-up for this system")
}

pub fn is_enabled() -> bool {
    launcher()
        .and_then(|l| l.is_enabled().map_err(Into::into))
        .unwrap_or(false)
}

pub fn set_enabled(on: bool) -> Result<()> {
    let l = launcher()?;
    if on {
        l.enable().context("couldn't add Orbit to start-up")?;
    } else if l.is_enabled().unwrap_or(false) {
        l.disable().context("couldn't remove Orbit from start-up")?;
    }
    Ok(())
}
