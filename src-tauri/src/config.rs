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
        if let Ok(path) = std::env::var("AIT_CONFIG_PATH") {
            return PathBuf::from(path);
        }
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
            Ok(cfg) => match cfg.validate() {
                Ok(()) => cfg,
                Err(e) => {
                    Self::preserve_invalid(path, &e, "invalid");
                    Self::default()
                }
            },
            Err(e) => {
                let suffix = match e.classify() {
                    serde_json::error::Category::Syntax | serde_json::error::Category::Eof => {
                        "corrupt"
                    }
                    serde_json::error::Category::Data | serde_json::error::Category::Io => {
                        "invalid"
                    }
                };
                Self::preserve_invalid(path, &e, suffix);
                Self::default()
            }
        }
    }

    fn preserve_invalid(path: &Path, reason: &dyn std::fmt::Display, suffix: &str) {
        // Corrupt or semantically invalid, NOT missing. Silently reverting to
        // Default would wipe every per-provider pref + sort order + auto_anchor
        // with no trace. Preserve the bad file as a recoverable sibling copy.
        eprintln!(
            "config: {} is invalid ({reason}); keeping a .corrupt-* copy and resetting to defaults",
            path.display()
        );
        let mut backup = path.as_os_str().to_owned();
        backup.push(format!(
            ".{suffix}-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = std::fs::rename(path, std::path::PathBuf::from(backup));
    }

    /// Persist to disk atomically.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::config_path())
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
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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

    #[test]
    fn load_from_semantically_invalid_file_is_preserved_not_used() {
        let dir =
            std::env::temp_dir().join(format!("ait_cfg_invalid_semantic_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.json");
        let cfg = AppConfig {
            poll_seconds: 5,
            ..Default::default()
        };
        std::fs::write(&p, serde_json::to_string(&cfg).unwrap()).unwrap();

        let loaded = AppConfig::load_from(&p);

        assert_eq!(loaded.poll_seconds, 300);
        assert!(!p.exists(), "the invalid config.json was renamed away");
        let preserved = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("config.json.invalid-")
            });
        assert!(
            preserved,
            "an .invalid-* backup of the invalid config must exist"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_reports_write_failure() {
        let dir = std::env::temp_dir().join(format!("ait_cfg_save_dir_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let cfg = AppConfig::default();
        let err = cfg.save_to(&dir).unwrap_err();

        assert!(
            matches!(
                err.kind(),
                std::io::ErrorKind::IsADirectory
                    | std::io::ErrorKind::PermissionDenied
                    | std::io::ErrorKind::AlreadyExists
                    | std::io::ErrorKind::Other
            ),
            "unexpected error kind: {err:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_reports_config_path_write_failure() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("ait_cfg_save_path_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("AIT_CONFIG_PATH", &dir);

        let err = AppConfig::default().save().unwrap_err();

        assert!(
            matches!(
                err.kind(),
                std::io::ErrorKind::IsADirectory
                    | std::io::ErrorKind::PermissionDenied
                    | std::io::ErrorKind::AlreadyExists
                    | std::io::ErrorKind::Other
            ),
            "unexpected error kind: {err:?}"
        );

        std::env::remove_var("AIT_CONFIG_PATH");
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

    // ── Chunk-0 migration gate: pin the CURRENT on-disk config shape ─────────
    //
    // CHUNK-3 MIGRATION TARGET: today `custom_name` + `primary_window` (and
    // `sort_index`) live in the per-PROVIDER `ProviderConfig` slot, indexed by
    // provider — so all accounts of one provider share a single name/window
    // (BUG-2 "rename-all"). Chunk 3 relocates `custom_name`/`primary_window` to a
    // per-`service_id` `accounts: HashMap<String, AccountConfig>` and adds
    // `schema_version`, running a one-time `migrate()` that seeds the accounts
    // map from the old provider slots then clears them. These two tests are the
    // load-bearing data-loss gate: they pin that a realistic OLD-shape config
    // (no `schema_version`, no `accounts`) parses today WITHOUT resetting — so
    // Chunk 3's migration test can prove old prefs are migrated, not wiped.

    /// Characterization: a realistic v0 config (provider-level `custom_name` /
    /// `primary_window` / `sort_index`, an `auto_anchor` map, and NO
    /// `schema_version` / `accounts` fields) loads via the real `load_from`
    /// path WITHOUT tripping the corrupt/invalid preserve-and-reset branch, and
    /// the provider-level `custom_name` is readable at `providers[0].custom_name`.
    /// Pins TODAY's behavior; the relocation is Chunk 3's job.
    #[test]
    fn config_v0_fixture_loads_without_data_loss_today() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let fixture = include_str!("../tests/config_v0_provider_level.json");

        // Write the fixture to a temp path and load it through the production
        // `load_from` (the same branch that would reset a config it can't parse).
        let dir =
            std::env::temp_dir().join(format!("ait_cfg_v0_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, fixture).unwrap();

        let cfg = AppConfig::load_from(&path);

        // NOT reset: if the v0 shape failed to parse, `load_from` would have
        // moved the file aside and returned defaults (poll_seconds 300, all
        // slots default). Proving the real values survived proves no data loss.
        assert_eq!(cfg.poll_seconds, 600, "v0 poll_seconds must survive load");
        assert!(
            path.exists(),
            "a parseable v0 config must NOT be renamed away as corrupt/invalid"
        );

        // The provider-level customizations are readable as-is today.
        assert_eq!(
            cfg.providers[0].custom_name.as_deref(),
            Some("Work Claude"),
            "provider-level custom_name must load from the v0 slot"
        );
        assert_eq!(
            cfg.providers[0].primary_window.as_deref(),
            Some("Weekly"),
            "provider-level primary_window must load from the v0 slot"
        );
        assert_eq!(cfg.providers[0].sort_index, 2);
        assert!(!cfg.providers[2].enabled, "per-provider enabled survives");
        assert_eq!(cfg.providers[2].notify_thresholds, vec![80, 100]);

        // auto_anchor is already correctly keyed per service_id (the shape the
        // migration mirrors) and must round-trip from the v0 file unchanged.
        assert_eq!(
            cfg.auto_anchor.get("stored:zai-1700000000"),
            Some(&true)
        );
        assert_eq!(cfg.auto_anchor.get("auto:codex"), Some(&false));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A realistic current-shape `AppConfig` serializes → deserializes to an
    /// equal value. Pins the round-trip so Chunk 3's reshape can't silently drop
    /// a field. (`ProviderConfig` skips `None` `custom_name`/`primary_window` on
    /// serialize, so we assert the meaningful fields rather than `Debug`-equality.)
    #[test]
    fn appconfig_roundtrips_current_shape() {
        let mut cfg = AppConfig {
            poll_seconds: 450,
            ..Default::default()
        };
        cfg.providers[0].custom_name = Some("Work Claude".into());
        cfg.providers[0].primary_window = Some("Weekly".into());
        cfg.providers[0].sort_index = 2;
        cfg.providers[2].enabled = false;
        cfg.providers[2].notify_thresholds = vec![80, 100];
        cfg.auto_anchor.insert("stored:zai-1".into(), true);

        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(back.poll_seconds, cfg.poll_seconds);
        assert_eq!(back.providers.len(), 6);
        for (a, b) in cfg.providers.iter().zip(back.providers.iter()) {
            assert_eq!(a.enabled, b.enabled);
            assert_eq!(a.custom_name, b.custom_name);
            assert_eq!(a.primary_window, b.primary_window);
            assert_eq!(a.notify_thresholds, b.notify_thresholds);
            assert_eq!(a.sort_index, b.sort_index);
        }
        assert_eq!(back.auto_anchor, cfg.auto_anchor);
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
