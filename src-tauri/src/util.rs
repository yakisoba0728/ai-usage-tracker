//! Small shared helpers with no provider/domain coupling.

use std::io;
use std::path::Path;

/// Uppercase the first character of `s`, leaving the rest unchanged. Returns an
/// empty string for empty input. Used for plan/tier labels across providers.
pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Atomically write `contents` to `path`: write a sibling `<path>.tmp` then
/// rename it over the target, so a crash or a concurrent reader never observes a
/// torn file. On Unix, `mode` (e.g. `0o600` for secrets) is applied to the temp
/// file before the rename, so the final file is never momentarily world-readable
/// — the rename preserves the inode's permission bits. `mode` is ignored off Unix.
pub fn write_atomic(path: &Path, contents: &[u8], mode: Option<u32>) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    write_tmp(&tmp, contents, mode)?;
    std::fs::rename(&tmp, path)
}

#[cfg(unix)]
fn write_tmp(tmp: &Path, contents: &[u8], mode: Option<u32>) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    if let Some(m) = mode {
        opts.mode(m);
    }
    let mut f = opts.open(tmp)?;
    f.write_all(contents)?;
    if let Some(m) = mode {
        // Re-apply the mode in case a stale temp file pre-existed: create+truncate
        // reuses the existing inode and would NOT re-apply the open `mode`.
        f.set_permissions(std::fs::Permissions::from_mode(m))?;
    }
    f.sync_all()
}

#[cfg(not(unix))]
fn write_tmp(tmp: &Path, contents: &[u8], _mode: Option<u32>) -> io::Result<()> {
    std::fs::write(tmp, contents)
}

/// Best-effort: restrict a directory to owner-only (`0o700`) on Unix so a
/// plaintext credential store under it (e.g. the `~/.config` fallback) is not
/// readable by other local accounts. No-op (and never fails) off Unix.
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
