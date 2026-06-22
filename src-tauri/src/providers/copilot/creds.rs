//! GitHub Copilot credential read: reuse the Copilot CLI's stored OAuth token
//! (macOS Keychain `copilot-cli`, else `~/.copilot/config.json`), or a pasted PAT.

use std::path::PathBuf;

use serde_json::Value;

use crate::providers::ProviderError;
use crate::secrets;

/// Reuse the Copilot CLI's stored OAuth token. The Copilot CLI keeps the token
/// in the OS-native secret store on every platform (macOS Keychain, Windows
/// Credential Manager, Linux Secret Service) under service `copilot-cli`, and
/// only writes the plaintext `<COPILOT_HOME or ~/.copilot>/config.json` as a
/// fallback when no keystore is available. We therefore read the keystore first,
/// then fall back to the file.
pub(super) fn read_copilot_token() -> Result<String, ProviderError> {
    #[cfg(target_os = "macos")]
    {
        // macOS: shell-out service-only lookup (matches how claude-meter reads).
        if let Ok(raw) = secrets::read_macos_keychain("copilot-cli") {
            if let Some(t) = parse_copilot_keystore_value(&raw) {
                return Ok(t);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Windows Credential Manager via the keyring crate.
        if let Some(t) = read_copilot_keyring() {
            return Ok(t);
        }
    }
    // Plaintext config fallback (honors COPILOT_HOME, like Codex's CODEX_HOME).
    let path = copilot_config_path();
    let v = secrets::read_json_file(&path)?;
    v.get("github.com")
        .and_then(|g| g.get("oauth_token"))
        .and_then(|t| t.as_str())
        .map(String::from)
        .ok_or_else(|| {
            ProviderError::NotLoggedIn(
                "Copilot CLI token not found (run `copilot login` or paste a PAT)".into(),
            )
        })
}

/// Parse a Copilot keystore value: either JSON `{github.com:{oauth_token}}` or a
/// raw token string. None if empty / no token present.
fn parse_copilot_keystore_value(raw: &str) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        if let Some(t) = v
            .get("github.com")
            .and_then(|g| g.get("oauth_token"))
            .and_then(|t| t.as_str())
        {
            return Some(t.to_string());
        }
    }
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn copilot_config_path() -> PathBuf {
    std::env::var("COPILOT_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".copilot")
        })
        .join("config.json")
}

/// Read the `copilot-cli` token from the OS secret store on Windows/Linux.
/// The exact account name the Copilot CLI uses is undocumented and differs per
/// platform [needs verification on a real Windows/Linux Copilot install], so we
/// try the most likely candidates; a miss falls through to the plaintext config.
#[cfg(not(target_os = "macos"))]
fn read_copilot_keyring() -> Option<String> {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    // [NEEDS HARDWARE VERIFICATION] On Windows the credential lives in Windows
    // Credential Manager under service "copilot-cli", but the ACCOUNT name the
    // Copilot CLI uses is embedded in its native addon and undocumented. These
    // candidates are a best-effort guess; the `~/.copilot/config.json` file
    // fallback covers the miss. Verify on a real Windows box with `cmdkey /list`.
    let candidates = ["github.com", "copilot-cli", user.as_str()];
    for acct in candidates {
        if acct.is_empty() {
            continue;
        }
        if let Ok(entry) = keyring::Entry::new("copilot-cli", acct) {
            if let Ok(raw) = entry.get_password() {
                if let Some(t) = parse_copilot_keystore_value(&raw) {
                    return Some(t);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_copilot_keystore_value_json_and_raw() {
        // JSON shape the Copilot CLI keystore uses.
        assert_eq!(
            parse_copilot_keystore_value(r#"{"github.com":{"oauth_token":"gho_abc"}}"#).as_deref(),
            Some("gho_abc")
        );
        // Raw token value (trimmed).
        assert_eq!(
            parse_copilot_keystore_value("  gho_raw  ").as_deref(),
            Some("gho_raw")
        );
        // Empty → None.
        assert_eq!(parse_copilot_keystore_value("   "), None);
    }

    #[test]
    fn copilot_config_path_honors_copilot_home() {
        std::env::set_var("COPILOT_HOME", "/tmp/ait_copilot_home");
        assert_eq!(
            copilot_config_path(),
            std::path::PathBuf::from("/tmp/ait_copilot_home/config.json")
        );
        std::env::remove_var("COPILOT_HOME");
    }
}
