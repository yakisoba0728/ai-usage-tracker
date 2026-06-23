//! Window anchoring: send a minimal throwaway message so a provider's rolling
//! usage window starts at a predictable time. The app's only write path — kept
//! entirely in Rust (tokens never cross IPC). Supported: Claude, Codex, z.ai.
//! z.ai sends a 1-token message; Codex sends a minimal turn via the Responses
//! API (no 1-token cap — reasoning models reject it); Claude (session-key only,
//! FEAT-2/BUG-1) sends via the claude.ai WEB chat completion endpoint — see
//! `providers::claude::web::send_claude_web`.
//!
//! Structure (spec §7): per-provider send bodies live in `send/{zai,codex}`
//! (Claude's arm is a one-call route kept inline in `send` below); credential
//! resolution in `resolve`; the cooldown guard in `cooldown`. This module is the
//! dispatcher + the bits shared across arms (`supported`, `anchor_body`, the
//! endpoint URLs) and re-exports the public surface so callers keep their paths.

mod cooldown;
mod resolve;
mod send;

use crate::http;
use crate::model::Provider;
use crate::providers::ProviderError;

// Public surface preserved verbatim: callers (`commands/`) and the in-module
// tests reach these at `anchor::<name>` exactly as before the split.
pub use cooldown::{clear, failure_is_transient, try_begin};
pub use send::{send_codex, send_zai};

// Coding-plan endpoint (not the general /api/paas/v4) so the message draws on the
// GLM Coding Plan window the tracker shows. `glm-4-flash` is rejected (code 1211);
// use a current coding model.
const ZAI_CHAT_URL: &str = "https://api.z.ai/api/coding/paas/v4/chat/completions";

const CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";

/// Providers where anchoring is meaningful AND a send path exists.
pub fn supported(provider: Provider) -> bool {
    matches!(provider, Provider::Claude | Provider::Codex | Provider::Zai)
}

/// A minimal 1-token user message body (shared by the chat-completions shapes).
pub fn anchor_body(model: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "max_tokens": 1,
        "messages": [{ "role": "user", "content": "." }]
    })
}

/// Send an anchor message for the given UI `service_id`.
///
/// `device_id` is the stable per-install `anthropic-device-id` (from
/// `AppConfig`, ensured at startup) — required for the Claude web-chat anchor;
/// the other providers ignore it. Both call sites supply it (the auto path holds
/// the `ConfigStore`; `send_anchor_now` reads it from the managed config).
pub async fn send(service_id: &str, device_id: &str) -> Result<(), ProviderError> {
    let http = http::shared();
    if service_id.starts_with("stored:") {
        let cred = resolve::resolve_stored(service_id)
            .ok_or_else(|| ProviderError::NotLoggedIn(format!("no stored account {service_id}")))?;
        if !supported(cred.provider) {
            return Err(ProviderError::NotLoggedIn(format!(
                "{:?} does not support anchoring",
                cred.provider
            )));
        }
        // Refresh an expired token before sending, so a manual "Anchor now" is
        // never weaker than the poll path (which refreshes via fetch_credential)
        // and doesn't 401 on a stale bearer (B-11). Claude session keys have no
        // refresh path, so this is a no-op for them.
        let cred = crate::providers::refresh_if_expired(&http, &cred).await?;
        return match cred.provider {
            Provider::Zai => send_zai(&http, &cred.access_token, ZAI_CHAT_URL).await,
            // Claude is session-key only: anchor via the claude.ai WEB chat
            // completion endpoint (FEAT-2/BUG-1). The stored access_token holds
            // the pasted session key / full Cookie header.
            Provider::Claude => {
                crate::providers::claude::web::send_claude_web(
                    &http,
                    &cred.access_token,
                    device_id,
                    crate::providers::claude::web::CLAUDE_WEB_API_BASE,
                )
                .await
            }
            Provider::Codex => {
                send_codex(
                    &http,
                    &cred.access_token,
                    cred.account_id.as_deref(),
                    CODEX_RESPONSES_URL,
                )
                .await
            }
            other => Err(ProviderError::NotLoggedIn(format!(
                "anchoring for stored {other:?} not implemented yet"
            ))),
        };
    }
    // Auto (CLI/env-detected) anchors. Claude has NO auto path anymore
    // (session-key only) — there is no `auto:claude` arm.
    match service_id {
        "auto:zai" => send_zai(&http, &resolve::resolve_zai_auto()?, ZAI_CHAT_URL).await,
        "auto:codex" => {
            let (access, account_id) = crate::providers::codex::prepare_auto_auth(&http).await?;
            send_codex(&http, &access, account_id.as_deref(), CODEX_RESPONSES_URL).await
        }
        other => Err(ProviderError::NotLoggedIn(format!(
            "anchoring not implemented for {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;

    #[test]
    fn supported_is_claude_codex_zai() {
        assert!(supported(Provider::Claude));
        assert!(supported(Provider::Codex));
        assert!(supported(Provider::Zai));
        assert!(!supported(Provider::Copilot));
        assert!(!supported(Provider::Gemini));
        assert!(!supported(Provider::Cursor));
    }

    #[test]
    fn anchor_body_is_one_token_user_message() {
        let b = anchor_body("glm-4-flash");
        assert_eq!(b["model"], "glm-4-flash");
        assert_eq!(b["max_tokens"], 1);
        assert_eq!(b["messages"][0]["role"], "user");
        assert!(b["messages"][0]["content"].is_string());
    }

    // NOTE: the Claude anchor moved to the claude.ai WEB chat completion endpoint
    // (session-key only, FEAT-2/BUG-1). The old `/v1/messages` OAuth wiremock
    // test was deleted; the new request-SHAPE test lives next to the
    // implementation — `providers::claude::web::tests::send_claude_web_*`.
}
