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

pub(super) fn resolve_codex_auto() -> Result<(String, Option<String>), ProviderError> {
    let v = crate::secrets::read_json_file(&crate::secrets::codex_auth_path())?;
    let tokens = v
        .get("tokens")
        .ok_or_else(|| ProviderError::NotLoggedIn("codex auth.json: no tokens".into()))?;
    let access = tokens
        .get("access_token")
        .and_then(|s| s.as_str())
        .ok_or_else(|| ProviderError::NotLoggedIn("codex: no access_token".into()))?;
    let account_id = tokens
        .get("account_id")
        .and_then(|s| s.as_str())
        .map(String::from);
    Ok((access.to_string(), account_id))
}

/// Resolve the stored credential whose UI id is `service_id` (`stored:<id>`).
pub(super) fn resolve_stored(service_id: &str) -> Option<crate::store::StoredCredential> {
    crate::store::find_by_service_id(service_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_zai_auto_reads_env_or_errors() {
        std::env::set_var("ZAI_API_KEY", "zk-env-123");
        assert_eq!(resolve_zai_auto().unwrap(), "zk-env-123");
        std::env::remove_var("ZAI_API_KEY");
        assert!(resolve_zai_auto().is_err());
    }
}
