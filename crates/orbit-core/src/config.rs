//! Persistent settings, stored as TOML in the platform config folder:
//! - Linux:   ~/.config/orbitos/config.toml
//! - macOS:   ~/Library/Application Support/orbitos/config.toml
//! - Windows: %APPDATA%\orbitos\config.toml

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Which route OrbitOS uses to reach Claude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    /// The signed-in Claude Code CLI (`claude -p`). No API key stored.
    Cli,
    /// Anthropic Messages API with an API key.
    Api,
    /// API when a key is available, otherwise CLI.
    Auto,
    /// Your own OpenAI-compatible server (vLLM, Ollama, LM Studio, llama.cpp).
    Local,
}

impl BackendKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cli" => Some(Self::Cli),
            "api" => Some(Self::Api),
            "auto" => Some(Self::Auto),
            "local" | "vllm" => Some(Self::Local),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cli => "Claude Code CLI",
            Self::Api => "Anthropic API",
            Self::Auto => "Auto",
            Self::Local => "Local server (vLLM)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub backend: BackendKind,
    /// Model used by the API backend (the CLI uses its own default).
    pub model: String,
    pub max_tokens: u32,
    /// Optional; `ANTHROPIC_API_KEY` in the environment takes priority.
    pub api_key: Option<String>,
    /// Optional explicit path to the `claude` executable.
    pub cli_path: Option<String>,
    /// Start the desktop fullscreen (useful as a login session).
    pub fullscreen: bool,
    /// Orbit Shield: also scan with ClamAV when it is installed.
    pub use_clamav: bool,
    /// Orbit Shield: watch Downloads and Desktop for new files.
    pub shield_watch: bool,
    /// Media player volume, 0.0 to 1.0.
    pub volume: f32,

    // --- Local AI server (vLLM / any OpenAI-compatible API) ---
    pub local_url: String,
    /// Empty = use the first model the server lists.
    pub local_model: String,
    pub local_api_key: Option<String>,
    /// Optional image model for `/v1/images/generations`.
    pub image_model: String,

    // --- Appearance ---
    pub theme: String,
    pub accent: Option<[u8; 3]>,
    pub wallpaper: String,
    pub wallpaper_image: Option<String>,
    /// Animate the wallpaper (uses a little more power).
    pub animate_wallpaper: bool,

    // --- Voice ---
    pub whisper_bin: Option<String>,
    pub whisper_model: Option<String>,
    pub speak_replies: bool,
    pub wake_word: bool,

    // --- Widgets ---
    pub weather_city: String,

    // --- Media ---
    pub eq: [f32; 3],
    pub bemusic_url: Option<String>,
    pub bemusic_token: Option<String>,

    // --- Orbit Shield ---
    pub scheduled_scan: bool,
    pub scan_hour: u8,
    pub last_scheduled_scan_day: i64,

    // --- Backup ---
    pub backup_dir: Option<String>,
    pub auto_backup: bool,
    pub last_backup_day: i64,

    /// Set once the first-run welcome is finished.
    pub onboarded: bool,

    // --- Tray & background (0.3.1) ---
    /// Closing the window hides Orbit to the tray instead of quitting.
    pub close_to_tray: bool,
    /// The one-time "Orbit is still running" tip has been shown.
    pub tray_tip_shown: bool,
    /// Ctrl+Alt+O (Cmd+Option+O on macOS) shows or hides Orbit from anywhere.
    pub global_hotkey: bool,
    /// Keep listening for "Hey Orbit" while Orbit is hidden in the tray.
    pub listen_while_hidden: bool,
    /// Interface size. None = pick automatically from the screen (4K screens get larger text).
    pub ui_scale: Option<f32>,
    /// The HD nebula became the standard wallpaper in 0.3.2; switched once for existing setups.
    pub nebula_default_applied: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            backend: BackendKind::Cli,
            model: "claude-sonnet-5-5".into(),
            max_tokens: 4096,
            api_key: None,
            cli_path: None,
            fullscreen: false,
            use_clamav: true,
            shield_watch: true,
            volume: 0.8,
            local_url: "http://localhost:8000/v1".into(),
            local_model: String::new(),
            local_api_key: None,
            image_model: String::new(),
            theme: "orbit".into(),
            accent: None,
            wallpaper: "nebula".into(),
            wallpaper_image: None,
            animate_wallpaper: true,
            whisper_bin: None,
            whisper_model: None,
            speak_replies: false,
            wake_word: false,
            weather_city: "London".into(),
            eq: [0.0, 0.0, 0.0],
            bemusic_url: None,
            bemusic_token: None,
            scheduled_scan: true,
            scan_hour: 12,
            last_scheduled_scan_day: 0,
            backup_dir: None,
            auto_backup: false,
            last_backup_day: 0,
            onboarded: false,
            close_to_tray: true,
            tray_tip_shown: false,
            global_hotkey: true,
            listen_while_hidden: false,
            ui_scale: None,
            nebula_default_applied: true,
        }
    }
}

impl Config {
    /// Folder for OrbitOS data (chats, playlists, window layout, …).
    pub fn data_dir() -> PathBuf {
        let d = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("orbitos");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("orbitos")
            .join("config.toml")
    }

    pub fn load() -> Self {
        let mut c: Self = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default();
        if !c.nebula_default_applied {
            // Old default wallpaper → the new standard one. A deliberate choice is kept.
            if c.wallpaper == "starfield" {
                c.wallpaper = "nebula".into();
            }
            c.nebula_default_applied = true;
        }
        c
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    /// The API key in use, environment first.
    pub fn effective_api_key(&self) -> Option<String> {
        std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| self.api_key.clone().filter(|k| !k.trim().is_empty()))
    }

    /// Resolve `Auto` to a concrete backend.
    pub fn resolved_backend(&self) -> BackendKind {
        match self.backend {
            BackendKind::Auto => {
                if self.effective_api_key().is_some() {
                    BackendKind::Api
                } else {
                    BackendKind::Cli
                }
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_cli() {
        let c = Config::default();
        assert_eq!(c.backend, BackendKind::Cli);
        assert_eq!(c.resolved_backend(), BackendKind::Cli);
    }

    #[test]
    fn roundtrip_toml() {
        let mut c = Config::default();
        c.backend = BackendKind::Auto;
        c.model = "x".into();
        let s = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.backend, BackendKind::Auto);
        assert_eq!(back.model, "x");
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let c: Config = toml::from_str("backend = \"api\"").unwrap();
        assert_eq!(c.backend, BackendKind::Api);
        assert_eq!(c.max_tokens, 4096);
    }

    #[test]
    fn parse_backend() {
        assert_eq!(BackendKind::parse("API"), Some(BackendKind::Api));
        assert_eq!(BackendKind::parse("nope"), None);
    }
}
