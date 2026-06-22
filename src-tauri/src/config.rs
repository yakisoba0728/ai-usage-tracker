//! User configuration — persisted to disk. Provider slot order matches the
//! [Claude, Codex, Gemini, Copilot, Cursor, z.ai] canonical order used across
//! the codebase.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The current on-disk config schema version. Bumped whenever `migrate()` gains
/// a new step so the migration runs exactly once per upgrade.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Per-provider customizable settings. Lives inside `AppConfig.providers`,
/// indexed by canonical provider order. These are genuinely provider-level —
/// every account of a provider shares them. Per-ACCOUNT name/window moved out to
/// `AppConfig.accounts` (keyed by service_id) to fix BUG-2 "rename-all".
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub enabled: bool,
    /// Notification thresholds in percent (0–100). Fires a notification when
    /// usage crosses each value. Default: [50, 75, 90, 95, 100].
    #[serde(default = "default_thresholds")]
    pub notify_thresholds: Vec<u8>,
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
            notify_thresholds: default_thresholds(),
            sort_index: 0,
        }
    }
}

/// Per-ACCOUNT customizable settings, keyed by service_id (`auto:<provider>` or
/// `stored:<id>`) in `AppConfig.accounts`. Two accounts of the same provider
/// have independent entries — this is the fix for BUG-2 (rename-all).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountConfig {
    /// Override the display name shown on the card / modal title. None = use the
    /// canonical provider label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_name: Option<String>,
    /// Which window label to surface as the card headline. None = auto-pick
    /// (highest-burn window).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_window: Option<String>,
}

