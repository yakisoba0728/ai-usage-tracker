//! App-managed store for manually-added (OAuth / pasted-key) accounts. The full
//! credential — including access/refresh/id tokens — is persisted to a plaintext
//! `accounts.json` in the app config dir.
//!
//! Secrets were previously kept in the OS keychain (keyring v3), but that was
//! reverted: the unsigned dev/release build's ad-hoc signature changes on every
//! rebuild, which invalidated the keychain item ACL and made macOS prompt for the
//! login password on every poll. Plain JSON has no such prompt. Tokens still
//! never cross IPC — `list_accounts` masks to non-secret metadata — they are
//! simply stored in plaintext at rest now. Auto-detected CLI accounts are NOT
//! stored here; they're rediscovered fresh each poll.

use serde::{Deserialize, Serialize};

use crate::model::Provider;

/// Serializes account mutations so an overlapping manual `refresh_now` + the
/// poll loop can't interleave a read-modify-write of accounts.json (lost update).
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A credential for a manually-added account: full secret + metadata. Persisted
/// verbatim to accounts.json.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredCredential {
    pub id: String,
    pub provider: Provider,
    pub label: String, // email or "Account N"
    /// `default` so an older keychain-era record (metadata only, no token) still
    /// deserializes — `load()` then skips it as unusable rather than failing the
    /// whole file parse.
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: i64, // epoch ms; 0 = unknown
    #[serde(default)]
    pub id_token: Option<String>, // Codex: plan/email
    #[serde(default)]
    pub account_id: Option<String>, // Codex: chatgpt-account-id
}

#[derive(Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    accounts: Vec<StoredCredential>,
}

fn store_path() -> std::path::PathBuf {
    // Test/override hook so unit tests don't touch the real accounts.json.
    if let Ok(p) = std::env::var("AIT_ACCOUNTS_PATH") {
        return std::path::PathBuf::from(p);
    }
    let base = dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("ai-usage-tracker");
    let _ = std::fs::create_dir_all(&base);
    base.join("accounts.json")
}

fn read_accounts() -> Vec<StoredCredential> {
    let path = store_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let parsed: StoreFile = serde_json::from_str(&raw).unwrap_or_default();
    parsed.accounts
}

