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

/// Drop a stored account's refresh lock so the map doesn't grow forever across
/// add/remove churn (UG-1). Keyed by the RAW credential id. Called from
/// `remove_account` once removal actually happened.
pub fn forget_stored_refresh_lock(id: &str) {
    STORED_REFRESH_LOCKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(id);
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
    fn forget_stored_refresh_lock_shrinks_the_map() {
        // Unique key so this can't race the parallel keyed-by-id test (same
        // process-global static).
        let id = "acct-forget-unique-xyz";
        forget_stored_refresh_lock(id); // start clean

        let a1 = stored_refresh_lock_for(id);
        let a1b = stored_refresh_lock_for(id);
        assert!(
            std::sync::Arc::ptr_eq(&a1, &a1b),
            "before forgetting, the entry is cached and reused"
        );

        forget_stored_refresh_lock(id);
        let a2 = stored_refresh_lock_for(id);
        assert!(
            !std::sync::Arc::ptr_eq(&a1, &a2),
            "forgetting removed the entry, so a fresh Arc is minted — map shrank"
        );

        forget_stored_refresh_lock(id); // cleanup
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

    /// B-1 end-to-end regression: two (here N=8) concurrent `refresh_if_expired`
    /// calls on the SAME expired credential must (a) hit the underlying OAuth
    /// token endpoint exactly ONCE — the per-id lock + re-read-inside-lock means
    /// the 7 waiters adopt the rotation the first caller committed instead of
    /// replaying the now-rotated-away refresh token — and (b) every caller
    /// returns the rotated access token, which (c) is what's persisted on disk.
    ///
    /// Faithful characterization, not a synthetic stub: we seed a real expired
    /// `StoredCredential` via `store::add` (temp `AIT_ACCOUNTS_PATH`) and point
    /// Gemini's refresh POST at a wiremock token endpoint (via the
    /// `GEMINI_OAUTH_TOKEN_URL` env override — same category as the existing
    /// `GEMINI_OAUTH_CLIENT_ID/_SECRET` hooks; production defaults to the real
    /// URL). Gemini is chosen over Codex because it derives `expires_at` from
    /// `now + expires_in` — a mock `{access_token, expires_in: 3600}` yields a
    /// genuine future expiry, so the in-lock re-read short-circuit fires for the
    /// 7 waiters exactly as it would in production. The mock counts hits, so
    /// `received_requests().len() == 1` is the deterministic single-refresh pin.
    // The process-global env vars (AIT_ACCOUNTS_PATH + GEMINI_OAUTH_TOKEN_URL)
    // must stay set across every `.await` in this test, so the `STORE_TEST_ENV_LOCK`
    // (a `std::sync::Mutex`, matching every other store-touching test) is
    // deliberately held across await points to keep the env isolation intact —
    // this is single-threaded-with-respect-to-the-store test setup, not a
    // production lock, so the await-holding-lock concern (cross-task deadlock /
    // contention) doesn't apply.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_refresh_refreshes_once_and_all_adopt_rotation() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        // Both AIT_ACCOUNTS_PATH and GEMINI_OAUTH_TOKEN_URL are process-global,
        // so serialize against the other store-touching tests.
        let _g = crate::store::STORE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let server = MockServer::start().await;
        // Each successful refresh rotates to a fresh access/refresh token pair
        // with a 1h future expiry, so the re-read inside the lock sees a
        // non-expired record and short-circuits without a second POST.
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ROTATED-ACCESS",
                "refresh_token": "ROTATED-REFRESH",
                "expires_in": 3600,
            })))
            .mount(&server)
            .await;

        let accounts_path = std::env::temp_dir()
            .join(format!("ait_b1_concurrent_{}.json", std::process::id()));
        let _ = std::fs::remove_file(&accounts_path);
        std::env::set_var("AIT_ACCOUNTS_PATH", &accounts_path);
        std::env::set_var("GEMINI_OAUTH_TOKEN_URL", format!("{}/token", server.uri()));

        // Seed an already-expired stored Gemini credential (expires_at in the
        // past, positive → `is_expired` true).
        let id = crate::store::add(crate::store::StoredCredential {
            id: String::new(),
            provider: Provider::Gemini,
            label: "b1-user@example.invalid".into(),
            access_token: "STALE-ACCESS".into(),
            refresh_token: Some("STALE-REFRESH".into()),
            expires_at: 1, // positive + in the past → expired
            id_token: None,
            account_id: None,
        })
        .unwrap();
        let seed = crate::store::find(&id).unwrap();

        // Fire N=8 concurrent refreshes of the SAME expired credential.
        let http = crate::http::shared();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let http = http.clone();
                let cred = seed.clone();
                tokio::spawn(async move { refresh_if_expired(&http, &cred).await })
            })
            .collect();

        let mut results = Vec::new();
        for h in handles {
            results.push(h.await.unwrap());
        }

        // (a) The underlying token endpoint was hit exactly once: lock + re-read
        // collapsed 8 concurrent refreshes into a single rotation.
        let hits = server.received_requests().await.unwrap();
        let refresh_hits = hits
            .iter()
            .filter(|r| r.url.path() == "/token")
            .count();
        assert_eq!(
            refresh_hits, 1,
            "the OAuth token endpoint must be hit exactly once for 8 concurrent refreshes (B-1)"
        );

        // (b) Every caller returns the rotated access token — no waiter replayed
        // the stale token or errored out.
        for (i, r) in results.iter().enumerate() {
            let cred = r
                .as_ref()
                .unwrap_or_else(|e| panic!("refresh #{i} errored: {e}"));
            assert_eq!(
                cred.access_token, "ROTATED-ACCESS",
                "refresh #{i} must adopt the rotated access token"
            );
            assert_eq!(
                cred.refresh_token.as_deref(),
                Some("ROTATED-REFRESH"),
                "refresh #{i} must adopt the rotated refresh token"
            );
        }

        // (c) The persisted record equals the rotation (re-read path reads it back).
        let persisted = crate::store::find(&id).unwrap();
        assert_eq!(persisted.access_token, "ROTATED-ACCESS");
        assert_eq!(persisted.refresh_token.as_deref(), Some("ROTATED-REFRESH"));
        assert!(
            persisted.expires_at > chrono::Utc::now().timestamp_millis(),
            "rotated expiry must be in the future so waiters short-circuit"
        );

        std::env::remove_var("AIT_ACCOUNTS_PATH");
        std::env::remove_var("GEMINI_OAUTH_TOKEN_URL");
        let _ = std::fs::remove_file(&accounts_path);
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
