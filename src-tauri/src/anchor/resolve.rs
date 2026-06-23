//! Credential resolution for the anchor dispatcher: turn a UI `service_id` into
//! the bearer (and account id) a send body needs. `auto:*` arms read env / the
//! CLI's on-disk auth; `stored:<id>` looks up the keychain-backed store.
//! (Claude has no auto resolver — it is session-key only, so `resolve_claude_auto`
//! is gone.)

use crate::providers::ProviderError;

/// Resolve the z.ai API key from the environment.
pub(super) fn resolve_zai_auto() -> Result<String, ProviderError> {
    match std::env::var("ZAI_API_KEY") {
        Ok(k) if !k.trim().is_empty() => Ok(k.trim().to_string()),
        _ => Err(ProviderError::NotLoggedIn("z.ai API key not set".into())),
    }
}

#[cfg(test)]
fn resolve_codex_auto_for_test() -> Result<(String, Option<String>), ProviderError> {
    let v = crate::secrets::read_json_file(&crate::secrets::codex_auth_path())?;
    let tokens = v
        .get("tokens")
        .ok_or_else(|| ProviderError::NotLoggedIn("codex auth.json: no tokens".into()))?;
    let access = tokens
        .get("access_token")
        .and_then(|s| s.as_str())
        .ok_or_else(|| ProviderError::NotLoggedIn("codex: no access_token".into()))?;
    let needs_refresh = crate::jwt::jwt_exp(access)
        .map(|exp| exp <= chrono::Utc::now().timestamp() + 300)
        .unwrap_or(false);
    let has_refresh = tokens
        .get("refresh_token")
        .and_then(|s| s.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if needs_refresh && !has_refresh {
        return Err(ProviderError::Expired(
            "Codex token expired or is near expiry and auth.json has no refresh_token".into(),
        ));
    }
    let account_id = tokens
        .get("account_id")
        .and_then(|s| s.as_str())
        .map(String::from)
        .or_else(|| {
            tokens
                .get("id_token")
                .and_then(|s| s.as_str())
                .and_then(|jwt| crate::jwt::codex_identity(jwt).1)
        });
    Ok((access.to_string(), account_id))
}

/// Resolve the stored credential whose UI id is `service_id` (`stored:<id>`).
pub(super) fn resolve_stored(service_id: &str) -> Option<crate::store::StoredCredential> {
    crate::store::find_by_service_id(service_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use std::sync::Mutex;

    static CODEX_HOME_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn jwt_with_claims(claims: serde_json::Value) -> String {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        format!("hdr.{payload}.sig")
    }

    fn write_codex_auth(tag: &str, auth: serde_json::Value) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ait_anchor_codex_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("auth.json"), serde_json::to_vec(&auth).unwrap()).unwrap();
        dir
    }

    #[test]
    fn resolve_zai_auto_reads_env_or_errors() {
        std::env::set_var("ZAI_API_KEY", "zk-env-123");
        assert_eq!(resolve_zai_auto().unwrap(), "zk-env-123");
        std::env::remove_var("ZAI_API_KEY");
        assert!(resolve_zai_auto().is_err());
    }

    #[test]
    fn resolve_codex_auto_derives_account_id_from_id_token() {
        let _g = CODEX_HOME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let id_token = jwt_with_claims(serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "acct-from-id-token" },
        }));
        let dir = write_codex_auth(
            "id_token",
            serde_json::json!({
                "tokens": {
                    "access_token": "access",
                    "refresh_token": "refresh",
                    "id_token": id_token
                }
            }),
        );
        std::env::set_var("CODEX_HOME", &dir);

        let (_, account_id) = resolve_codex_auto_for_test().unwrap();

        assert_eq!(account_id.as_deref(), Some("acct-from-id-token"));

        std::env::remove_var("CODEX_HOME");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_codex_auto_rejects_expiring_auth_without_refresh_token() {
        let _g = CODEX_HOME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let access_token = jwt_with_claims(serde_json::json!({
            "exp": chrono::Utc::now().timestamp() + 60,
        }));
        let dir = write_codex_auth(
            "expiring_no_refresh",
            serde_json::json!({
                "tokens": {
                    "access_token": access_token
                }
            }),
        );
        std::env::set_var("CODEX_HOME", &dir);

        let err = resolve_codex_auto_for_test()
            .expect_err("expiring Codex auto auth must not use stale bearer");

        assert!(matches!(err, ProviderError::Expired(_)), "got {err:?}");

        std::env::remove_var("CODEX_HOME");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
