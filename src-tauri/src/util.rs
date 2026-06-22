//! Small shared helpers with no provider/domain coupling.

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Uppercase the first character of `s`, leaving the rest unchanged. Returns an
/// empty string for empty input. Used for plan/tier labels across providers.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Best-effort redaction for technical error strings before they cross IPC or
/// land in UI toasts. Keeps status/context text, removes common token forms and
/// email addresses.
pub fn scrub_sensitive_text(input: &str) -> String {
    let mut out = input.to_string();
    for key in [
        "access_token",
        "refresh_token",
        "id_token",
        "sessionKey",
        "session_key",
        "authorization",
        "cookie",
    ] {
        out = redact_keyed_value(&out, key);
    }
    out = redact_prefixed_tokens(
        &out,
        &["sk-ant-", "gho_", "ghu_", "github_pat_", "ya29.", "Bearer "],
    );
    redact_emails(&out)
}

fn redact_prefixed_tokens(input: &str, prefixes: &[&str]) -> String {
    let mut out = String::with_capacity(input.len());
    let mut idx = 0;
    while idx < input.len() {
        let rest = &input[idx..];
        if let Some(prefix) = prefixes.iter().find(|prefix| rest.starts_with(**prefix)) {
            out.push_str("[redacted]");
            idx += prefix.len();
            while idx < input.len() {
                let ch = input[idx..].chars().next().unwrap();
                if is_secret_delimiter(ch) {
                    break;
                }
                idx += ch.len_utf8();
            }
            continue;
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        idx += ch.len_utf8();
    }
    out
}

fn redact_keyed_value(input: &str, key: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let key_lower = key.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(rel) = lower[cursor..].find(&key_lower) {
        let start = cursor + rel;
        let mut idx = start + key.len();
        idx = skip_ascii_ws(input, idx);
        if char_at(input, idx) == Some('"') {
            idx += 1;
            idx = skip_ascii_ws(input, idx);
        }
        if !matches!(char_at(input, idx), Some(':') | Some('=')) {
            out.push_str(&input[cursor..start + key.len()]);
            cursor = start + key.len();
            continue;
        }
        idx += 1;
        idx = skip_ascii_ws(input, idx);
        if matches!(char_at(input, idx), Some('"') | Some('\'')) {
            let quote = char_at(input, idx).unwrap();
            idx += quote.len_utf8();
            out.push_str(&input[cursor..idx]);
            out.push_str("[redacted]");
            while idx < input.len() {
                let ch = input[idx..].chars().next().unwrap();
                if ch == quote {
                    break;
                }
                idx += ch.len_utf8();
            }
            cursor = idx;
        } else {
            out.push_str(&input[cursor..idx]);
            out.push_str("[redacted]");
            while idx < input.len() {
                let ch = input[idx..].chars().next().unwrap();
                if is_secret_delimiter(ch) {
                    break;
                }
                idx += ch.len_utf8();
            }
            cursor = idx;
        }
    }
    out.push_str(&input[cursor..]);
    out
}

fn redact_emails(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut token = String::new();

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '%' | '+' | '-' | '@') {
            token.push(ch);
        } else {
            flush_email_token(&mut out, &mut token);
            out.push(ch);
        }
    }
    flush_email_token(&mut out, &mut token);
    out
}

fn flush_email_token(out: &mut String, token: &mut String) {
    if looks_like_email(token) {
        out.push_str("[redacted]");
    } else {
        out.push_str(token);
    }
    token.clear();
}

