//! User configuration (`config.toml`), matching SPEC §10 defaults.
//!
//! Hotkeys use the human-readable accelerator syntax used by the UI:
//! - Open popup: `Cmd+Shift+Period` (documented Linux equivalent
//!   `Ctrl+Shift+Period`).
//! - Pause/resume: `Cmd+Shift+P` (Linux `Ctrl+Shift+P`).
//!
//! On Linux the UI should treat `Cmd` as `Ctrl` when registering the global
//! hotkey; the stored default string keeps the macOS spelling for portability.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Default open-popup hotkey (macOS spelling).
pub const DEFAULT_HOTKEY_OPEN: &str = "Cmd+Shift+Period";
/// Default pause hotkey (macOS spelling).
pub const DEFAULT_HOTKEY_PAUSE: &str = "Cmd+Shift+P";
/// Documented Linux equivalent for the open hotkey.
pub const DEFAULT_HOTKEY_OPEN_LINUX: &str = "Ctrl+Shift+Period";
/// Documented Linux equivalent for the pause hotkey.
pub const DEFAULT_HOTKEY_PAUSE_LINUX: &str = "Ctrl+Shift+P";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub paused: bool,
    pub poll_interval_ms: u64,
    pub max_text_bytes: u64,
    pub max_image_bytes: u64,
    pub thumbnail_max_edge: u32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            paused: false,
            poll_interval_ms: 300,
            max_text_bytes: 1_048_576,
            max_image_bytes: 5_242_880,
            thumbnail_max_edge: 512,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RetentionConfig {
    pub max_age_days: u32,
    pub max_items: u64,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            max_age_days: 30,
            max_items: 10_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub hotkey_open: String,
    pub hotkey_pause: String,
    pub recent_count: u32,
    pub launch_at_login: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            hotkey_open: DEFAULT_HOTKEY_OPEN.to_string(),
            hotkey_pause: DEFAULT_HOTKEY_PAUSE.to_string(),
            recent_count: 5,
            launch_at_login: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyConfig {
    pub honor_concealed: bool,
    pub secret_heuristics: bool,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            honor_concealed: true,
            secret_heuristics: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub capture: CaptureConfig,
    pub retention: RetentionConfig,
    pub ui: UiConfig,
    pub privacy: PrivacyConfig,
}

impl Config {
    /// Parse config from a TOML string.
    pub fn from_toml(s: &str) -> Result<Self> {
        Ok(toml::from_str(s)?)
    }

    /// Serialize config to a pretty TOML string.
    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Load config from a file. Missing files yield defaults.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)?;
        Self::from_toml(&raw)
    }

    /// Save config to a file, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_toml()?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = Config::default();
        assert_eq!(c.capture.poll_interval_ms, 300);
        assert_eq!(c.capture.max_text_bytes, 1_048_576);
        assert_eq!(c.capture.max_image_bytes, 5_242_880);
        assert_eq!(c.capture.thumbnail_max_edge, 512);
        assert!(!c.capture.paused);
        assert_eq!(c.retention.max_age_days, 30);
        assert_eq!(c.retention.max_items, 10_000);
        assert_eq!(c.ui.hotkey_open, DEFAULT_HOTKEY_OPEN);
        assert_eq!(c.ui.hotkey_pause, DEFAULT_HOTKEY_PAUSE);
        assert_eq!(c.ui.recent_count, 5);
        assert!(c.ui.launch_at_login);
        assert!(c.privacy.honor_concealed);
        assert!(c.privacy.secret_heuristics);
    }

    #[test]
    fn roundtrip_toml() {
        let c = Config::default();
        let s = c.to_toml().expect("serialize");
        let back = Config::from_toml(&s).expect("deserialize");
        assert_eq!(c, back);
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let s = "[capture]\npoll_interval_ms = 500\n";
        let c = Config::from_toml(s).expect("parse");
        assert_eq!(c.capture.poll_interval_ms, 500);
        // Untouched fields fall back to defaults.
        assert_eq!(c.capture.max_text_bytes, 1_048_576);
        assert_eq!(c.retention.max_items, 10_000);
    }

    #[test]
    fn save_and_load_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("config.toml");
        let mut c = Config::default();
        c.capture.paused = true;
        c.save(&path).expect("save");
        let loaded = Config::load(&path).expect("load");
        assert!(loaded.capture.paused);
    }

    #[test]
    fn load_missing_returns_default() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("does-not-exist.toml");
        let c = Config::load(&path).expect("load");
        assert_eq!(c, Config::default());
    }
}
