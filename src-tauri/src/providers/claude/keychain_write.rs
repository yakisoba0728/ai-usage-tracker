//! Platform plumbing for writing Claude Code's refreshed credentials back to
//! where the CLI reads them (macOS Keychain via `/usr/bin/security`, else the
//! `~/.claude/.credentials.json` file). Split out of `super`.

use crate::providers::ProviderError;

/// Write the serialized credentials blob `s` back to the CLI's store.
pub(super) fn write_creds(s: &str) -> Result<(), ProviderError> {
    #[cfg(target_os = "macos")]
    {
        // Match the account of the EXISTING entry so `add-generic-password -U`
        // updates it in place. Claude Code writes under the OS username
        // (verified live: acct == $USER), but if a pre-existing entry lives
        // under a different account (shared machine, renamed user), blindly
        // using $USER would fork into a duplicate that `secrets::read_macos_keychain`
        // (which reads any account) might not surface. Fall back to $USER on
        // first write when no entry exists yet.
        let acct = existing_keychain_account()
            .unwrap_or_else(|| std::env::var("USER").unwrap_or_default());
        let out = std::process::Command::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-s",
                "Claude Code-credentials",
                "-a",
                &acct,
                "-w",
                s,
                "-U",
            ])
            .output()
            .map_err(|e| ProviderError::Network(format!("security write: {e}")))?;
        if !out.status.success() {
            return Err(ProviderError::Network(format!(
                "security write: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let p = crate::secrets::claude_token_path();
        std::fs::write(&p, s)
            .map_err(|e| ProviderError::Network(format!("write {}: {e}", p.display())))?;
        Ok(())
    }
}

/// Best-effort `acct` attribute of the existing `Claude Code-credentials`
/// keychain item — i.e. the exact entry `secrets::read_macos_keychain` reads.
/// None if there is no item or `security` can't run (caller falls back to $USER).
#[cfg(target_os = "macos")]
fn existing_keychain_account() -> Option<String> {
    let out = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", "Claude Code-credentials"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // `security find-generic-password -s X` (no -w/-g) prints attributes
    // without prompting; the account line looks like:  "acct"<blob>="yakisoba"
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().find_map(|l| {
        let l = l.trim();
        let after = l.strip_prefix("\"acct\"<blob>=\"")?;
        after
            .strip_suffix('"')
            .map(str::to_string)
            .filter(|s| !s.is_empty())
    })
}
