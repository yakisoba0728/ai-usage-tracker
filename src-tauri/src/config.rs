//! User configuration — persisted to disk. Provider slot order matches the
//! [Claude, Codex, Gemini, Copilot, Cursor, z.ai] canonical order used across
//! the codebase.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    /// Per-`service_id` opt-in for auto window-anchoring (default: empty = OFF).
    #[serde(default)]
    pub auto_anchor: HashMap<String, bool>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            poll_seconds: 300,
            providers: Default::default(),
            auto_anchor: HashMap::new(),
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
            // Keep the app config dir owner-only (shared with the plaintext token
            // store under the same dir; see store.rs / X-1).
            crate::util::set_dir_private(&app_dir);
            app_dir.join("config.json")
        } else {
            PathBuf::from("config.json")
        }
    }

    /// Load from disk. Missing → defaults (expected first run). A file that
    /// exists but fails to parse is preserved (see `load_from`) rather than
    /// silently discarded.
    pub fn load() -> Self {
        Self::load_from(&Self::config_path())
    }

    fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default(); // missing → defaults (expected first run)
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                // Corrupt, NOT missing (broken hand-edit, wrong-typed value, or a
                // `providers` array of length != 6). Silently reverting to Default
                // would wipe every per-provider pref + sort order + auto_anchor with
                // no trace. Preserve the bad file as a recoverable .corrupt-* copy
                // and log, then fall back to defaults (B-7).
                eprintln!(
                    "config: {} is corrupt ({e}); keeping a .corrupt-* copy and resetting to defaults",
                    path.display()
                );
                let mut backup = path.as_os_str().to_owned();
                backup.push(format!(".corrupt-{}", chrono::Utc::now().timestamp()));
                let _ = std::fs::rename(path, std::path::PathBuf::from(backup));
                Self::default()
            }
        }
    }

    /// Persist to disk atomically.
    pub fn save(&self) {
        let _ = self.save_to(&Self::config_path());
    }

    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        // Atomic + owner-only, via the shared helper (X-1 consistency).
        crate::util::write_atomic(path, text.as_bytes(), Some(0o600))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_from_missing_file_returns_default() {
        let p = std::env::temp_dir().join(format!("ait_cfg_missing_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        assert_eq!(AppConfig::load_from(&p).poll_seconds, 300);
    }

    #[test]
    fn load_from_parses_valid_file() {
        let p = std::env::temp_dir().join(format!("ait_cfg_valid_{}.json", std::process::id()));
        let cfg = AppConfig {
            poll_seconds: 900,
            ..Default::default()
        };
        std::fs::write(&p, serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(AppConfig::load_from(&p).poll_seconds, 900);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn load_from_corrupt_file_is_preserved_not_silently_reset() {
        let dir = std::env::temp_dir().join(format!("ait_cfg_corrupt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.json");
        std::fs::write(&p, "{ this is not valid json").unwrap();

        let cfg = AppConfig::load_from(&p);

        // Corrupt → defaults (B-7), but the bad file must NOT be silently
        // discarded — it is moved aside as a recoverable .corrupt-* sibling.
        assert_eq!(cfg.poll_seconds, 300);
        assert!(!p.exists(), "the corrupt config.json was renamed away");
        let preserved = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("config.json.corrupt-")
            });
        assert!(
            preserved,
            "a .corrupt-* backup of the bad config must exist"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn save_to_writes_owner_only_parseable_config() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!("ait_cfg_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let cfg = AppConfig {
            poll_seconds: 123,
            ..Default::default()
        };

        cfg.save_to(&path).unwrap();

        // Round-trips (atomic, not torn).
        let back: AppConfig =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.poll_seconds, 123);
        // Owner-only (shares the dir with the plaintext token store; X-1).
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        let _ = std::fs::remove_file(&path);
    }

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
    fn auto_anchor_defaults_empty_and_roundtrips() {
        let mut c = AppConfig::default();
        assert!(c.auto_anchor.is_empty());
        c.auto_anchor.insert("stored:zai-1".into(), true);
        let json = serde_json::to_string(&c).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.auto_anchor.get("stored:zai-1"), Some(&true));
        // Old configs without auto_anchor still load via serde's field default.
        let mut v = serde_json::to_value(AppConfig::default()).unwrap();
        v.as_object_mut().unwrap().remove("auto_anchor");
        let old: AppConfig = serde_json::from_value(v).unwrap(); // must succeed (not unwrap_or_default)
        assert!(old.auto_anchor.is_empty());
    }
}
