//! Provider trait + shared error type + parallel `fetch_all`.

pub mod claude;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod gemini;
pub mod zai;

use async_trait::async_trait;
use futures::future::join_all;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

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

impl ProviderError {
    /// Stable machine code for the frontend to localize (`error.<code>`).
    /// The variant's `Display` string travels alongside as the technical detail.
    pub fn code(&self) -> &'static str {
        match self {
            ProviderError::NotLoggedIn(_) => "not_logged_in",
            ProviderError::Expired(_) => "token_expired",
            ProviderError::Network(_) => "network",
            ProviderError::Status { .. } => "server_error",
            ProviderError::Parse(_) => "parse_error",
        }
    }
}

impl From<crate::secrets::SecretsError> for ProviderError {
    fn from(e: crate::secrets::SecretsError) -> Self {
        ProviderError::NotLoggedIn(e.to_string())
    }
}

impl From<ProviderError> for crate::model::ServiceError {
    fn from(e: ProviderError) -> Self {
        crate::model::ServiceError {
            code: e.code().to_string(),
            detail: Some(e.to_string()),
        }
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
                error: Some(e.into()),
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
/// Serializes refresh+persist per stored credential, so two refreshes of the
/// same rotating token cannot replay the old refresh_token while different
/// stored accounts can still fetch/refresh independently.
static STORED_REFRESH_LOCKS: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn stored_refresh_lock_for(id: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = STORED_REFRESH_LOCKS
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    locks
        .entry(id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

fn is_expired(cred: &crate::store::StoredCredential, now_ms: i64) -> bool {
    cred.expires_at > 0 && cred.expires_at < now_ms
}

/// Refresh a stored credential if its access token is past expiry, persisting the
/// rotated token, and return the credential to actually use. Serialized via
/// `REFRESH_LOCK` and RE-READS the latest persisted record inside the lock, so a
/// refresher that waited adopts a rotation a concurrent one already committed
/// instead of replaying the now-invalid old refresh token (B-1). Providers with
/// no refresh path (Cursor/Copilot/z.ai) or no refresh_token (Claude session
/// keys) leave the credential unchanged.
pub async fn refresh_if_expired(
    http: &reqwest::Client,
    cred: &crate::store::StoredCredential,
) -> Result<crate::store::StoredCredential, ProviderError> {
    if !is_expired(cred, chrono::Utc::now().timestamp_millis()) {
        return Ok(cred.clone());
    }
    let lock = stored_refresh_lock_for(&cred.id);
    let _guard = lock.lock().await;
    // Re-read: a concurrent refresher may have rotated + persisted while we waited.
    let latest = crate::store::find(&cred.id).ok_or_else(|| {
        ProviderError::NotLoggedIn(format!(
            "stored {:?} account {} was removed before refresh",
            cred.provider, cred.id
        ))
    })?;
    if !is_expired(&latest, chrono::Utc::now().timestamp_millis()) {
        return Ok(latest);
    }
    match refresh_stored(http, &latest).await {
        Some(updated) => match crate::store::update(&updated) {
            Ok(true) => Ok(updated),
            Ok(false) => Err(ProviderError::NotLoggedIn(format!(
                "stored {:?} account {} vanished before refreshed token could persist",
                updated.provider, updated.id
            ))),
            Err(e) => Err(ProviderError::Network(format!(
                "stored {:?} account {} refreshed token not persisted: {e}",
                updated.provider, updated.id
            ))),
        },
        None => Ok(latest),
    }
}

pub async fn fetch_credential(cred: &crate::store::StoredCredential) -> ServiceUsage {
    let http = crate::http::shared();
    // P0 #3: refresh expired stored tokens before fetching (serialized + re-read).
    let active = match refresh_if_expired(&http, cred).await {
        Ok(active) => active,
        Err(e) => {
            return ServiceUsage {
                id: stored_service_id(&cred.id),
                source: ServiceSource::Stored,
                provider: cred.provider,
                connected: false,
                plan: None,
                account: Some(cred.label.clone()),
                error: Some(e.into()),
                windows: vec![],
                detail_windows: vec![],
                raw_response: None,
            }
        }
    };

    let res = match active.provider {
        Provider::Codex => crate::providers::codex::fetch_stored(&http, &active).await,
        Provider::Gemini => crate::providers::gemini::fetch_stored(&http, &active).await,
        Provider::Claude => crate::providers::claude::fetch_stored(&http, &active).await,
        Provider::Copilot => crate::providers::copilot::fetch_stored(&http, &active).await,
        Provider::Zai => crate::providers::zai::fetch_stored(&http, &active).await,
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
            error: Some(e.into()),
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
        // The failed provider carries a stable, localizable error code.
        assert_eq!(
            out[1].error.as_ref().map(|e| e.code.as_str()),
            Some("not_logged_in")
        );
        assert_eq!(out[2].provider, Provider::Gemini);
        assert!(out[2].connected);
    }

    #[test]
    fn is_expired_only_for_positive_past_expiry() {
        let cred = |exp: i64| crate::store::StoredCredential {
            id: "x".into(),
            provider: Provider::Codex,
            label: "x".into(),
            access_token: "a".into(),
            refresh_token: None,
            expires_at: exp,
            id_token: None,
            account_id: None,
        };
        assert!(is_expired(&cred(100), 200)); // positive, in the past
        assert!(!is_expired(&cred(300), 200)); // in the future
        assert!(!is_expired(&cred(0), 200)); // 0 = unknown → never expired
        assert!(!is_expired(&cred(-5), 200)); // non-positive → never
        assert!(!is_expired(&cred(200), 200)); // exactly now → not yet (strict <)
    }

    #[test]
    fn stored_refresh_locks_are_keyed_by_account_id() {
        let a1 = stored_refresh_lock_for("account-a");
        let a2 = stored_refresh_lock_for("account-a");
        let b = stored_refresh_lock_for("account-b");

        assert!(
            std::sync::Arc::ptr_eq(&a1, &a2),
            "the same stored account must reuse one refresh lock"
        );
        assert!(
            !std::sync::Arc::ptr_eq(&a1, &b),
            "different stored accounts must not serialize each other's refresh"
        );
    }

    #[test]
    fn provider_error_codes_are_stable() {
        assert_eq!(
            ProviderError::NotLoggedIn("x".into()).code(),
            "not_logged_in"
        );
        assert_eq!(ProviderError::Expired("x".into()).code(), "token_expired");
        assert_eq!(ProviderError::Network("x".into()).code(), "network");
        assert_eq!(
            ProviderError::Status {
                status: 429,
                body: "rl".into()
            }
            .code(),
            "server_error"
        );
        assert_eq!(ProviderError::Parse("x".into()).code(), "parse_error");
    }

    #[test]
    fn service_error_conversion_keeps_code_and_display_detail() {
        let se: crate::model::ServiceError = ProviderError::Status {
            status: 429,
            body: "rate limited".into(),
        }
        .into();
        assert_eq!(se.code, "server_error");
        assert_eq!(
            se.detail.as_deref(),
            Some("unexpected response (429): rate limited")
        );
    }

    /// Guards the `fetch_credential` dispatch (the sole consumer of the
    /// per-provider stored-fetch adapters). Cursor is the one explicit `Err`
    /// arm (no manual accounts) and reaches no network, so it deterministically
    /// yields a disconnected, `Stored`-sourced card carrying `not_logged_in`.
    #[tokio::test]
    async fn fetch_credential_cursor_is_disconnected_not_logged_in() {
        let cred = crate::store::StoredCredential {
            id: "cur-1".into(),
            provider: Provider::Cursor,
            label: "x".into(),
            access_token: "tok".into(),
            refresh_token: None,
            expires_at: 0,
            id_token: None,
            account_id: None,
        };
        let u = fetch_credential(&cred).await;
        assert_eq!(u.provider, Provider::Cursor);
        assert!(!u.connected);
        assert_eq!(u.source, ServiceSource::Stored);
        assert_eq!(u.id, stored_service_id("cur-1"));
        assert_eq!(u.account.as_deref(), Some("x"));
        assert_eq!(
            u.error.as_ref().map(|e| e.code.as_str()),
            Some("not_logged_in")
        );
    }
}
