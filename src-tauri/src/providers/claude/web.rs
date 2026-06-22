//! Claude.ai web API client for manually-added session-key accounts (no OAuth /
//! CLI involved). Split out of the OAuth/CLI path in `super`.

use serde::Deserialize;

use super::format_plan;
use crate::model::{auto_service_id, LimitWindow, Provider, ServiceSource, ServiceUsage};
use crate::providers::ProviderError;

/// Shape verified against the live `/api/organizations` response: each org
/// carries `rate_limit_tier` directly and `memberships[].account.email_address`
/// inline, so no separate `/account` round-trip is needed (the shape that call
/// used to assume was wrong and always degraded to no email / no tier).
#[derive(Deserialize)]
struct WebOrg {
    uuid: String, // the identifier the usage endpoint expects
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    rate_limit_tier: Option<String>,
    #[serde(default)]
    memberships: Vec<WebMembership>,
}
#[derive(Deserialize)]
struct WebMembership {
    #[serde(default)]
    account: Option<WebMemberAccount>,
}
#[derive(Deserialize)]
struct WebMemberAccount {
    #[serde(default, rename = "email_address")]
    email_address: Option<String>,
}
#[derive(Deserialize)]
struct WebWindow {
    #[serde(default)]
    utilization: Option<f64>, // claude.ai returns int percent 0-100
    #[serde(default)]
    resets_at: Option<chrono::DateTime<chrono::Utc>>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "snake_case")]
struct WebUsage {
    #[serde(default)]
    five_hour: Option<WebWindow>,
    #[serde(default)]
    seven_day: Option<WebWindow>,
    #[serde(default)]
    seven_day_sonnet: Option<WebWindow>,
    #[serde(default)]
    seven_day_opus: Option<WebWindow>,
}

/// Fetch Claude usage via the claude.ai web API using a `sessionKey` cookie
/// (manually-added session-key account). No OAuth / CLI involved.
pub(crate) async fn fetch_with_session_key(
    http: &reqwest::Client,
    session_key: &str,
) -> Result<ServiceUsage, ProviderError> {
    let cookie = format!("sessionKey={}", session_key_cookie_value(session_key));

    let resp = http
        .get("https://claude.ai/api/organizations")
        .header("Cookie", &cookie)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let v: serde_json::Value = crate::http::send_for_json(resp, "claude.ai/organizations").await?;
    let orgs: Vec<WebOrg> =
        serde_json::from_value(v).map_err(|e| ProviderError::Parse(format!("orgs: {e}")))?;
    let org = orgs
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::Parse("no claude organization".into()))?;

    let resp = http
        .get(format!(
            "https://claude.ai/api/organizations/{}/usage",
            org.uuid
        ))
        .header("Cookie", &cookie)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    let v: serde_json::Value = crate::http::send_for_json(resp, "claude.ai/usage").await?;
    let raw_json = serde_json::to_string_pretty(&v).ok();
    let u: WebUsage =
        serde_json::from_value(v).map_err(|e| ProviderError::Parse(format!("usage: {e}")))?;

    // Email + plan tier come straight off the org object we already fetched.
    let email = org
        .memberships
        .iter()
        .find_map(|m| m.account.as_ref())
        .and_then(|a| a.email_address.clone());

    let win = |label: &str, w: &Option<WebWindow>| -> Option<LimitWindow> {
        w.as_ref().map(|w| LimitWindow {
            label: label.into(),
            used_percent: w.utilization.map(|v| v as f32),
            resets_at: w.resets_at.map(|d| d.timestamp()),
            used: None,
            limit: None,
        })
    };
    let mut windows = vec![];
    let mut detail = vec![];
    if let Some(x) = win("5-hour", &u.five_hour) {
        windows.push(x);
    }
    if let Some(x) = win("7-day", &u.seven_day) {
        windows.push(x);
    }
    if let Some(x) = win("7-day (Sonnet)", &u.seven_day_sonnet) {
        detail.push(x);
    }
    if let Some(x) = win("7-day (Opus)", &u.seven_day_opus) {
        detail.push(x);
    }
    Ok(ServiceUsage {
        id: auto_service_id(Provider::Claude),
        source: ServiceSource::Auto,
        provider: Provider::Claude,
        connected: true,
        plan: format_plan(&org.rate_limit_tier, &None).or(org.name),
        account: email,
        error: None,
        windows,
        detail_windows: detail,
        raw_response: raw_json,
    })
}

pub(super) fn session_key_cookie_value(input: &str) -> String {
    let trimmed = input.trim();
    let payload = trimmed
        .strip_prefix("Cookie:")
        .or_else(|| trimmed.strip_prefix("cookie:"))
        .unwrap_or(trimmed)
        .trim();
    for part in payload.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("sessionKey=") {
            return value.trim().to_string();
        }
    }
    payload.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_pasted_session_key_cookie_values() {
        assert_eq!(
            session_key_cookie_value("sk-ant-sid01-raw"),
            "sk-ant-sid01-raw"
        );
        assert_eq!(
            session_key_cookie_value("sessionKey=sk-ant-sid01-direct"),
            "sk-ant-sid01-direct"
        );
        assert_eq!(
            session_key_cookie_value("Cookie: other=1; sessionKey=sk-ant-sid01-cookie; x=2"),
            "sk-ant-sid01-cookie"
        );
    }

    #[test]
    fn web_orgs_parse_tier_and_email() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/claude_web_orgs.json")).unwrap();
        let orgs: Vec<WebOrg> = serde_json::from_value(v).unwrap();
        let org = orgs.into_iter().next().unwrap();
        assert_eq!(org.uuid, "11111111-2222-3333-4444-555555555555");
        assert_eq!(
            org.rate_limit_tier.as_deref(),
            Some("default_claude_max_20x")
        );
        let email = org
            .memberships
            .iter()
            .find_map(|m| m.account.as_ref())
            .and_then(|a| a.email_address.clone());
        assert_eq!(email.as_deref(), Some("claude-user@example.invalid"));
    }
}
