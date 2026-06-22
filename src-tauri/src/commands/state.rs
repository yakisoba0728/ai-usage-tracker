//! Managed-state types + their constructors, plus the provider-constructor
//! table. These are the `.manage()`d stores `lib.rs` wires into the Tauri app
//! and the canonical auto-detect provider list the refresh pipeline builds from.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::model::UsageSnapshot;
use crate::providers::ProviderApi;

pub type SnapshotStore = Arc<RwLock<UsageSnapshot>>;
pub type ConfigStore = Arc<RwLock<AppConfig>>;

pub fn empty_snapshot_store() -> SnapshotStore {
    Arc::new(RwLock::new(UsageSnapshot {
        fetched_at: 0,
        services: vec![],
    }))
}

pub fn default_config_store() -> ConfigStore {
    Arc::new(RwLock::new(AppConfig::load()))
}

/// Auto-detect provider constructors in canonical order. The index lines up
/// positionally with `AppConfig::enabled_array()` / `PROVIDER_ORDER`, so adding
/// a provider is one row here — no hand-counted indices to keep in sync. Local
/// parsing (keychain / credential files / env) runs for ALL providers; Claude
/// additionally supports a pasted session key via "Add account".
pub(crate) const PROVIDER_CTORS: [fn() -> Box<dyn ProviderApi>; 6] = [
    || Box::new(crate::providers::claude::ClaudeProvider::new()),
    || Box::new(crate::providers::codex::CodexProvider::new()),
    || Box::new(crate::providers::gemini::GeminiProvider::new()),
    || Box::new(crate::providers::copilot::CopilotProvider::new()),
    || Box::new(crate::providers::cursor::CursorProvider::new()),
    || Box::new(crate::providers::zai::ZaiProvider::new()),
];
