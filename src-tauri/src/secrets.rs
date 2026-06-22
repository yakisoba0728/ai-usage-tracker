//! Cross-platform credential reads. Claude on macOS reads the Keychain via
//! `/usr/bin/security` (the stored account is the OS username, not empty, so the
//! `keyring` crate's `Entry::new(service, "")` cannot be used). Everywhere else
//! (and for Codex/Gemini) we read JSON files from disk.

use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum SecretsError {
    #[error("credential not found: {0}")]
    NotFound(String),
    #[error("credential read failed: {0}")]
    Read(String),
    #[error("credential parse failed: {0}")]
    Parse(String),
}

fn dirs_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn claude_token_path() -> PathBuf {
    let base = std::env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_home().join(".claude"));
    base.join(".credentials.json")
}

pub fn codex_home() -> PathBuf {
    std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_home().join(".codex"))
}

pub fn codex_auth_path() -> PathBuf {
    codex_home().join("auth.json")
}

/// Cursor's local state DB (SQLite). None if the platform path doesn't exist.
pub fn cursor_state_db() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let p =
            dirs_home().join("Library/Application Support/Cursor/User/globalStorage/state.vscdb");
        p.exists().then_some(p)
    }
    #[cfg(target_os = "linux")]
    {
        let p = dirs_home().join(".config/Cursor/User/globalStorage/state.vscdb");
        p.exists().then_some(p)
    }
    #[cfg(target_os = "windows")]
    {
        // Base = `AIT_CURSOR_DATA_DIR` override > `dirs::data_dir()` (Roaming
        // `%APPDATA%`, where the VS Code-fork Cursor keeps globalStorage). The
        // app-specific override (same category as `CODEX_HOME`/`CLAUDE_CONFIG_DIR`)
        // exists so a hermetic test can redirect the base: `dirs::data_dir()`
        // resolves via the Win32 Known Folder API and ignores `%APPDATA%`, so it
        // can't be redirected by env alone. With the override UNSET this is
        // byte-identical to the previous `dirs::data_dir()?.join(...)` behavior.
        let base = std::env::var("AIT_CURSOR_DATA_DIR")
            .map(PathBuf::from)
            .ok()
            .or_else(dirs::data_dir)?;
        let p = base.join("Cursor/User/globalStorage/state.vscdb");
        p.exists().then_some(p)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

/// Read + JSON-parse a file from disk.
pub fn read_json_file(path: &PathBuf) -> Result<Value, SecretsError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| SecretsError::NotFound(format!("{}: {e}", path.display())))?;
    serde_json::from_str::<Value>(&raw)
        .map_err(|e| SecretsError::Parse(format!("{}: {e}", path.display())))
}

pub(crate) fn normalize_claude_oauth_token(raw: &str) -> Option<String> {
    let token = raw
        .trim()
        .strip_prefix("Bearer ")
        .unwrap_or_else(|| raw.trim())
        .trim();
    if token.starts_with("sk-ant-") || token.starts_with("oat-") {
        Some(token.to_string())
    } else {
        None
    }
}

pub fn parse_claude_creds_raw(raw: &str, source: &str) -> Result<Value, SecretsError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(SecretsError::Parse(format!(
            "{source}: empty Claude credentials"
        )));
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(v) => Ok(v),
        Err(json_err) => normalize_claude_oauth_token(trimmed)
            .map(|access_token| {
                serde_json::json!({
                    "claudeAiOauth": {
                        "accessToken": access_token,
                    },
                })
            })
            .ok_or_else(|| SecretsError::Parse(format!("{source}: {json_err}"))),
    }
}

