//! Codex credential read/write: reuses the Codex CLI's `~/.codex/auth.json`
//! (`AuthDotJson` / `Tokens`). Atomic + owner-only writes via `write_atomic`.

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::providers::ProviderError;
use crate::secrets;

#[derive(Deserialize)]
pub(super) struct AuthDotJson {
    #[serde(default)]
    pub(super) tokens: Option<Tokens>,
}
#[derive(Deserialize)]
pub(super) struct Tokens {
    pub(super) access_token: String,
    #[serde(default)]
    pub(super) refresh_token: Option<String>,
    #[serde(default)]
    pub(super) id_token: Option<String>,
    #[serde(default)]
    pub(super) account_id: Option<String>,
}

pub(super) fn read_auth() -> Result<(Value, Tokens), ProviderError> {
    let v = secrets::read_json_file(&secrets::codex_auth_path())?;
    let a: AuthDotJson = serde_json::from_value(v.clone())
        .map_err(|e| ProviderError::Parse(format!("codex auth: {e}")))?;
    let tokens = a.tokens.ok_or_else(|| {
        ProviderError::NotLoggedIn("Codex not logged in (run `codex login`)".into())
    })?;
    Ok((v, tokens))
}

pub(super) fn write_auth(v: &Value) -> Result<(), ProviderError> {
    write_auth_to(&secrets::codex_auth_path(), v)
}

fn write_auth_to(path: &Path, v: &Value) -> Result<(), ProviderError> {
    let text = serde_json::to_string_pretty(v)
        .map_err(|e| ProviderError::Parse(format!("codex auth write: {e}")))?;
    // Atomic + owner-only: a crash/ENOSPC mid-write must not corrupt the CLI's
    // auth.json — by the time we get here the old refresh token is already
    // rotated away server-side, so a torn file would strand both tools (B-5).
    crate::util::write_atomic(path, text.as_bytes(), Some(0o600))
        .map_err(|e| ProviderError::Network(format!("write {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn write_auth_persists_atomically_and_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ait_codex_auth_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("auth.json");
        let v = serde_json::json!({"tokens":{"access_token":"a","refresh_token":"r"}});

        write_auth_to(&path, &v).unwrap();

        // Round-trips as valid JSON (not torn).
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back["tokens"]["access_token"], "a");
        // Credential file → owner-only (B-5 / consistency with X-1).
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // The temp file is consumed by the rename.
        assert!(!dir.join("auth.json.tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_tokens_from_fixture() {
        let v: Value =
            serde_json::from_str(include_str!("../../../tests/codex_auth_fixture.json")).unwrap();
        let a: AuthDotJson = serde_json::from_value(v).unwrap();
        let tokens = a.tokens.unwrap();
        assert_eq!(tokens.refresh_token.as_deref(), Some("r"));
    }
}