impl AccountConfig {
    /// True when this entry carries no customization at all — used to avoid
    /// persisting empty placeholder entries.
    fn is_empty(&self) -> bool {
        self.custom_name.is_none() && self.primary_window.is_none()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    /// On-disk schema version. Missing (older configs) → 0 via serde default,
    /// which triggers the one-time `migrate()`; a fresh `Default` config is
    /// already current (`CURRENT_SCHEMA_VERSION`) and never re-migrates.
    #[serde(default)]
    pub schema_version: u32,
    pub poll_seconds: u64,
    /// Indexed by canonical provider order [Claude, Codex, Gemini, Copilot,
    /// Cursor, z.ai].
    pub providers: [ProviderConfig; 6],
    /// Per-`service_id` account settings (display name + pinned window). Keyed
    /// by `auto:<provider>` / `stored:<id>`; default empty.
    #[serde(default)]
    pub accounts: HashMap<String, AccountConfig>,
    /// Per-`service_id` opt-in for auto window-anchoring (default: empty = OFF).
    #[serde(default)]
    pub auto_anchor: HashMap<String, bool>,
    /// Launch the app at login (FEAT-4). The OS login-item registration is the
    /// source of truth; this flag mirrors the user's intent so `.setup` can
    /// reconcile a manually-removed login item back on. Default: false.
    #[serde(default)]
    pub launch_at_login: bool,
    /// Whether the background GitHub-release update notifier runs (FEAT-5).
    /// Default: true (`default_true`) so old configs without the field still
    /// opt in.
    #[serde(default = "default_true")]
    pub auto_update_check: bool,
    /// The last release version we fired an "update available" notification for,
    /// so the 24h check doesn't re-notify the same release every interval
    /// (FEAT-5). `None` until the first notification. Stored without a leading
    /// `v`. Empty/absent in old configs via serde default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_notified_version: Option<String>,
}

/// serde `default = ...` helper: a bool field that defaults to `true` when the
/// key is absent from an older on-disk config (serde's built-in default is
/// `false`).
fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            // A fresh config is already at the current schema — it must NOT be
            // re-migrated (that path only runs for older, lower-versioned files).
            schema_version: CURRENT_SCHEMA_VERSION,
            poll_seconds: 300,
            providers: Default::default(),
            accounts: HashMap::new(),
            auto_anchor: HashMap::new(),
            launch_at_login: false,
            auto_update_check: true,
            last_notified_version: None,
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
        // Two-stage parse so the one-time migration can lift fields that no
        // longer exist on the typed struct (provider-level custom_name /
        // primary_window → per-account `accounts` map). The classification of a
        // failure is preserved (syntax/EOF = "corrupt"; semantic = "invalid").
        let raw: serde_json::Value = match serde_json::from_str(&text) {
            Ok(value) => value,
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
                return Self::default();
            }
        };

        // Lift legacy per-provider fields BEFORE deserializing into the new shape
        // (a serde-ignored field would otherwise be silently dropped on parse).
        let lifted = Self::lift_legacy_provider_fields(&raw);

        let mut cfg: Self = match serde_json::from_value(raw) {
            Ok(cfg) => cfg,
            Err(e) => {
                // Reaching here means valid JSON but a shape/type mismatch
                // (Data) — semantically invalid, preserved as ".invalid-*".
                Self::preserve_invalid(path, &e, "invalid");
                return Self::default();
            }
        };

        cfg.migrate(lifted);

        match cfg.validate() {
            Ok(()) => cfg,
            Err(e) => {
                Self::preserve_invalid(path, &e, "invalid");
                Self::default()
            }
        }
    }

    /// Extract the OLD per-provider `custom_name`/`primary_window` from the raw
    /// parsed JSON, paired with the provider slot index. Returns the
    /// auto-service-id keyed `AccountConfig`s the migration should seed. Empty
    /// when there's nothing to migrate (no such fields present).
    fn lift_legacy_provider_fields(raw: &serde_json::Value) -> Vec<(String, AccountConfig)> {
        let mut lifted = Vec::new();
        let Some(slots) = raw.get("providers").and_then(|p| p.as_array()) else {
            return lifted;
        };
        for (idx, slot) in slots.iter().enumerate() {
            let custom_name = slot
                .get("custom_name")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            let primary_window = slot
                .get("primary_window")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            if custom_name.is_none() && primary_window.is_none() {
                continue;
            }
            let Some(provider) = crate::model::provider_at_index(idx) else {
                continue;
            };
            lifted.push((
                crate::model::auto_service_id(provider),
                AccountConfig {
                    custom_name,
                    primary_window,
                },
            ));
        }
        lifted
    }

    /// One-time, idempotent schema migration. Runs only when the loaded
    /// `schema_version` is below the current target. v0→v1 seeds the per-account
    /// `accounts` map from the legacy per-provider fields lifted from the raw
    /// JSON, never overwriting an account entry the user already has.
    fn migrate(&mut self, legacy_accounts: Vec<(String, AccountConfig)>) {
        if self.schema_version >= CURRENT_SCHEMA_VERSION {
            return;
        }
        // v0 → v1: per-provider name/window → per-account `accounts`.
        for (service_id, legacy) in legacy_accounts {
            // Don't clobber a real entry that somehow already exists; merge the
            // legacy fields into any blank slot instead.
            let entry = self.accounts.entry(service_id).or_default();
            if entry.custom_name.is_none() {
                entry.custom_name = legacy.custom_name;
            }
            if entry.primary_window.is_none() {
                entry.primary_window = legacy.primary_window;
            }
        }
        // Drop any entry that ended up with no customization (defensive).
        self.accounts.retain(|_, a| !a.is_empty());
        self.schema_version = CURRENT_SCHEMA_VERSION;
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

    // ── Per-account settings migration gate (FEAT-1 / BUG-2) ────────────────
    //
    // Before this chunk `custom_name` + `primary_window` lived in the per-PROVIDER
    // `ProviderConfig` slot, indexed by provider — so all accounts of one provider
    // shared a single name/window (BUG-2 "rename-all"). They now live in a
    // per-`service_id` `accounts: HashMap<String, AccountConfig>`; `schema_version`
    // gates a one-time `migrate()` that seeds the accounts map from the old
    // provider slots. These two tests are the load-bearing data-loss gate.

    /// MIGRATION GATE (D2.1): a realistic v0 config (provider-level `custom_name` /
    /// `primary_window` on the Claude slot, `sort_index`, an `auto_anchor` map, and
    /// NO `schema_version` / `accounts` fields) loads via the real `load_from`
    /// path WITHOUT tripping the corrupt/invalid preserve-and-reset branch, and the
    /// old provider-level `custom_name`/`primary_window` are MIGRATED into
    /// `accounts[auto_service_id(Claude)]` — not lost, not reset. Per-provider
    /// `enabled` / `notify_thresholds` / `sort_index` stay on the provider slot.
    #[test]
    fn config_v0_provider_level_fields_migrate_into_accounts_map() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let fixture = include_str!("../tests/config_v0_provider_level.json");

        // Write the fixture to a temp path and load it through the production
        // `load_from` (the same branch that would reset a config it can't parse).
        let dir = std::env::temp_dir().join(format!("ait_cfg_v0_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, fixture).unwrap();

        let cfg = AppConfig::load_from(&path);

        // NOT reset: if the v0 shape failed to parse, `load_from` would have moved
        // the file aside and returned defaults (poll_seconds 300, empty accounts).
        // Proving the real values survived proves no data loss.
        assert_eq!(cfg.poll_seconds, 600, "v0 poll_seconds must survive load");
        assert!(
            path.exists(),
            "a parseable v0 config must NOT be renamed away as corrupt/invalid"
        );

        // The old provider-level customizations are MIGRATED to the Claude
        // (slot 0) auto service id — this is the whole point of the migration.
        let claude_id = crate::model::auto_service_id(crate::model::Provider::Claude);
        let acct = cfg
            .accounts
            .get(&claude_id)
            .expect("migration must seed accounts[auto:claude] from the v0 Claude slot");
        assert_eq!(
            acct.custom_name.as_deref(),
            Some("Work Claude"),
            "provider-level custom_name must migrate into accounts[auto:claude]"
        );
        assert_eq!(
            acct.primary_window.as_deref(),
            Some("Weekly"),
            "provider-level primary_window must migrate into accounts[auto:claude]"
        );

        // No spurious account entries for slots that had no custom_name/window.
        assert_eq!(
            cfg.accounts.len(),
            1,
            "only the one customized slot should produce an account entry"
        );

        // Per-provider settings stay on the provider slot (NOT migrated).
        assert_eq!(cfg.providers[0].sort_index, 2);
        assert!(!cfg.providers[2].enabled, "per-provider enabled survives");
        assert_eq!(cfg.providers[2].notify_thresholds, vec![80, 100]);

        // The migration bumps schema_version so it runs exactly once.
        assert_eq!(cfg.schema_version, 1, "migration must bump schema_version");

        // auto_anchor is already correctly keyed per service_id and must
        // round-trip from the v0 file unchanged.
        assert_eq!(cfg.auto_anchor.get("stored:zai-1700000000"), Some(&true));
        assert_eq!(cfg.auto_anchor.get("auto:codex"), Some(&false));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A config already at schema_version 1 is NEVER re-migrated, even if a stray
    /// legacy provider-level field is still present in the JSON: the user's
    /// current `accounts` map wins and the legacy field is simply ignored (it has
    /// no field on the typed struct). Guards against a double-migration clobber.
    #[test]
    fn already_migrated_config_is_not_re_migrated() {
        let raw = serde_json::json!({
            "schema_version": 1,
            "poll_seconds": 300,
            "providers": [
                // Stray legacy field on the Claude slot — must be ignored because
                // schema_version is already current.
                { "enabled": true, "custom_name": "STALE", "notify_thresholds": [50], "sort_index": 0 },
                { "enabled": true, "notify_thresholds": [50], "sort_index": 1 },
                { "enabled": true, "notify_thresholds": [50], "sort_index": 2 },
                { "enabled": true, "notify_thresholds": [50], "sort_index": 3 },
                { "enabled": true, "notify_thresholds": [50], "sort_index": 4 },
                { "enabled": true, "notify_thresholds": [50], "sort_index": 5 }
            ],
            "accounts": { "auto:claude": { "custom_name": "Current" } },
            "auto_anchor": {}
        });
        let lifted = AppConfig::lift_legacy_provider_fields(&raw);
        let mut cfg: AppConfig = serde_json::from_value(raw).unwrap();
        cfg.migrate(lifted);

        // The legacy "STALE" name must NOT overwrite the user's current entry.
        assert_eq!(
            cfg.accounts.get("auto:claude").unwrap().custom_name.as_deref(),
            Some("Current"),
            "an already-migrated config must keep its current account name"
        );
        assert_eq!(cfg.accounts.len(), 1);
    }

    /// During a real v0→v1 migration, a provider whose slot carries a legacy name
    /// must NOT clobber an `accounts` entry the same config already happens to
    /// carry for that service id — existing fields win, legacy only fills blanks.
    #[test]
    fn migration_does_not_clobber_a_preexisting_account_entry() {
        let raw = serde_json::json!({
            // No schema_version (v0) → migrate runs.
            "poll_seconds": 300,
            "providers": [
                { "enabled": true, "custom_name": "Legacy Claude", "primary_window": "Weekly", "notify_thresholds": [50], "sort_index": 0 },
                { "enabled": true, "notify_thresholds": [50], "sort_index": 1 },
                { "enabled": true, "notify_thresholds": [50], "sort_index": 2 },
                { "enabled": true, "notify_thresholds": [50], "sort_index": 3 },
                { "enabled": true, "notify_thresholds": [50], "sort_index": 4 },
                { "enabled": true, "notify_thresholds": [50], "sort_index": 5 }
            ],
            // The user already has a name on auto:claude; only the window is blank.
            "accounts": { "auto:claude": { "custom_name": "Kept Name" } },
            "auto_anchor": {}
        });
        let lifted = AppConfig::lift_legacy_provider_fields(&raw);
        let mut cfg: AppConfig = serde_json::from_value(raw).unwrap();
        cfg.migrate(lifted);

        let acct = cfg.accounts.get("auto:claude").unwrap();
        assert_eq!(
            acct.custom_name.as_deref(),
            Some("Kept Name"),
            "an existing custom_name must NOT be overwritten by the legacy slot"
        );
        assert_eq!(
            acct.primary_window.as_deref(),
            Some("Weekly"),
            "but a blank field is filled from the legacy slot"
        );
        assert_eq!(cfg.schema_version, 1);
    }

    /// ISOLATION GATE (D2.2): two distinct accounts of the SAME provider (Claude)
    /// — an auto:claude and a stored:abc — carry independent `custom_name`s that
    /// survive a save→load round-trip. This is the regression net for BUG-2: a
    /// rename of one account must NOT touch the other.
    #[test]
    fn per_account_custom_names_are_independent_across_save_load() {
        let auto_id = crate::model::auto_service_id(crate::model::Provider::Claude);
        let stored_id = crate::model::stored_service_id("abc");

        let mut cfg = AppConfig::default();
        cfg.accounts.insert(
            auto_id.clone(),
            AccountConfig {
                custom_name: Some("Personal Claude".into()),
                primary_window: None,
            },
        );
        cfg.accounts.insert(
            stored_id.clone(),
            AccountConfig {
                custom_name: Some("Work Claude".into()),
                primary_window: Some("Weekly".into()),
            },
        );

        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(
            back.accounts.get(&auto_id).and_then(|a| a.custom_name.as_deref()),
            Some("Personal Claude"),
            "auto:claude name survives independently"
        );
        assert_eq!(
            back.accounts.get(&stored_id).and_then(|a| a.custom_name.as_deref()),
            Some("Work Claude"),
            "stored:abc name survives independently (NOT overwritten by the auto rename)"
        );
        assert_eq!(
            back.accounts.get(&stored_id).and_then(|a| a.primary_window.as_deref()),
            Some("Weekly"),
        );
        // The auto account has no primary_window; the stored one's must not leak.
        assert!(back.accounts.get(&auto_id).unwrap().primary_window.is_none());
    }

    /// A realistic current-shape `AppConfig` (new accounts map + schema_version)
    /// serializes → deserializes to an equal value. Pins the round-trip so a
    /// later reshape can't silently drop a field.
    #[test]
    fn appconfig_roundtrips_current_shape() {
        let mut cfg = AppConfig {
            poll_seconds: 450,
            ..Default::default()
        };
        cfg.accounts.insert(
            "auto:claude".into(),
            AccountConfig {
                custom_name: Some("Work Claude".into()),
                primary_window: Some("Weekly".into()),
            },
        );
        cfg.providers[0].sort_index = 2;
        cfg.providers[2].enabled = false;
        cfg.providers[2].notify_thresholds = vec![80, 100];
        cfg.auto_anchor.insert("stored:zai-1".into(), true);

        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(back.poll_seconds, cfg.poll_seconds);
        assert_eq!(back.schema_version, cfg.schema_version);
        assert_eq!(back.providers.len(), 6);
        for (a, b) in cfg.providers.iter().zip(back.providers.iter()) {
            assert_eq!(a.enabled, b.enabled);
            assert_eq!(a.notify_thresholds, b.notify_thresholds);
            assert_eq!(a.sort_index, b.sort_index);
        }
        let a = back.accounts.get("auto:claude").unwrap();
        assert_eq!(a.custom_name.as_deref(), Some("Work Claude"));
        assert_eq!(a.primary_window.as_deref(), Some("Weekly"));
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

    // ── FEAT-4 / FEAT-5 new config fields (D2 round-trip + old-config defaults) ──

    /// `launch_at_login` defaults false, `auto_update_check` defaults TRUE, and
    /// `last_notified_version` round-trips. An OLDER config missing all three
    /// fields must still load (serde field defaults), with `auto_update_check`
    /// defaulting to true via `default_true` — NOT serde's built-in `false`.
    #[test]
    fn feat4_5_fields_default_and_roundtrip() {
        let def = AppConfig::default();
        assert!(!def.launch_at_login, "launch_at_login defaults off");
        assert!(def.auto_update_check, "auto_update_check defaults ON");
        assert!(def.last_notified_version.is_none());

        let cfg = AppConfig {
            launch_at_login: true,
            auto_update_check: false,
            last_notified_version: Some("0.2.0".into()),
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(back.launch_at_login);
        assert!(!back.auto_update_check);
        assert_eq!(back.last_notified_version.as_deref(), Some("0.2.0"));

        // An old config that predates these fields still loads, and the
        // update-check defaults to ON (the `default_true` helper) rather than
        // serde's bare `false`.
        let mut v = serde_json::to_value(AppConfig::default()).unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.remove("launch_at_login");
        obj.remove("auto_update_check");
        obj.remove("last_notified_version");
        let old: AppConfig = serde_json::from_value(v).unwrap();
        assert!(!old.launch_at_login, "missing launch_at_login → false");
        assert!(
            old.auto_update_check,
            "missing auto_update_check must default to TRUE (default_true), not serde's false"
        );
        assert!(old.last_notified_version.is_none());
    }
}
