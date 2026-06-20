//! App-managed store for manually-added (OAuth / pasted-key) accounts. Secrets
//! (access/refresh/id tokens) are kept in the OS keychain (see keychain.rs);
//! only non-secret metadata is persisted to accounts.json. Auto-detected CLI
//! accounts are NOT stored here — they're rediscovered fresh each poll.

use serde::{Deserialize, Serialize};

use crate::model::Provider;

/// A credential for a manually-added account (in-memory: full secret + metadata).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredCredential {
    pub id: String,
    pub provider: Provider,
    pub label: String, // email or "Account N"
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

/// On-disk record: non-secret metadata only. Secrets live in the OS keychain.
#[derive(Clone, Serialize, Deserialize)]
struct StoredRecord {
    id: String,
    provider: Provider,
    label: String,
    #[serde(default)]
    expires_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
}

/// The secret bundle stored in the OS keychain, keyed by the credential id.
#[derive(Serialize, Deserialize)]
struct SecretBlob {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    accounts: Vec<StoredRecord>,
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

fn read_records() -> Vec<StoredRecord> {
    let path = store_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let parsed: StoreFile = serde_json::from_str(&raw).unwrap_or_default();
    parsed.accounts
}

fn persist_records(records: &[StoredRecord]) {
    let path = store_path();
    let file = StoreFile {
        accounts: records.to_vec(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::write(path, json);
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

fn secret_blob_of(cred: &StoredCredential) -> Result<String, String> {
    serde_json::to_string(&SecretBlob {
        access_token: cred.access_token.clone(),
        refresh_token: cred.refresh_token.clone(),
        id_token: cred.id_token.clone(),
    })
    .map_err(|e| e.to_string())
}

/// Add a credential: secret → keychain, metadata → accounts.json. Returns the
/// assigned id. Errs if the secret cannot be written to the keychain.
pub fn add(mut cred: StoredCredential) -> Result<String, String> {
    if cred.id.is_empty() {
        cred.id = gen_id(&cred.provider);
    }
    let blob = secret_blob_of(&cred)?;
    crate::keychain::store_secret(&cred.id, &blob)?;
    let mut records = read_records();
    records.push(StoredRecord {
        id: cred.id.clone(),
        provider: cred.provider,
        label: cred.label,
        expires_at: cred.expires_at,
        account_id: cred.account_id,
    });
    persist_records(&records);
    Ok(cred.id)
}

pub fn remove(id: &str) -> bool {
    let mut records = read_records();
    let before = records.len();
    records.retain(|r| r.id != id);
    let changed = records.len() != before;
    if changed {
        let _ = crate::keychain::delete_secret(id); // best-effort
        persist_records(&records);
    }
    changed
}

/// Replace a stored credential in place (the refresh path persists rotated
/// tokens). Returns true if the id was found. A keychain write failure is logged
/// but not fatal — the in-memory token is still used for this cycle.
pub fn update(cred: &StoredCredential) -> bool {
    let mut records = read_records();
    let mut found = false;
    for r in records.iter_mut() {
        if r.id == cred.id {
            r.provider = cred.provider;
            r.label = cred.label.clone();
            r.expires_at = cred.expires_at;
            r.account_id = cred.account_id.clone();
            found = true;
            break;
        }
    }
    if found {
        match secret_blob_of(cred) {
            Ok(blob) => {
                if let Err(e) = crate::keychain::store_secret(&cred.id, &blob) {
                    eprintln!(
                        "keychain: failed to persist updated secret for {}: {e}",
                        cred.id
                    );
                }
            }
            Err(e) => eprintln!("keychain: serialize secret failed for {}: {e}", cred.id),
        }
        persist_records(&records);
    }
    found
}

pub fn list() -> Vec<StoredCredential> {
    load()
}

/// Load all stored accounts, reading each secret from the keychain. A record
/// whose keychain secret is missing or unreadable is skipped.
pub fn load() -> Vec<StoredCredential> {
    read_records()
        .into_iter()
        .filter_map(|rec| {
            let blob = crate::keychain::load_secret(&rec.id).ok().flatten()?;
            let secret: SecretBlob = serde_json::from_str(&blob).ok()?;
            Some(StoredCredential {
                id: rec.id,
                provider: rec.provider,
                label: rec.label,
                access_token: secret.access_token,
                refresh_token: secret.refresh_token,
                expires_at: rec.expires_at,
                id_token: secret.id_token,
                account_id: rec.account_id,
            })
        })
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
    fn add_load_roundtrip_keeps_secret_out_of_file() {
        let _g = ENV_LOCK.lock().unwrap();
        let path = temp_path("roundtrip");
        let _ = std::fs::remove_file(&path);
        std::env::set_var("AIT_ACCOUNTS_PATH", &path);

        let id = add(StoredCredential {
            id: String::new(),
            provider: Provider::Codex,
            label: "a@b.c".into(),
            access_token: "SECRET-ACCESS".into(),
            refresh_token: Some("SECRET-REFRESH".into()),
            expires_at: 123,
            id_token: Some("SECRET-ID".into()),
            account_id: Some("acct".into()),
        })
        .unwrap();
        assert!(!id.is_empty());

        // The metadata file must not contain any token material.
        let file = std::fs::read_to_string(&path).unwrap();
        assert!(!file.contains("SECRET-ACCESS"), "file leaked access token");
        assert!(
            !file.contains("SECRET-REFRESH"),
            "file leaked refresh token"
        );
        assert!(!file.contains("access_token"), "file has a token field");
        assert!(file.contains("a@b.c"), "metadata (label) should persist");

        // load() reconstructs the full credential from the keychain.
        let loaded = load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].access_token, "SECRET-ACCESS");
        assert_eq!(loaded[0].refresh_token.as_deref(), Some("SECRET-REFRESH"));
        assert_eq!(loaded[0].id_token.as_deref(), Some("SECRET-ID"));
        assert_eq!(loaded[0].account_id.as_deref(), Some("acct"));

        assert!(remove(&id));
        assert!(load().is_empty());
        std::env::remove_var("AIT_ACCOUNTS_PATH");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn metadata_record_serializes_without_secret_fields() {
        let rec = StoredRecord {
            id: "x".into(),
            provider: Provider::Codex,
            label: "a@b.c".into(),
            expires_at: 7,
            account_id: Some("acct".into()),
        };
        let s = serde_json::to_string(&StoreFile {
            accounts: vec![rec],
        })
        .unwrap();
        assert!(!s.contains("access_token"));
        assert!(!s.contains("refresh_token"));
        assert!(s.contains("\"acct\""));
    }
}
