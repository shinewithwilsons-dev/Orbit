//! OrbitOS core library.
//!
//! Everything platform-specific lives here so the shell and desktop stay
//! identical on Windows, Linux and macOS.

pub mod apps;
pub mod claude;
pub mod config;
pub mod exec;
pub mod files;
pub mod system;

pub use claude::{Message, Role};
pub use config::{BackendKind, Config};

/// Human-readable name of the host platform.
pub fn host_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "Unix"
    }
}

/// Shell used to run commands on the host.
pub fn host_shell() -> &'static str {
    if cfg!(windows) {
        "cmd.exe"
    } else {
        "sh"
    }
}

/// Expand a leading `~` to the user's home directory.
pub fn expand_tilde(input: &str) -> std::path::PathBuf {
    if input == "~" {
        return dirs::home_dir().unwrap_or_else(|| ".".into());
    }
    if let Some(rest) = input
        .strip_prefix("~/")
        .or_else(|| input.strip_prefix("~\\"))
    {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    std::path::PathBuf::from(input)
}

/// Format a byte count as KB / MB / GB.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_format() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
    }

    #[test]
    fn tilde() {
        assert_eq!(expand_tilde("/tmp"), std::path::PathBuf::from("/tmp"));
        if let Some(h) = dirs::home_dir() {
            assert_eq!(expand_tilde("~/x"), h.join("x"));
        }
    }
}
