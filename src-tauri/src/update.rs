//! GitHub-release update notifier (FEAT-5, spec §6.6).
//!
//! No new plugin: reuses the shared `reqwest` client (`http::shared()`),
//! `tauri-plugin-opener` (to open the release page), and the FEAT-3
//! `tauri-plugin-notification` path. A background task checks the GitHub
//! "latest release" endpoint at startup and on a long (24h) interval; when the
//! published `tag_name` is newer than this build's `CARGO_PKG_VERSION` it fires
//! one OS notification and remembers the version so the same release never
//! re-notifies every interval.
//!
//! The version comparison (`version_is_newer`) is a PURE function with no Tauri
//! deps so it is unit-tested in isolation (D2 — test the logic, keep the I/O
//! shell thin).

use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::commands::ConfigStore;

/// The GitHub "latest release" endpoint for this repo (spec §6.6, owner
/// confirmed `yakisoba0728/ai-usage-tracker`).
const RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/yakisoba0728/ai-usage-tracker/releases/latest";

/// How often the background task re-checks for a new release. A release cadence
/// is days, not minutes — a daily check keeps the API load trivial (well under
/// GitHub's unauthenticated 60 req/h limit) while still surfacing an update
/// within a day.
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// The relevant fields of a GitHub release object.
#[derive(Debug, Clone, serde::Deserialize)]
struct GithubRelease {
    /// e.g. "v0.2.0" or "0.2.0".
    tag_name: String,
    /// The human release page to open in the browser when the user wants it.
    html_url: String,
}

/// A newer release than the running build, already parsed + version-stripped.
/// Serialized to the frontend by `check_update_now` so the "Check for updates"
/// button can show the result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AvailableUpdate {
    /// The published version with any leading `v` stripped (e.g. "0.2.0").
    pub version: String,
    /// The release page URL.
    pub html_url: String,
}

/// Strip a single leading `v`/`V` and surrounding whitespace from a git tag so
/// "v0.2.0" and "0.2.0" compare identically.
fn normalize_version(tag: &str) -> &str {
    tag.trim().trim_start_matches(['v', 'V'])
}

/// Compare two dotted numeric versions. Returns true iff `candidate` is strictly
/// newer than `current`. Pure, no `semver` crate (spec §6.6 / D2):
/// - split each on `.`,
/// - parse each segment as a number (a non-numeric or missing segment counts as
///   0, so "1.2" and "1.2.0" are equal and a malformed suffix never makes a
///   version spuriously "newer"),
/// - compare segment-by-segment up to the longer length.
///
/// A leading `v` on either side is stripped first, so `version_is_newer("v1.2.0",
/// "1.1.9")` is true.
pub fn version_is_newer(candidate: &str, current: &str) -> bool {
    let cand = parse_segments(normalize_version(candidate));
    let cur = parse_segments(normalize_version(current));
    let len = cand.len().max(cur.len());
    for i in 0..len {
        // A missing trailing segment is treated as 0 ("1.2" == "1.2.0").
        let c = cand.get(i).copied().unwrap_or(0);
        let r = cur.get(i).copied().unwrap_or(0);
        if c != r {
            return c > r;
        }
    }
    false // all segments equal → not newer
}

