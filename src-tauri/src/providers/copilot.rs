//! GitHub Copilot — reads the Copilot CLI's stored token (macOS Keychain
//! `copilot-cli`, else `~/.copilot/config.json`) and calls the internal
//! `GET /copilot_internal/user` endpoint, which returns the `quota_snapshots`
//! gauge (chat / premium_interactions / completions) plus `quota_reset_date`.
//!
//! Verified against `vbgate/opencode-mystatus` (MIT, the reference cited in the
//! README): `/copilot_internal/v2/token` is a *token-exchange* endpoint that
//! returns `{token, expires_at, refresh_in, endpoints}` — NOT the quota source.
//! The quota lives at `/copilot_internal/user`. (The previous code hit the
//! token endpoint and would never have parsed `quota_snapshots` from a live
//! response.) For manual accounts use a fine-grained PAT with "Copilot
//! Requests: Read" permission (paste via Add account); `Authorization: token
//! <key>` works for both Copilot CLI OAuth tokens and PATs on this endpoint.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::http;
use crate::model::{auto_service_id, LimitWindow, Provider, ServiceSource, ServiceUsage};
use crate::providers::ProviderError;
use crate::secrets;

/// Quota source per `vbgate/opencode-mystatus` `plugin/lib/copilot.ts`
/// (Strategy 2 — direct call with the OAuth/PAT token using the legacy
/// `Authorization: token <t>` scheme).
const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";

/// Header values mirror the current VS Code Copilot extension (Jan 2026),
/// per `vbgate/opencode-mystatus` `plugin/lib/copilot.ts`. These identify the
/// client as VS Code Copilot Chat, which `/copilot_internal/user` expects.
const EDITOR_VERSION: &str = "vscode/1.107.0";
const EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.35.0";
const USER_AGENT: &str = "GitHubCopilotChat/0.35.0";

#[derive(Deserialize, Default)]
struct CopilotUsageResp {
    #[serde(default)]
    login: Option<String>,
    #[serde(default)]
    copilot_plan: Option<String>,
    #[serde(default)]
    quota_reset_date: Option<String>,
    #[serde(default)]
    quota_snapshots: QuotaSnapshots,
}

#[derive(Deserialize, Default)]
struct QuotaSnapshots {
    #[serde(default)]
    chat: Option<QuotaSnapshot>,
    #[serde(default)]
    premium_interactions: Option<QuotaSnapshot>,
    #[serde(default)]
    completions: Option<QuotaSnapshot>,
}

#[derive(Deserialize, Default)]
struct QuotaSnapshot {
    #[serde(default)]
    entitlement: Option<f64>,
    /// Canonical field per `opencode-mystatus` (`used = entitlement - remaining`).
    #[serde(default)]
    remaining: Option<f64>,
    /// Older payloads carry this alias; we accept either.
    #[serde(default)]
    quota_remaining: Option<f64>,
    #[serde(default)]
    percent_remaining: Option<f64>,
    #[serde(default)]
    unlimited: Option<bool>,
}

pub struct CopilotProvider {
    http: reqwest::Client,
}

impl Default for CopilotProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CopilotProvider {
    pub fn new() -> Self {
        Self {
            http: http::build_client(),
        }
    }
}

/// Reuse the Copilot CLI's stored OAuth token. The Copilot CLI keeps the token
/// in the OS-native secret store on every platform (macOS Keychain, Windows
/// Credential Manager, Linux Secret Service) under service `copilot-cli`, and
/// only writes the plaintext `<COPILOT_HOME or ~/.copilot>/config.json` as a
/// fallback when no keystore is available. We therefore read the keystore first,
/// then fall back to the file.
fn read_copilot_token() -> Result<String, ProviderError> {
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
        // Windows Credential Manager / Linux Secret Service via the keyring crate.
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

/// Parse the `quota_reset_date` field. The real API returns a plain `YYYY-MM-DD`
/// date (e.g. `"2026-07-01"`); tolerate full RFC-3339 too.
fn parse_reset_date(s: &str) -> Option<i64> {
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(d.timestamp());
    }
    chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0).map(|ndt| ndt.and_utc().timestamp()))
}

