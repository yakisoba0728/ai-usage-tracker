//! App-managed store for manually-added (OAuth) accounts. Auto-detected CLI
//! accounts are NOT stored here — they're discovered fresh each poll. This file
//! only holds accounts the user added via the in-app device-code login.

use serde::{Deserialize, Serialize};

use crate::model::Provider;

/// A credential for a manually-added account, persisted to disk.
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

#[derive(Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    accounts: Vec<StoredCredential>,
}

fn store_path() -> std::path::PathBuf {
    let base = dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(".config"))
        .join("ai-usage-tracker");
    let _ = std::fs::create_dir_all(&base);
    base.join("accounts.json")
}

pub fn load() -> Vec<StoredCredential> {
    let path = store_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let parsed: StoreFile = serde_json::from_str(&raw).unwrap_or_default();
    parsed.accounts
}

fn persist(accounts: &[StoredCredential]) {
    let path = store_path();
    let file = StoreFile {
        accounts: accounts.to_vec(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&file) {
        let _ = std::fs::write(path, json);
    }
}

/// Add a credential; returns its assigned id.
pub fn add(mut cred: StoredCredential) -> String {
    let mut accounts = load();
    if cred.id.is_empty() {
        cred.id = format!(
            "{}-{:x}",
            serde_json::to_string(&cred.provider)
                .unwrap_or_default()
                .trim_matches('"'),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
    }
    let id = cred.id.clone();
    accounts.push(cred);
    persist(&accounts);
    id
}

pub fn remove(id: &str) -> bool {
    let mut accounts = load();
    let before = accounts.len();
    accounts.retain(|a| a.id != id);
    let changed = accounts.len() != before;
    if changed {
        persist(&accounts);
    }
    changed
}

/// Replace a stored credential in place (used by the refresh path to persist
/// rotated tokens). Returns true if the id was found and updated.
pub fn update(cred: &StoredCredential) -> bool {
    let mut accounts = load();
    let mut changed = false;
    for a in accounts.iter_mut() {
        if a.id == cred.id {
            *a = cred.clone();
            changed = true;
            break;
        }
    }
    if changed {
        persist(&accounts);
    }
    changed
}

pub fn list() -> Vec<StoredCredential> {
    load()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ait_store_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        // dirs::config_dir honors XDG_CONFIG_HOME on Linux, not macOS. For the
        // test we instead exercise persist/load directly via a known path.
        dir.join("accounts.json")
    }

    #[test]
    fn roundtrips_through_json() {
        let cred = StoredCredential {
            id: "x".into(),
            provider: Provider::Codex,
            label: "a@b.c".into(),
            access_token: "at".into(),
            refresh_token: Some("rt".into()),
            expires_at: 123,
            id_token: Some("jwt".into()),
            account_id: Some("acct".into()),
        };
        let file = StoreFile {
            accounts: vec![cred.clone()],
        };
        let s = serde_json::to_string(&file).unwrap();
        let back: StoreFile = serde_json::from_str(&s).unwrap();
        assert_eq!(back.accounts.len(), 1);
        assert_eq!(back.accounts[0].label, "a@b.c");
        assert_eq!(back.accounts[0].account_id.as_deref(), Some("acct"));
        let _ = temp_store();
    }

    #[test]
    fn add_assigns_unique_id() {
        let cred = StoredCredential {
            id: String::new(),
            provider: Provider::Gemini,
            label: "g".into(),
            access_token: "at".into(),
            refresh_token: None,
            expires_at: 0,
            id_token: None,
            account_id: None,
        };
        let id = add(cred);
        assert!(!id.is_empty());
        assert!(remove(&id));
    }
}
