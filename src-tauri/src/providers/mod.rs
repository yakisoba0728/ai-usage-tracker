//! Provider trait + shared error type + parallel `fetch_all`.

pub mod claude;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod gemini;
pub mod zai;

use async_trait::async_trait;
use futures::future::join_all;

use crate::model::{auto_service_id, stored_service_id, Provider, ServiceSource, ServiceUsage};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("credentials not found: {0}")]
    NotLoggedIn(String),
    #[error("token expired: {0}")]
    Expired(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected response ({status}): {body}")]
    Status { status: u16, body: String },
    #[error("parse error: {0}")]
    Parse(String),
}

impl From<crate::secrets::SecretsError> for ProviderError {
    fn from(e: crate::secrets::SecretsError) -> Self {
        ProviderError::NotLoggedIn(e.to_string())
    }
}

#[async_trait]
pub trait ProviderApi: Send + Sync {
    fn key(&self) -> Provider;
    async fn fetch(&self) -> Result<ServiceUsage, ProviderError>;
}

/// Run every provider concurrently. A failing provider is downgraded to a
/// disconnected `ServiceUsage` and never aborts the batch (isolation invariant).
pub async fn fetch_all(providers: Vec<Box<dyn ProviderApi>>) -> Vec<ServiceUsage> {
    let futs = providers.into_iter().map(|p| async move {
        let key = p.key();
        match p.fetch().await {
            Ok(u) => u,
            Err(e) => ServiceUsage {
                id: auto_service_id(key),
                source: ServiceSource::Auto,
                provider: key,
                connected: false,
                plan: None,
                account: None,
                error: Some(e.to_string()),
                windows: vec![],
                detail_windows: vec![],
                raw_response: None,
            },
        }
    });
    join_all(futs).await
}

/// Fetch usage for a manually-added (OAuth/API-key) account whose token lives
/// in the store. Refreshes the access token first if it's near expiry (P0 #3).
pub async fn fetch_credential(cred: &crate::store::StoredCredential) -> ServiceUsage {
    let http = crate::http::build_client();
    let now_ms = chrono::Utc::now().timestamp_millis();

    // P0 #3: refresh expired stored tokens before fetching. Cursor/CoPilot/z.ai
    // return None (no refresh path); Claude session-key accounts return None
    // (no refresh_token). Codex/Gemini OAuth accounts rotate and persist.
    let active = if cred.expires_at > 0 && cred.expires_at < now_ms {
        match refresh_stored(&http, cred).await {
            Some(updated) => {
                if !crate::store::update(&updated) {
                    eprintln!(
                        "stored {:?} account {}: refreshed token not persisted",
                        updated.provider, updated.id
                    );
                }
                updated
            }
            None => cred.clone(),
        }
    } else {
        cred.clone()
    };

    let label = active.label.as_str();
    let res = match active.provider {
        Provider::Codex => {
            crate::providers::codex::fetch_with(
                &http,
                &active.access_token,
                active.account_id.as_deref(),
                &active.id_token,
                Some(label),
            )
            .await
        }
        Provider::Gemini => {
            crate::providers::gemini::fetch_with(&http, &active.access_token, Some(label)).await
        }
        Provider::Claude => {
            crate::providers::claude::fetch_with_session_key(&http, &active.access_token).await
        }
        Provider::Copilot => {
            crate::providers::copilot::fetch_with(&http, &active.access_token).await
        }
        Provider::Zai => {
            crate::providers::zai::fetch_with(&http, &active.access_token, Some(label)).await
        }
        Provider::Cursor => Err(ProviderError::NotLoggedIn(
            "manual accounts not supported for Cursor (CLI-detected only)".into(),
        )),
    };
    match res {
        Ok(mut u) => {
            u.id = stored_service_id(&active.id);
            u.source = ServiceSource::Stored;
            if u.account.is_none() {
                u.account = Some(active.label.clone());
            }
            u
        }
        Err(e) => ServiceUsage {
            id: stored_service_id(&active.id),
            source: ServiceSource::Stored,
            provider: active.provider,
            connected: false,
            plan: None,
            account: Some(active.label.clone()),
            error: Some(e.to_string()),
            windows: vec![],
            detail_windows: vec![],
            raw_response: None,
        },
    }
}

/// Dispatch to the provider's `refresh_stored` helper. Returns Some(updated) if
/// a refresh happened (caller persists), None if not applicable / failed.
async fn refresh_stored(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Option<crate::store::StoredCredential> {
    match cred.provider {
        Provider::Claude => crate::providers::claude::refresh_stored(http, cred).await,
        Provider::Codex => crate::providers::codex::refresh_stored(http, cred).await,
        Provider::Gemini => crate::providers::gemini::refresh_stored(http, cred).await,
        Provider::Copilot => crate::providers::copilot::refresh_stored(http, cred).await,
        Provider::Cursor => crate::providers::cursor::refresh_stored(http, cred).await,
        Provider::Zai => crate::providers::zai::refresh_stored(http, cred).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OkStub(Provider);
    #[async_trait]
    impl ProviderApi for OkStub {
        fn key(&self) -> Provider {
            self.0
        }
        async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
            Ok(ServiceUsage {
                id: auto_service_id(self.0),
                source: ServiceSource::Auto,
                provider: self.0,
                connected: true,
                plan: Some("pro".into()),
                account: None,
                error: None,
                windows: vec![],
                detail_windows: vec![],
                raw_response: None,
            })
        }
    }

    struct ErrStub(Provider);
    #[async_trait]
    impl ProviderApi for ErrStub {
        fn key(&self) -> Provider {
            self.0
        }
        async fn fetch(&self) -> Result<ServiceUsage, ProviderError> {
            Err(ProviderError::NotLoggedIn("nope".into()))
        }
    }

    #[tokio::test]
    async fn fetch_all_isolates_failures() {
        let providers: Vec<Box<dyn ProviderApi>> = vec![
            Box::new(OkStub(Provider::Claude)),
            Box::new(ErrStub(Provider::Codex)),
            Box::new(OkStub(Provider::Gemini)),
        ];
        let out = fetch_all(providers).await;
        assert_eq!(out.len(), 3);
        // ordering preserved
        assert_eq!(out[0].provider, Provider::Claude);
        assert!(out[0].connected);
        assert_eq!(out[1].provider, Provider::Codex);
        assert!(!out[1].connected);
        assert!(out[1].error.is_some());
        assert_eq!(out[2].provider, Provider::Gemini);
        assert!(out[2].connected);
    }
}