/// Pure: quota_snapshots → (headline windows, detail notes).
///
/// Metered categories (unlimited=false) become real usage windows sorted by
/// highest burn; the first one is the card headline. Unlimited categories
/// (unlimited=true — typically `chat` and `completions` on individual plans)
/// become labeled notes in `detail_windows` so the user can see what their
/// plan includes without polluting the usage math.
fn normalize(resp: &CopilotUsageResp) -> (Vec<LimitWindow>, Vec<LimitWindow>) {
    let reset = resp.quota_reset_date.as_deref().and_then(parse_reset_date);
    let categories: [(&str, &Option<QuotaSnapshot>); 3] = [
        ("Chat", &resp.quota_snapshots.chat),
        (
            "Premium requests",
            &resp.quota_snapshots.premium_interactions,
        ),
        ("Completions", &resp.quota_snapshots.completions),
    ];
    let mut windows: Vec<LimitWindow> = Vec::new();
    let mut detail: Vec<LimitWindow> = Vec::new();
    for (label, q) in categories {
        let Some(q) = q.as_ref() else { continue };
        if q.unlimited.unwrap_or(false) {
            // Unlimited category — surface as a labeled note in the modal.
            detail.push(LimitWindow {
                label: format!("{label} (unlimited)"),
                used_percent: None,
                resets_at: reset,
                used: None,
                limit: None,
            });
            continue;
        }
        let pct = q.percent_remaining.map(|r| (100.0 - r) as f32);
        // Reference uses `remaining`; older payloads carry `quota_remaining`.
        let remaining = q.remaining.or(q.quota_remaining);
        let used = match (q.entitlement, remaining) {
            (Some(e), Some(r)) => Some(e - r),
            _ => None,
        };
        windows.push(LimitWindow {
            label: (*label).into(),
            used_percent: pct,
            resets_at: reset,
            used,
            limit: q.entitlement,
        });
    }
    windows.sort_by(|a, b| {
        b.used_percent
            .unwrap_or(0.0)
            .partial_cmp(&a.used_percent.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    (windows, detail)
}

#[async_trait]
impl crate::providers::ProviderApi for CopilotProvider {
    fn key(&self) -> Provider {
        Provider::Copilot
    }

    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        let token = read_copilot_token()?;
        fetch_with(&self.http, &token).await
    }
}

/// Fetch Copilot usage given an explicit token (auto-detected Copilot CLI token
/// or a manually-pasted PAT). Calls `GET /copilot_internal/user`.
pub(crate) async fn fetch_with(
    http: &reqwest::Client,
    token: &str,
) -> Result<ServiceUsage, ProviderError> {
    let resp = http
        .get(USAGE_URL)
        .header("Authorization", format!("token {token}"))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT)
        .header("Editor-Version", EDITOR_VERSION)
        .header("Editor-Plugin-Version", EDITOR_PLUGIN_VERSION)
        .header("Copilot-Integration-Id", "vscode-chat")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let val: Value = http::send_for_json(resp, USAGE_URL).await?;
    let raw_json = serde_json::to_string_pretty(&val).ok();
    let u: CopilotUsageResp =
        serde_json::from_value(val).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let (windows, detail_windows) = normalize(&u);
    Ok(ServiceUsage {
        id: auto_service_id(Provider::Copilot),
        source: ServiceSource::Auto,
        provider: Provider::Copilot,
        connected: true,
        plan: u.copilot_plan,
        account: u.login,
        error: None,
        windows,
        detail_windows,
        raw_response: raw_json,
    })
}

/// Refresh a stored credential's access_token using its refresh_token.
///
/// Returns `None` (no-op) for Copilot: the `gho_…` user OAuth tokens the
/// Copilot CLI stores in `copilot-cli` / `~/.copilot/config.json` **do not
/// expire and have no refresh grant** — they persist until revoked, and the
/// device-code flow the CLI uses (`cli/cli` `internal/authflow/flow.go`,
/// client_id `178c6fc778ccc68e1d6a`) does not return a `refresh_token`. Only
/// GitHub-App-installed tokens can expire/refresh, and the Copilot CLI does not
/// issue those. PATs are similarly non-expiring. The caller therefore falls
/// back to the existing token. See `docs/oauth-credential-research.md` §4.B.
pub(crate) async fn refresh_stored(
    _http: &reqwest::Client,
    _cred: &crate::store::StoredCredential,
) -> Option<crate::store::StoredCredential> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_live_fixture_shows_metered_and_unlimited_notes() {
        // Real response captured 2026-06-19 from an individual-plan account:
        // chat + completions are unlimited; premium_interactions is metered
        // (200 of 200 remaining → 0% used).
        let raw = include_str!("../../tests/copilot_internal_fixture.json");
        let v: Value = serde_json::from_str(raw).unwrap();
        let u: CopilotUsageResp = serde_json::from_value(v).unwrap();
        assert_eq!(u.login.as_deref(), Some("yakisoba0728"));
        assert_eq!(u.copilot_plan.as_deref(), Some("individual"));
        assert_eq!(u.quota_reset_date.as_deref(), Some("2026-07-01"));

        let (ws, detail) = normalize(&u);
        // Only the metered category surfaces as a usage window.
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].label, "Premium requests");
        assert_eq!(ws[0].used_percent, Some(0.0)); // percent_remaining=100 → 0% used
        assert_eq!(ws[0].used, Some(0.0)); // entitlement(200) - remaining(200)
        assert_eq!(ws[0].limit, Some(200.0));
        assert!(ws[0].resets_at.is_some()); // 2026-07-01 parsed

        // Unlimited categories appear as labeled notes in detail (no percent).
        assert_eq!(detail.len(), 2);
        assert!(detail.iter().any(|w| w.label == "Chat (unlimited)"));
        assert!(detail.iter().any(|w| w.label == "Completions (unlimited)"));
        assert!(detail.iter().all(|w| w.used_percent.is_none()));
    }

    #[test]
    fn normalize_sorts_metered_categories_by_usage_desc() {
        // Synthetic: two metered categories, one more consumed than the other.
        let resp = CopilotUsageResp {
            login: None,
            copilot_plan: Some("pro".into()),
            quota_reset_date: Some("2026-07-01T00:00:00Z".into()),
            quota_snapshots: QuotaSnapshots {
                chat: Some(QuotaSnapshot {
                    entitlement: Some(300.0),
                    remaining: Some(250.0),
                    quota_remaining: Some(250.0),
                    percent_remaining: Some(83.33),
                    unlimited: Some(false),
                }),
                premium_interactions: Some(QuotaSnapshot {
                    entitlement: Some(300.0),
                    remaining: Some(50.0),
                    quota_remaining: Some(50.0),
                    percent_remaining: Some(16.67),
                    unlimited: Some(false),
                }),
                completions: None,
            },
        };
        let (ws, detail) = normalize(&resp);
        // Both metered categories in windows; none unlimited → detail empty.
        assert_eq!(ws.len(), 2);
        assert!(detail.is_empty());
        // Higher usage (lower percent_remaining) first.
        assert_eq!(ws[0].label, "Premium requests");
        assert_eq!(ws[0].used_percent, Some(83.33));
        assert_eq!(ws[0].used, Some(250.0));
        assert_eq!(ws[0].limit, Some(300.0));
        // Both windows share the parsed reset date.
        assert!(ws[0].resets_at.is_some());
        assert!(ws[1].resets_at.is_some());
    }

    #[test]
    fn parse_reset_date_accepts_plain_and_rfc3339() {
        // Real API format: plain YYYY-MM-DD.
        assert!(parse_reset_date("2026-07-01").is_some());
        // Tolerate full RFC-3339 too.
        assert!(parse_reset_date("2026-07-01T00:00:00Z").is_some());
        assert!(parse_reset_date("nonsense").is_none());
    }

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

    #[tokio::test]
    async fn refresh_stored_is_noop() {
        // Copilot tokens don't expire refreshably; refresh is a documented no-op.
        let http = crate::http::build_client();
        let cred = crate::store::StoredCredential {
            id: "x".into(),
            provider: Provider::Copilot,
            label: "test".into(),
            access_token: "gho_x".into(),
            refresh_token: None,
            expires_at: 0,
            id_token: None,
            account_id: None,
        };
        assert!(refresh_stored(&http, &cred).await.is_none());
    }
}
