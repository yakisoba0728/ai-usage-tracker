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
        keychain_add("Claude Code-credentials", &acct, s)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let p = crate::secrets::claude_token_path();
        // Atomic + owner-only: don't leave a concurrent CLI reader a half-written
        // .credentials.json (B-6).
        crate::util::write_atomic(&p, s.as_bytes(), Some(0o600))
            .map_err(|e| ProviderError::Network(format!("write {}: {e}", p.display())))
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
    // without prompting; the account line looks like:  "acct"<blob>="yakisoba".
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().find_map(parse_keychain_acct)
}

/// Parse the `acct` attribute from one `security find-generic-password` output
/// line. Handles the quoted form (`"acct"<blob>="name"`) AND the hex form
/// (`"acct"<blob>=0x<hex>`) that `security` emits when the account contains
/// non-ASCII bytes (e.g. `José` → `0x4A6F73C3A9`, `유저` → `0xEC9CA0ECA080`).
/// `<NULL>` / empty / non-acct lines → None (B-8).
#[cfg(target_os = "macos")]
fn parse_keychain_acct(line: &str) -> Option<String> {
    let after = line.trim().strip_prefix("\"acct\"<blob>=")?;
    if let Some(hex) = after.strip_prefix("0x") {
        // Hex run up to the first non-hex char, decoded as UTF-8 bytes.
        let hex: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if hex.is_empty() || !hex.len().is_multiple_of(2) {
            return None;
        }
        let bytes: Option<Vec<u8>> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect();
        return bytes
            .and_then(|b| String::from_utf8(b).ok())
            .filter(|s| !s.is_empty());
    }
    // Quoted form: `"acct"<blob>="value"`.
    after
        .strip_prefix('"')?
        .strip_suffix('"')
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

/// Add/update a generic-password keychain item, feeding the secret on stdin so
/// it never appears on the argv / process table (X-2). `security ... -w` (no
/// value) prompts for the password and then asks to retype it, so the secret is
/// written to stdin twice; the pipe is closed before we wait, so `security`
/// sees EOF and the call cannot deadlock.
#[cfg(target_os = "macos")]
fn keychain_add(service: &str, account: &str, secret: &str) -> Result<(), ProviderError> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("/usr/bin/security")
        .args(add_password_args(service, account))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ProviderError::Network(format!("security write: {e}")))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::Network("security: no stdin".into()))?;
        let mut write_twice = || -> std::io::Result<()> {
            stdin.write_all(secret.as_bytes())?;
            stdin.write_all(b"\n")?;
            stdin.write_all(secret.as_bytes())?;
            stdin.write_all(b"\n")
        };
        let res = write_twice();
        drop(stdin); // close the pipe so `security` sees EOF before we wait
        res.map_err(|e| ProviderError::Network(format!("security stdin: {e}")))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| ProviderError::Network(format!("security wait: {e}")))?;
    if !out.status.success() {
        return Err(ProviderError::Network(format!(
            "security write: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// Build the `security add-generic-password` argv. The secret is deliberately
/// NOT an argument (X-2): a trailing `-w` with no value makes `security` read
/// the password from stdin (prompted twice), so the token never appears in the
/// process table where any local `ps -ww` could capture it. `-U` updates the
/// existing item in place.
#[cfg(target_os = "macos")]
fn add_password_args(service: &str, account: &str) -> Vec<String> {
    vec![
        "add-generic-password".to_string(),
        "-s".to_string(),
        service.to_string(),
        "-a".to_string(),
        account.to_string(),
        "-U".to_string(),
        // No value after `-w`: security reads the secret from stdin (twice).
        "-w".to_string(),
    ]
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn parse_keychain_acct_handles_quoted_and_hex_forms() {
        assert_eq!(
            parse_keychain_acct("    \"acct\"<blob>=\"yakisoba\""),
            Some("yakisoba".into())
        );
        // hex-encoded non-ASCII accounts (security uses hex for non-printable).
        assert_eq!(
            parse_keychain_acct("    \"acct\"<blob>=0x4A6F73C3A9"),
            Some("José".into())
        );
        assert_eq!(
            parse_keychain_acct("    \"acct\"<blob>=0xEC9CA0ECA080"),
            Some("유저".into())
        );
        // <NULL> / empty / unrelated lines → None.
        assert_eq!(parse_keychain_acct("    \"acct\"<blob>=<NULL>"), None);
        assert_eq!(parse_keychain_acct("    \"acct\"<blob>=\"\""), None);
        assert_eq!(
            parse_keychain_acct("    \"svce\"<blob>=\"Claude Code-credentials\""),
            None
        );
    }

    #[test]
    fn add_password_args_omit_the_secret_and_read_stdin() {
        let args = add_password_args("Claude Code-credentials", "alice");
        // The secret must never be on the argv (the whole point of X-2).
        let secret = "sk-ant-oat01-supersecret";
        assert!(
            !args
                .iter()
                .any(|a| a.contains(secret) || a.contains("sk-ant")),
            "no secret-shaped argument may be present: {args:?}"
        );
        // `-w` is the LAST arg with no value → security reads the secret on stdin.
        assert_eq!(args.last().map(String::as_str), Some("-w"));
        assert!(
            args.iter().any(|a| a == "-U"),
            "update-in-place flag present"
        );
        assert!(args.iter().any(|a| a == "add-generic-password"));
        assert!(args.iter().any(|a| a == "Claude Code-credentials"));
        assert!(args.iter().any(|a| a == "alice"));
    }

    /// Exercises the REAL production `keychain_add` (stdin-fed secret) against a
    /// throwaway keychain item and reads it back, proving the stdin-twice piping
    /// actually stores the secret. Ignored by default — it touches the login
    /// keychain — run with `cargo test -- --ignored keychain_add_round_trips`.
    #[test]
    #[ignore = "touches the login keychain; run explicitly with --ignored"]
    fn keychain_add_round_trips_a_secret_via_stdin() {
        let svc = format!("ait-x2-probe-{}", std::process::id());
        let secret = r#"{"claudeAiOauth":{"accessToken":"sk-ant-probe123","refreshToken":"rt"}}"#;

        keychain_add(&svc, "ait-test-acct", secret).expect("keychain_add should succeed");

        let out = std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", &svc, "-w"])
            .output()
            .unwrap();
        let got = String::from_utf8_lossy(&out.stdout);
        // cleanup before asserting so a failure still removes the probe item.
        let _ = std::process::Command::new("/usr/bin/security")
            .args(["delete-generic-password", "-s", &svc])
            .output();
        assert_eq!(
            got.trim_end_matches('\n'),
            secret,
            "stdin-fed secret must round-trip exactly"
        );
    }
}
