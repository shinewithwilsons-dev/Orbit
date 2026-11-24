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
}

impl BackendKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cli" => Some(Self::Cli),
            "api" => Some(Self::Api),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cli => "Claude Code CLI",
            Self::Api => "Anthropic API",
            Self::Auto => "Auto",
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
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("orbitos")
            .join("config.toml")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
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