fn looks_like_email(token: &str) -> bool {
    let Some((local, domain)) = token.split_once('@') else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn char_at(input: &str, idx: usize) -> Option<char> {
    input.get(idx..)?.chars().next()
}

fn skip_ascii_ws(input: &str, mut idx: usize) -> usize {
    while let Some(ch) = char_at(input, idx) {
        if !ch.is_ascii_whitespace() {
            break;
        }
        idx += ch.len_utf8();
    }
    idx
}

fn is_secret_delimiter(ch: char) -> bool {
    ch.is_ascii_whitespace()
        || matches!(
            ch,
            '"' | '\'' | ',' | ';' | '&' | '<' | '>' | '{' | '}' | '[' | ']' | ')' | '('
        )
}

/// Atomically write `contents` to `path`: write a unique sibling temp file then
/// rename it over the target, so a crash or a concurrent reader never observes a
/// torn file. On Unix, `mode` (e.g. `0o600` for secrets) is applied to the temp
/// file before the rename, so the final file is never momentarily world-readable
/// — the rename preserves the inode's permission bits. Off Unix, `mode` is not
/// portable via `std`; Windows files inherit the parent directory's ACL.
pub fn write_atomic(path: &Path, contents: &[u8], mode: Option<u32>) -> io::Result<()> {
    let tmp = unique_tmp_path(path);
    if let Err(e) = write_tmp(&tmp, contents, mode) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

fn unique_tmp_path(path: &Path) -> std::path::PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    tmp.push(format!(".tmp-{}-{counter}-{nanos}", std::process::id()));
    std::path::PathBuf::from(tmp)
}

#[cfg(unix)]
fn write_tmp(tmp: &Path, contents: &[u8], mode: Option<u32>) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    if let Some(m) = mode {
        opts.mode(m);
    }
    let mut f = opts.open(tmp)?;
    f.write_all(contents)?;
    if let Some(m) = mode {
        // Re-apply after creation so the requested owner-only mode is not relaxed
        // by the process umask.
        f.set_permissions(std::fs::Permissions::from_mode(m))?;
    }
    f.sync_all()
}

#[cfg(not(unix))]
fn write_tmp(tmp: &Path, contents: &[u8], _mode: Option<u32>) -> io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp)?;
    f.write_all(contents)?;
    f.sync_all()
}

/// Best-effort: restrict a directory to owner-only (`0o700`) on Unix so a
/// plaintext credential store under it (e.g. the `~/.config` fallback) is not
/// readable by other local accounts. Off Unix this is a no-op: Rust `std` has no
/// portable ACL API, and Windows config directories normally inherit per-user
/// profile ACLs.
#[cfg(unix)]
pub fn set_dir_private(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
pub fn set_dir_private(_dir: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capitalizes_first_char_only() {
        assert_eq!(capitalize("pro"), "Pro");
        assert_eq!(capitalize("max 20x"), "Max 20x");
        assert_eq!(capitalize("A"), "A");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn write_atomic_writes_and_overwrites() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("ait_wa_{}.bin", std::process::id()));
        let _ = std::fs::remove_file(&p);
        write_atomic(&p, b"first", None).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"first");
        // Overwrites existing content; the temp file must be gone after rename.
        write_atomic(&p, b"second-longer", None).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"second-longer");
        let mut tmp = p.clone().into_os_string();
        tmp.push(".tmp");
        assert!(
            !std::path::Path::new(&tmp).exists(),
            "temp file consumed by rename"
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn write_atomic_does_not_consume_existing_fixed_tmp_sibling() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("ait_wa_collision_{}.bin", std::process::id()));
        let mut fixed_tmp = p.clone().into_os_string();
        fixed_tmp.push(".tmp");
        let fixed_tmp = std::path::PathBuf::from(fixed_tmp);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(&fixed_tmp);
        std::fs::write(&fixed_tmp, b"external-temp").unwrap();

        write_atomic(&p, b"new-target", None).unwrap();

        assert_eq!(std::fs::read(&p).unwrap(), b"new-target");
        assert_eq!(
            std::fs::read(&fixed_tmp).unwrap(),
            b"external-temp",
            "an existing fixed .tmp sibling belongs to another writer/run"
        );
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(&fixed_tmp);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_applies_owner_only_mode() {
        use std::os::unix::fs::PermissionsExt;
        let p = std::env::temp_dir().join(format!("ait_wa_mode_{}.bin", std::process::id()));
        let _ = std::fs::remove_file(&p);
        write_atomic(&p, b"secret", Some(0o600)).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "credential file must be owner-only");
        let _ = std::fs::remove_file(&p);
    }

    #[cfg(unix)]
    #[test]
    fn set_dir_private_restricts_to_owner() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("ait_dir_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        // Loosen first so the assertion proves set_dir_private did the tightening.
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755));
        set_dir_private(&d);
        let mode = std::fs::metadata(&d).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let _ = std::fs::remove_dir_all(&d);
    }
}