fn persist_accounts(accounts: &[StoredCredential]) {
    let path = store_path();
    let file = StoreFile {
        accounts: accounts.to_vec(),
    };
    let Ok(json) = serde_json::to_string_pretty(&file) else {
        return;
    };
    // Write to a sibling temp file then atomically rename over the target, so a
    // concurrent reader never observes a half-written accounts.json.
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn gen_id(provider: &Provider) -> String {
    format!(
        "{}-{:x}",
        serde_json::to_string(provider)
            .unwrap_or_default()
            .trim_matches('"'),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    )
}

/// Add a credential to accounts.json. Returns the assigned id.
pub fn add(mut cred: StoredCredential) -> Result<String, String> {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if cred.id.is_empty() {
        cred.id = gen_id(&cred.provider);
    }
    let id = cred.id.clone();
    let mut accounts = read_accounts();
    accounts.push(cred);
    persist_accounts(&accounts);
    Ok(id)
}

pub fn remove(id: &str) -> bool {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut accounts = read_accounts();
    let before = accounts.len();
    accounts.retain(|c| c.id != id);
    let changed = accounts.len() != before;
    if changed {
        persist_accounts(&accounts);
    }
    changed
}

/// Replace a stored credential in place (the refresh path persists rotated
/// tokens). Returns `true` if the id was found and the updated record written.
/// The whole file is rewritten atomically, so a rotated token and its metadata
/// can never land out of sync.
pub fn update(cred: &StoredCredential) -> bool {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut accounts = read_accounts();
    let Some(idx) = accounts.iter().position(|c| c.id == cred.id) else {
        return false; // unknown id; nothing to update
    };
    accounts[idx] = cred.clone();
    persist_accounts(&accounts);
    true
}

/// Build a rotated credential, preserving `id`/`provider`/`label`/`account_id`.
/// A `None` `refresh_token`/`id_token` keeps the existing one (providers rotate
/// these inconsistently — Google/OpenAI may omit the refresh token, and only
/// Codex returns a fresh id_token). The caller passes the already-computed
/// `expires_at` (epoch ms; 0 = unknown).
pub fn rotate_credential(
    cred: &StoredCredential,
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_at: i64,
) -> StoredCredential {
    StoredCredential {
        id: cred.id.clone(),
        provider: cred.provider,
        label: cred.label.clone(),
        access_token,
        refresh_token: refresh_token.or_else(|| cred.refresh_token.clone()),
        expires_at,
        id_token: id_token.or_else(|| cred.id_token.clone()),
        account_id: cred.account_id.clone(),
    }
}

pub fn list() -> Vec<StoredCredential> {
    load()
}

/// Load all stored accounts. A record with an empty access token (e.g. a stale
/// metadata-only record left behind by an older keychain-backed build) is
/// skipped rather than surfaced as a broken account.
pub fn load() -> Vec<StoredCredential> {
    read_accounts()
        .into_iter()
        .filter(|c| !c.access_token.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // AIT_ACCOUNTS_PATH is process-global, so serialize the file-touching tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ait_store_{tag}_{}.json", std::process::id()))
    }

    #[test]
    fn add_load_roundtrip_persists_full_credential() {
        let _g = ENV_LOCK.lock().unwrap();
        let path = temp_path("roundtrip");
        let _ = std::fs::remove_file(&path);
        std::env::set_var("AIT_ACCOUNTS_PATH", &path);

        let id = add(StoredCredential {
            id: String::new(),
            provider: Provider::Codex,
            label: "a@b.c".into(),
            access_token: "ACCESS-1".into(),
            refresh_token: Some("REFRESH-1".into()),
            expires_at: 123,
            id_token: Some("ID-1".into()),
            account_id: Some("acct".into()),
        })
        .unwrap();
        assert!(!id.is_empty());

        // Tokens are now persisted in the plaintext file (the deliberate
        // post-keychain behavior), alongside the metadata.
        let file = std::fs::read_to_string(&path).unwrap();
        assert!(
            file.contains("ACCESS-1"),
            "access token should persist to file"
        );
        assert!(file.contains("REFRESH-1"), "refresh token should persist");
        assert!(file.contains("a@b.c"), "label metadata should persist");

        // load() reconstructs the full credential from the file.
        let loaded = load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].access_token, "ACCESS-1");
        assert_eq!(loaded[0].refresh_token.as_deref(), Some("REFRESH-1"));
        assert_eq!(loaded[0].id_token.as_deref(), Some("ID-1"));
        assert_eq!(loaded[0].account_id.as_deref(), Some("acct"));
        assert_eq!(loaded[0].expires_at, 123);

        assert!(remove(&id));
        assert!(load().is_empty());
        std::env::remove_var("AIT_ACCOUNTS_PATH");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_skips_tokenless_stale_records() {
        // A record left by an older keychain-backed build has metadata but no
        // access token in the file. It must be skipped, not surfaced as broken.
        let _g = ENV_LOCK.lock().unwrap();
        let path = temp_path("stale");
        std::fs::write(
            &path,
            r#"{"accounts":[{"id":"old-1","provider":"gemini","label":"x","expires_at":0}]}"#,
        )
        .unwrap();
        std::env::set_var("AIT_ACCOUNTS_PATH", &path);

        assert!(load().is_empty(), "a tokenless stale record is skipped");

        std::env::remove_var("AIT_ACCOUNTS_PATH");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn update_replaces_credential_in_place() {
        let _g = ENV_LOCK.lock().unwrap();
        let path = temp_path("update");
        let _ = std::fs::remove_file(&path);
        std::env::set_var("AIT_ACCOUNTS_PATH", &path);

        let id = add(StoredCredential {
            id: "upd-1".into(),
            provider: Provider::Codex,
            label: "a@b.c".into(),
            access_token: "ACCESS-V1".into(),
            refresh_token: Some("REFRESH-V1".into()),
            expires_at: 100,
            id_token: None,
            account_id: None,
        })
        .unwrap();

        let rotated = StoredCredential {
            id: id.clone(),
            provider: Provider::Codex,
            label: "a@b.c".into(),
            access_token: "ACCESS-V2".into(),
            refresh_token: Some("REFRESH-V2".into()),
            expires_at: 999_999,
            id_token: None,
            account_id: None,
        };
        assert!(update(&rotated), "update finds and replaces the record");

        let loaded = load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].access_token, "ACCESS-V2");
        assert_eq!(loaded[0].refresh_token.as_deref(), Some("REFRESH-V2"));
        assert_eq!(loaded[0].expires_at, 999_999);

        // Updating an unknown id is a no-op returning false.
        let mut unknown = rotated.clone();
        unknown.id = "does-not-exist".into();
        assert!(!update(&unknown));

        assert!(remove(&id));
        std::env::remove_var("AIT_ACCOUNTS_PATH");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn concurrent_adds_do_not_lose_records() {
        let _g = ENV_LOCK.lock().unwrap();
        let path = temp_path("concurrent");
        let _ = std::fs::remove_file(&path);
        std::env::set_var("AIT_ACCOUNTS_PATH", &path);

        let handles: Vec<_> = (0..16)
            .map(|i| {
                std::thread::spawn(move || {
                    add(StoredCredential {
                        id: format!("conc-{i}"),
                        provider: Provider::Codex,
                        label: format!("user{i}"),
                        access_token: format!("a{i}"),
                        refresh_token: None,
                        expires_at: 0,
                        id_token: None,
                        account_id: None,
                    })
                    .unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        // load() must also succeed (no torn/half-written file).
        let loaded = load();
        assert_eq!(loaded.len(), 16, "no records lost under concurrent adds");

        for i in 0..16 {
            remove(&format!("conc-{i}"));
        }
        std::env::remove_var("AIT_ACCOUNTS_PATH");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rotate_credential_preserves_identity_and_keeps_old_optionals_when_none() {
        let cred = StoredCredential {
            id: "rot-1".into(),
            provider: Provider::Gemini,
            label: "me@x.com".into(),
            access_token: "old-at".into(),
            refresh_token: Some("old-rt".into()),
            expires_at: 1,
            id_token: Some("old-id".into()),
            account_id: Some("acct".into()),
        };
        // None keeps the existing refresh/id tokens.
        let kept = rotate_credential(&cred, "new-at".into(), None, None, 999);
        assert_eq!(kept.id, "rot-1");
        assert_eq!(kept.provider, Provider::Gemini);
        assert_eq!(kept.label, "me@x.com");
        assert_eq!(kept.account_id.as_deref(), Some("acct"));
        assert_eq!(kept.access_token, "new-at");
        assert_eq!(kept.refresh_token.as_deref(), Some("old-rt"));
        assert_eq!(kept.id_token.as_deref(), Some("old-id"));
        assert_eq!(kept.expires_at, 999);
        // Some rotates them.
        let rotated = rotate_credential(
            &cred,
            "n2".into(),
            Some("new-rt".into()),
            Some("new-id".into()),
            5,
        );
        assert_eq!(rotated.refresh_token.as_deref(), Some("new-rt"));
        assert_eq!(rotated.id_token.as_deref(), Some("new-id"));
    }
}
