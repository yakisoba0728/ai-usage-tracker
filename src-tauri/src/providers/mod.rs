//! Provider trait + shared error type + parallel `fetch_all`.

pub mod claude;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod gemini;

use async_trait::async_trait;
use futures::future::join_all;

use crate::model::{Provider, ServiceUsage};

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
    let futs = providers
        .into_iter()
        .map(|p| async move {
            let key = p.key();
            match p.fetch().await {
                Ok(u) => u,
                Err(e) => ServiceUsage {
                    provider: key,
                    connected: false,
                    plan: None,
                    account: None,
                    error: Some(e.to_string()),
                    windows: vec![],
                },
            }
        });
    join_all(futs).await
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
                provider: self.0,
                connected: true,
                plan: Some("pro".into()),
                account: None,
                error: None,
                windows: vec![],
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
