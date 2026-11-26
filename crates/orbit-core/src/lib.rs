//! OrbitOS core library.
//!
//! Everything platform-specific lives here so the shell and desktop stay
//! identical on Windows, Linux and macOS.

pub mod actions;
pub mod antivirus;
pub mod apps;
pub mod autostart;
pub mod backup;
pub mod calc;
pub mod claude;
pub mod config;
pub mod exec;
pub mod files;
pub mod hardware;
pub mod instance;
pub mod malware;
pub mod media;
pub mod network;
pub mod office;
pub mod radio;
pub mod search;
pub mod store;
pub mod system;
pub mod voice;
pub mod watch;
pub mod weather;

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

/// Standard user folders. Uses the OS setting (localised names, XDG on
/// Linux) and falls back to `~/Documents`, `~/Music`, … when it's not set.
pub mod user_dirs {
    use std::path::PathBuf;

    fn pick(os: Option<PathBuf>, fallback: &str) -> Option<PathBuf> {
        os.filter(|p| p.exists()).or_else(|| {
            let p = dirs::home_dir()?.join(fallback);
            p.exists().then_some(p)
        })
    }
    pub fn documents() -> Option<PathBuf> {
        pick(dirs::document_dir(), "Documents")
    }
    pub fn downloads() -> Option<PathBuf> {
        pick(dirs::download_dir(), "Downloads")
    }
    pub fn desktop() -> Option<PathBuf> {
        pick(dirs::desktop_dir(), "Desktop")
    }
    pub fn music() -> Option<PathBuf> {
        pick(dirs::audio_dir(), "Music")
    }
    pub fn videos() -> Option<PathBuf> {
        pick(
            dirs::video_dir(),
            if cfg!(target_os = "macos") {
                "Movies"
            } else {
                "Videos"
            },
        )
    }
}

/// Percent-encode a string for use in a URL query.
pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "%20".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Seconds since 1970.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Days since 1970 (UTC), for once-a-day jobs.
pub fn today() -> i64 {
    (now_secs() / 86_400) as i64
}

/// Minimal standard base64 decoder (no external crate needed).
pub fn b64_decode(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let clean: Vec<u8> = input
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
        .collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
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
    fn urls() {
        assert_eq!(url_encode("São Paulo & co"), "S%C3%A3o%20Paulo%20%26%20co");
    }

    #[test]
    fn base64() {
        assert_eq!(b64_decode("T3JiaXRPUw==").unwrap(), b"OrbitOS");
        assert_eq!(b64_decode("YQ").unwrap(), b"a");
    }

    #[test]
    fn tilde() {
        assert_eq!(expand_tilde("/tmp"), std::path::PathBuf::from("/tmp"));
        if let Some(h) = dirs::home_dir() {
            assert_eq!(expand_tilde("~/x"), h.join("x"));
        }
    }
}