/// Read the macOS Keychain generic-password value for `service` (any account)
/// by shelling out to `/usr/bin/security` — matches how Claude Code/claude-meter
/// store and read the `Claude Code-credentials` item.
pub fn read_macos_keychain(service: &str) -> Result<String, SecretsError> {
    let out = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service, "-w"])
        .output()
        .map_err(|e| SecretsError::Read(format!("spawn security: {e}")))?;
    if !out.status.success() {
        return Err(SecretsError::NotFound(format!(
            "{service}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    // `security -w` appends a trailing newline; trim it at the source so the
    // return value is a clean credential regardless of caller (B-10).
    String::from_utf8(out.stdout)
        .map(|s| s.trim_end_matches('\n').to_string())
        .map_err(|e| SecretsError::Read(format!("keychain value not utf-8: {e}")))
}

/// Read Claude Code credentials as JSON. macOS → Keychain; otherwise the file.
pub fn read_claude_creds_json() -> Result<Value, SecretsError> {
    if let Ok(raw) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        if !raw.trim().is_empty() {
            return parse_claude_creds_raw(&raw, "CLAUDE_CODE_OAUTH_TOKEN");
        }
    }

    #[cfg(target_os = "macos")]
    {
        match read_macos_keychain("Claude Code-credentials")
            .and_then(|raw| parse_claude_creds_raw(&raw, "claude keychain"))
        {
            Ok(v) => Ok(v),
            Err(keychain_err) => match read_json_file(&claude_token_path()) {
                Ok(v) => Ok(v),
                Err(file_err) => {
                    if matches!(keychain_err, SecretsError::NotFound(_)) {
                        Err(file_err)
                    } else {
                        Err(keychain_err)
                    }
                }
            },
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        read_json_file(&claude_token_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_json_file_parses_and_errors_on_missing() {
        let tmp = std::env::temp_dir().join("ait_secrets_test.json");
        std::fs::write(&tmp, r#"{"k": 1}"#).unwrap();
        let v = read_json_file(&tmp).unwrap();
        assert_eq!(v["k"], 1);

        let missing = std::env::temp_dir().join("ait_does_not_exist_xyz.json");
        assert!(matches!(
            read_json_file(&missing),
            Err(SecretsError::NotFound(_))
        ));
    }

    #[test]
    fn codex_path_honors_env() {
        std::env::set_var("CODEX_HOME", "/tmp/ait_codex_home");
        assert_eq!(
            codex_auth_path(),
            PathBuf::from("/tmp/ait_codex_home/auth.json")
        );
        std::env::remove_var("CODEX_HOME");
    }

    /// Windows-only (runs on the Windows CI leg; compiles out on the macOS dev
    /// loop). Pins the Windows `cursor_state_db()` path resolution before the
    /// rewrite reshuffles `secrets.rs`, so a Windows regression isn't invisible
    /// to the macOS loop. Asserts BOTH the derived suffix
    /// (`Cursor/User/globalStorage/state.vscdb` under the `%APPDATA%`-class base)
    /// AND the existence gate (`None` when the DB file is absent). Hermetic via
    /// the `AIT_CURSOR_DATA_DIR` redirect — `dirs::data_dir()` resolves via the
    /// Win32 Known Folder API and ignores `%APPDATA%`, so we must not write into
    /// the real Roaming AppData.
    #[cfg(target_os = "windows")]
    #[test]
    fn cursor_state_db_resolves_windows_localappdata() {
        use std::sync::Mutex;
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let base = std::env::temp_dir().join(format!("ait_cursor_db_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let db = base.join("Cursor/User/globalStorage/state.vscdb");
        std::env::set_var("AIT_CURSOR_DATA_DIR", &base);

        // Absent → None (the `.exists()` gate; pins that a missing DB is not a
        // phantom path).
        assert_eq!(
            cursor_state_db(),
            None,
            "no state.vscdb under the base must resolve to None"
        );

        // Present → Some(base + the canonical Cursor suffix).
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        std::fs::write(&db, b"x").unwrap();
        assert_eq!(
            cursor_state_db(),
            Some(db.clone()),
            "the Windows cursor DB must resolve to <APPDATA>/Cursor/User/globalStorage/state.vscdb"
        );

        std::env::remove_var("AIT_CURSOR_DATA_DIR");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn parse_claude_creds_raw_accepts_json_and_oauth_token() {
        let json = parse_claude_creds_raw(
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-json"}}"#,
            "test-json",
        )
        .unwrap();
        assert_eq!(json["claudeAiOauth"]["accessToken"], "sk-ant-oat01-json");

        let raw = parse_claude_creds_raw("sk-ant-oat01-raw", "test-token").unwrap();
        assert_eq!(raw["claudeAiOauth"]["accessToken"], "sk-ant-oat01-raw");
    }
}
