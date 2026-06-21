//! OS keychain storage for the app's OWN stored-account secrets, via the
//! `keyring` crate (macOS Keychain / Windows Credential Manager / Linux Secret
//! Service). Each manually-added account's secret bundle (access/refresh/id
//! tokens, serialized as JSON) is stored under service "ai-usage-tracker" with
//! the account = the credential id. Non-secret metadata stays in accounts.json
//! (see store.rs). This keeps long-lived tokens out of a plaintext file at rest.
//!
//! The backend is indirected behind a trait so tests run against an in-memory
//! store. (The `keyring` crate's own mock builds an independent credential per
//! `Entry`, so it can't model the shared store the real backends provide; an
//! in-memory backend lets store.rs's migration/roundtrip logic be tested.)

#[cfg(not(test))]
const SERVICE: &str = "ai-usage-tracker";

trait Backend: Send + Sync {
    fn store(&self, id: &str, blob: &str) -> Result<(), String>;
    fn load(&self, id: &str) -> Result<Option<String>, String>;
    fn delete(&self, id: &str) -> Result<(), String>;
}

/// Production backend: the OS-native credential store via `keyring`. Only built
/// in non-test builds; tests use the in-memory backend below.
#[cfg(not(test))]
struct Keyring;
#[cfg(not(test))]
static KEYRING: Keyring = Keyring;

#[cfg(not(test))]
impl Backend for Keyring {
    fn store(&self, id: &str, blob: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, id)
            .map_err(|e| e.to_string())?
            .set_password(blob)
            .map_err(|e| e.to_string())
    }
    fn load(&self, id: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(SERVICE, id).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(SERVICE, id).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(test))]
fn backend() -> &'static dyn Backend {
    &KEYRING
}

/// In test builds the backend is a process-wide in-memory map, so unit tests
/// never touch (or prompt) the real OS keychain. It also supports per-id write
/// failure injection so store.rs can test the keychain-write-failure path.
#[cfg(test)]
mod test_backend {
    use super::Backend;
    use std::collections::{HashMap, HashSet};
    use std::sync::{Mutex, OnceLock};

    pub struct Mem {
        map: Mutex<HashMap<String, String>>,
        fail_ids: Mutex<HashSet<String>>,
    }

    impl Backend for Mem {
        fn store(&self, id: &str, blob: &str) -> Result<(), String> {
            if self.fail_ids.lock().unwrap().contains(id) {
                return Err("injected keychain store failure".into());
            }
            self.map.lock().unwrap().insert(id.into(), blob.into());
            Ok(())
        }
        fn load(&self, id: &str) -> Result<Option<String>, String> {
            Ok(self.map.lock().unwrap().get(id).cloned())
        }
        fn delete(&self, id: &str) -> Result<(), String> {
            self.map.lock().unwrap().remove(id);
            Ok(())
        }
    }

    static MEM: OnceLock<Mem> = OnceLock::new();

    pub fn mem() -> &'static Mem {
        MEM.get_or_init(|| Mem {
            map: Mutex::new(HashMap::new()),
            fail_ids: Mutex::new(HashSet::new()),
        })
    }

    /// Make `store(id, ..)` fail (or stop failing) for one id. Keyed by id so
    /// concurrent tests using other ids are unaffected.
    pub fn set_fail(id: &str, fail: bool) {
        let mut s = mem().fail_ids.lock().unwrap();
        if fail {
            s.insert(id.into());
        } else {
            s.remove(id);
        }
    }
}

#[cfg(test)]
fn backend() -> &'static dyn Backend {
    test_backend::mem()
}

/// Test hook: toggle keychain write failure for a single account id.
#[cfg(test)]
pub(crate) fn fail_store_for(id: &str, fail: bool) {
    test_backend::set_fail(id, fail);
}

/// Persist the secret blob for an account, overwriting any existing value.
pub fn store_secret(account_id: &str, blob: &str) -> Result<(), String> {
    backend().store(account_id, blob)
}

/// Load the secret blob for an account. `Ok(None)` when there is no entry.
pub fn load_secret(account_id: &str) -> Result<Option<String>, String> {
    backend().load(account_id)
}

/// Delete an account's secret. A missing entry is treated as success.
pub fn delete_secret(account_id: &str) -> Result<(), String> {
    backend().delete(account_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_store_load_delete() {
        let id = "keychain-test-roundtrip";
        store_secret(id, "secret-blob-1").unwrap();
        assert_eq!(load_secret(id).unwrap().as_deref(), Some("secret-blob-1"));
        store_secret(id, "secret-blob-2").unwrap(); // overwrite
        assert_eq!(load_secret(id).unwrap().as_deref(), Some("secret-blob-2"));
        delete_secret(id).unwrap();
        assert_eq!(load_secret(id).unwrap(), None);
    }

    #[test]
    fn load_missing_is_none_and_delete_missing_is_ok() {
        let id = "keychain-test-missing";
        assert_eq!(load_secret(id).unwrap(), None);
        delete_secret(id).unwrap(); // deleting a non-existent entry is not an error
    }
}
