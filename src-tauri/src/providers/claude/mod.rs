//! Claude — **session-key only** (FEAT-2 / BUG-1). Claude Code's OAuth tokens
//! are no longer used: the app reads usage and writes the window anchor through
//! the claude.ai WEB API with a `sessionKey` cookie the user pastes via "Add
//! account". The OAuth / macOS-keychain auto-detect was removed because session
//! keys are not OAuth Bearer tokens (the old `/v1/messages` anchor 401'd), so
//! there is no `auto:claude` card — Claude is a stored-only provider.

use async_trait::async_trait;

use crate::model::{Provider, ServiceUsage};
use crate::providers::ProviderError;

pub(crate) mod web;

/// Normalize a pasted Claude session key / Cookie header to the bare session-key
/// value used for the read path (`sessionKey=<value>`). The write (anchor) path
/// uses `web::full_cookie_header`, which preserves Cloudflare cookies.
pub(crate) fn normalize_session_key(input: &str) -> String {
    web::session_key_cookie_value(input)
}

/// Build a human plan label like "Max 20x" from `rateLimitTier`
/// (e.g. "default_claude_max_20x" → "Max 20x"). Falls back to the subscription
/// type capitalized. Used by the web read path to label the card.
fn format_plan(tier: &Option<String>, sub: &Option<String>) -> Option<String> {
    use crate::util::capitalize as cap;
    if let Some(t) = tier {
        let lower = t.to_lowercase();
        let toks: Vec<&str> = lower.split('_').collect();
        let base = if toks.iter().any(|x| x.contains("max")) {
            "Max"
        } else if toks.iter().any(|x| *x == "pro" || x.contains("pro")) {
            "Pro"
        } else if toks.iter().any(|x| x.contains("team")) {
            "Team"
        } else if toks.iter().any(|x| x.contains("enterprise")) {
            "Enterprise"
        } else {
            return sub.as_deref().map(cap);
        };
        let mult = toks.iter().rev().find(|x| {
            x.ends_with('x') && x[..x.len() - 1].chars().all(|c| c.is_ascii_digit()) && x.len() > 1
        });
        return Some(match mult {
            Some(m) => format!("{base} {m}"),
            None => base.to_string(),
        });
    }
    sub.as_deref().map(cap)
}

pub struct ClaudeProvider {
    #[allow(dead_code)]
    http: reqwest::Client,
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeProvider {
    pub fn new() -> Self {
        Self {
            http: crate::http::shared(),
        }
    }
}

#[async_trait]
impl crate::providers::ProviderApi for ClaudeProvider {
    fn key(&self) -> Provider {
        Provider::Claude
    }

    /// Claude is session-key only: there is no OAuth/keychain auto-detect, so the
    /// auto-detect path produces no `auto:claude` card. Returning `NotLoggedIn`
    /// makes `fetch_all` downgrade it to a disconnected auto card, which the
    /// dedupe drops as soon as a stored session-key account connects. Manual
    /// Claude accounts flow through `fetch_stored` instead.
    async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
        Err(ProviderError::NotLoggedIn(
            "Claude is session-key only — add an account with a claude.ai session key".into(),
        ))
    }
}

/// Refresh a stored Claude credential. Session keys do not rotate / have no
/// refresh token, so this is always a no-op (`None`) — kept so the
/// `providers::refresh_stored` dispatch still compiles for every provider.
pub(crate) async fn refresh_stored(
    _http: &reqwest::Client,
    _cred: &crate::store::StoredCredential,
) -> Option<crate::store::StoredCredential> {
    None
}

/// Fetch usage for a stored Claude account (uniform stored-fetch adapter).
/// Manual Claude accounts are session-key based, so this uses the claude.ai
/// web API path.
pub(crate) async fn fetch_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Result<ServiceUsage, ProviderError> {
    web::fetch_with_session_key(http, &cred.access_token).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_plan_from_tier() {
        assert_eq!(
            format_plan(&Some("default_claude_max_20x".into()), &Some("max".into())).as_deref(),
            Some("Max 20x")
        );
        assert_eq!(
            format_plan(&Some("default_claude_max_5x".into()), &Some("max".into())).as_deref(),
            Some("Max 5x")
        );
        assert_eq!(
            format_plan(&Some("default_claude_pro".into()), &Some("pro".into())).as_deref(),
            Some("Pro")
        );
        assert_eq!(
            format_plan(&None, &Some("max".into())).as_deref(),
            Some("Max")
        );
        assert_eq!(format_plan(&None, &None), None);
    }

    #[test]
    fn normalize_session_key_extracts_bare_value() {
        assert_eq!(normalize_session_key("sk-ant-sid01-raw"), "sk-ant-sid01-raw");
        assert_eq!(
            normalize_session_key("Cookie: other=1; sessionKey=sk-ant-sid01-c; x=2"),
            "sk-ant-sid01-c"
        );
    }

    /// Claude's auto-detect path yields no card: session-key only, so `fetch`
    /// returns `NotLoggedIn` (no OAuth/keychain read). The dedupe in the refresh
    /// pipeline drops this disconnected auto card once a stored account connects.
    #[tokio::test]
    async fn claude_auto_fetch_is_not_logged_in() {
        use crate::providers::ProviderApi;
        let p = ClaudeProvider::new();
        let err = p
            .fetch()
            .await
            .expect_err("Claude auto path must not produce a card");
        assert!(matches!(err, ProviderError::NotLoggedIn(_)), "got: {err:?}");
    }

    #[tokio::test]
    async fn refresh_stored_is_always_none_for_session_keys() {
        let cred = crate::store::StoredCredential {
            id: "id".into(),
            provider: Provider::Claude,
            label: "x".into(),
            access_token: "sk-ant-sid01-x".into(),
            refresh_token: None,
            expires_at: 0,
            id_token: None,
            account_id: None,
        };
        let http = crate::http::build_client();
        assert!(refresh_stored(&http, &cred).await.is_none());
    }
}
