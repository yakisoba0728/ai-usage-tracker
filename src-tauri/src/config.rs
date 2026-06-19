//! User configuration — persisted to disk. Provider slot order matches the
//! [Claude, Codex, Gemini, Copilot, Cursor, z.ai] canonical order used across
//! the codebase.
use crate::model::Provider;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Per-provider customizable settings. Lives inside `AppConfig.providers`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub enabled: bool,
    /// Override the display name shown on the card / modal title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_name: Option<String>,
    /// Notification thresholds in percent (0–100). Fires a notification when
    /// usage crosses each value. Default: [50, 75, 90, 95, 100].
    #[serde(default = "default_thresholds")]
    pub notify_thresholds: Vec<u8>,
    /// Which window label to surface as the card headline. None = auto-pick
    /// (highest-burn window).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_window: Option<String>,
    /// Sort index for drag-and-drop reordering. Lower = earlier.
    #[serde(default)]
    pub sort_index: i32,
}

fn default_thresholds() -> Vec<u8> {
    vec![50, 75, 90, 95, 100]
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            custom_name: None,
            notify_thresholds: default_thresholds(),
            primary_window: None,
            sort_index: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub poll_seconds: u64,
    /// Indexed by canonical provider order [Claude, Codex, Gemini, Copilot,
    /// Cursor, z.ai].
    pub providers: [ProviderConfig; 6],
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            poll_seconds: 300,
            providers: Default::default(),
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.poll_seconds < 30 {
            return Err(format!(
                "poll_seconds must be >= 30, got {}",
                self.poll_seconds
            ));
        }
        Ok(())
    }

    /// Backwards-compat helper: extract the enabled[] boolean array.
    pub fn enabled_array(&self) -> [bool; 6] {
        let mut arr = [true; 6];
        for (i, p) in self.providers.iter().enumerate() {
            arr[i] = p.enabled;
        }
        arr
    }

    // ── Persistence ────────────────────────────────────────────────────────

    fn config_path() -> PathBuf {
        if let Some(dir) = dirs::config_dir() {
            let app_dir = dir.join("ai-usage-tracker");
            let _ = std::fs::create_dir_all(&app_dir);
            app_dir.join("config.json")
        } else {
            PathBuf::from("config.json")
        }
    }

    /// Load from disk, or return Default if missing / unparseable.
    pub fn load() -> Self {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist to disk atomically.
    pub fn save(&self) {
        let path = Self::config_path();
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }

    /// Resolve the effective display name for a provider (custom override or
    /// the provider's built-in display()).
    pub fn display_name(&self, provider: Provider) -> &str {
        let idx = provider_index(provider);
        if let Some(cfg) = self.providers.get(idx) {
            if let Some(name) = &cfg.custom_name {
                return name;
            }
        }
        provider.display()
    }

    /// Resolve the effective sort key for a provider.
    pub fn sort_key(&self, provider: Provider) -> i32 {
        let idx = provider_index(provider);
        self.providers.get(idx).map(|c| c.sort_index).unwrap_or(0)
    }

    /// Resolve notification thresholds for a provider.
    pub fn thresholds(&self, provider: Provider) -> &[u8] {
        let idx = provider_index(provider);
        self.providers
            .get(idx)
            .map(|c| c.notify_thresholds.as_slice())
            .unwrap_or(&[])
    }
}

/// Canonical index for a Provider in the `providers[6]` array.
pub fn provider_index(p: Provider) -> usize {
    match p {
        Provider::Claude => 0,
        Provider::Codex => 1,
        Provider::Gemini => 2,
        Provider::Copilot => 3,
        Provider::Cursor => 4,
        Provider::Zai => 5,
    }
}

/// Reverse: index → Provider.
pub fn provider_at(idx: usize) -> Provider {
    match idx {
        0 => Provider::Claude,
        1 => Provider::Codex,
        2 => Provider::Gemini,
        3 => Provider::Copilot,
        4 => Provider::Cursor,
        _ => Provider::Zai,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_all_enabled_with_thresholds() {
        let c = AppConfig::default();
        assert_eq!(c.poll_seconds, 300);
        for p in &c.providers {
            assert!(p.enabled);
            assert_eq!(p.notify_thresholds, vec![50, 75, 90, 95, 100]);
        }
        assert!(c.validate().is_ok());
    }

    #[test]
    fn rejects_too_short_interval() {
        let c = AppConfig {
            poll_seconds: 5,
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn enabled_array_roundtrips() {
        let mut c = AppConfig::default();
        c.providers[2].enabled = false;
        let arr = c.enabled_array();
        assert_eq!(arr, [true, true, false, true, true, true]);
    }

    #[test]
    fn display_name_uses_custom_override() {
        let mut c = AppConfig::default();
        c.providers[0].custom_name = Some("My Claude".into());
        assert_eq!(c.display_name(Provider::Claude), "My Claude");
        assert_eq!(c.display_name(Provider::Codex), "Codex");
    }
}