/// Parse a dotted version into numeric segments. A non-numeric segment (e.g. a
/// "-rc1" suffix's leading number is taken; a wholly non-numeric chunk is 0) is
/// coerced to 0 so a weird tag never crashes and never reads as artificially
/// large. We take the leading run of digits of each segment so "2-rc1" → 2.
fn parse_segments(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|seg| {
            let digits: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

/// GET the latest release and, if it is newer than this build, return it. Errors
/// (offline, rate-limited, parse) are returned as a string and treated as
/// non-fatal by callers — a failed update check must never disrupt the app.
async fn fetch_available_update() -> Result<Option<AvailableUpdate>, String> {
    let client = crate::http::shared();
    let resp = client
        .get(RELEASES_LATEST_URL)
        // GitHub requires a User-Agent; Accept pins the stable API media type.
        .header("User-Agent", "ai-usage-tracker")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("update check request failed: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("update check returned HTTP {}", status.as_u16()));
    }
    let release: GithubRelease = resp
        .json()
        .await
        .map_err(|e| format!("update check parse failed: {e}"))?;

    let current = env!("CARGO_PKG_VERSION");
    if version_is_newer(&release.tag_name, current) {
        Ok(Some(AvailableUpdate {
            version: normalize_version(&release.tag_name).to_string(),
            html_url: release.html_url,
        }))
    } else {
        Ok(None)
    }
}

/// Fire the "update available" OS notification. Clicking it (or the in-app
/// follow-up) opens the release page via `tauri-plugin-opener`. The notification
/// text is intentionally simple English (it fires from Rust, possibly while the
/// webview is closed). Errors are swallowed — a failed toast must not abort the
/// check loop.
fn notify_update_available(app: &AppHandle, update: &AvailableUpdate) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app
        .notification()
        .builder()
        .title("Update available")
        .body(format!(
            "AI Usage Tracker {} is available. Click to open the release page.",
            update.version
        ))
        .show();
    // Best-effort: open the release page so the click has somewhere to land even
    // on platforms where notification-click routing isn't wired. Swallow errors.
    open_release_page(app, &update.html_url);
}

/// Open the release page in the default browser via the opener plugin.
fn open_release_page(app: &AppHandle, html_url: &str) {
    use tauri_plugin_opener::OpenerExt;
    let _ = app.opener().open_url(html_url, None::<&str>);
}

/// Run one update check, honoring config gating + last-notified suppression.
///
/// - When `force` is false (the scheduled path) the check is skipped entirely if
///   `auto_update_check` is off.
/// - A newer release only notifies (and opens the page) if it differs from
///   `last_notified_version`, which is then persisted so the SAME release never
///   re-notifies on the next interval.
/// - When `force` is true (the manual "Check for updates" button) the check runs
///   regardless of the toggle and always notifies for a newer release.
///
/// Returns the available update (if any) so the manual command can surface it;
/// errors are returned to the caller, never panicked.
pub async fn run_update_check(
    app: &AppHandle,
    cfg: &ConfigStore,
    force: bool,
) -> Result<Option<AvailableUpdate>, String> {
    if !force && !cfg.read().await.auto_update_check {
        return Ok(None);
    }

    let Some(update) = fetch_available_update().await? else {
        return Ok(None);
    };

    // Suppress a repeat notification for a release we've already flagged (only on
    // the automatic path; a manual check always re-notifies so the user gets
    // feedback from the button press).
    let already_notified =
        cfg.read().await.last_notified_version.as_deref() == Some(&update.version);
    if force || !already_notified {
        notify_update_available(app, &update);
    }

    // Remember this version so the scheduled loop doesn't re-fire for it. Persist
    // through the same path as the rest of the config (atomic, owner-only).
    if !already_notified {
        let mut guard = cfg.write().await;
        guard.last_notified_version = Some(update.version.clone());
        if let Err(e) = guard.save() {
            eprintln!("update: could not persist last_notified_version: {e}");
        }
    }

    Ok(Some(update))
}

/// Spawn the background update checker: one check shortly after startup, then
/// every `CHECK_INTERVAL`. Gating + suppression live in `run_update_check`, so
/// this is just the timer shell.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let cfg = app.state::<ConfigStore>().inner().clone();
        loop {
            if let Err(e) = run_update_check(&app, &cfg, false).await {
                // Non-fatal: offline / rate-limited / transient. Logged, then we
                // wait for the next interval.
                eprintln!("update: check failed: {e}");
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch_minor_and_major_are_detected() {
        assert!(version_is_newer("0.1.1", "0.1.0"), "newer patch");
        assert!(
            version_is_newer("0.2.0", "0.1.9"),
            "newer minor beats patch"
        );
        assert!(version_is_newer("1.0.0", "0.9.9"), "newer major beats all");
    }

    #[test]
    fn older_or_equal_is_not_newer() {
        assert!(!version_is_newer("0.1.0", "0.1.1"), "older patch");
        assert!(!version_is_newer("0.1.0", "0.2.0"), "older minor");
        assert!(!version_is_newer("0.9.9", "1.0.0"), "older major");
        assert!(!version_is_newer("0.1.0", "0.1.0"), "equal is not newer");
    }

    #[test]
    fn leading_v_prefix_is_ignored_on_either_side() {
        assert!(version_is_newer("v0.2.0", "0.1.0"), "v-prefixed candidate");
        assert!(version_is_newer("0.2.0", "v0.1.0"), "v-prefixed current");
        assert!(
            !version_is_newer("v0.1.0", "v0.1.0"),
            "both v-prefixed, equal"
        );
        assert!(version_is_newer("V1.0.0", "v0.9.0"), "uppercase V too");
    }

    #[test]
    fn uneven_segment_counts_treat_missing_as_zero() {
        // "1.2" == "1.2.0", neither is newer.
        assert!(!version_is_newer("1.2", "1.2.0"));
        assert!(!version_is_newer("1.2.0", "1.2"));
        // "1.2.1" > "1.2".
        assert!(version_is_newer("1.2.1", "1.2"));
        // "1.2" < "1.2.1".
        assert!(!version_is_newer("1.2", "1.2.1"));
        // A longer current with a trailing 0 is still equal.
        assert!(!version_is_newer("1", "1.0.0"));
    }

    #[test]
    fn double_digit_segments_compare_numerically_not_lexically() {
        // Lexically "9" > "10"; numerically 10 > 9. Must be numeric.
        assert!(version_is_newer("0.10.0", "0.9.0"));
        assert!(!version_is_newer("0.9.0", "0.10.0"));
        assert!(version_is_newer("0.0.11", "0.0.2"));
    }

    #[test]
    fn malformed_segments_coerce_to_zero_and_dont_read_as_newer() {
        // A non-numeric / pre-release-ish tag must not crash and must not be
        // treated as artificially large. "1.2.0-rc1" → 1.2.0 (leading digits).
        assert!(
            !version_is_newer("1.2.0-rc1", "1.2.0"),
            "rc suffix == base, not newer"
        );
        assert!(
            version_is_newer("1.3.0-rc1", "1.2.0"),
            "newer minor with rc suffix"
        );
        assert!(
            !version_is_newer("garbage", "0.1.0"),
            "wholly non-numeric → 0.0.0"
        );
    }

    #[test]
    fn normalize_version_strips_v_and_whitespace() {
        assert_eq!(normalize_version("  v1.2.3 "), "1.2.3");
        assert_eq!(normalize_version("V0.0.1"), "0.0.1");
        assert_eq!(normalize_version("2.0.0"), "2.0.0");
    }
}
