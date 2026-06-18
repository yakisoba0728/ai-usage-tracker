//! Background polling loop. A generation counter lets `restart` supersede an
//! older loop so config changes take effect without overlapping fetches.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::commands::{refresh_once, ConfigStore, SnapshotStore};

static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn start(app: AppHandle, poll_seconds: u64) {
    let my_gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let snap = app
            .state::<SnapshotStore>()
            .inner()
            .clone();
        let cfg = app.state::<ConfigStore>().inner().clone();
        // immediate first fetch, then poll
        refresh_once(&app, &cfg, &snap).await;
        loop {
            tokio::time::sleep(Duration::from_secs(poll_seconds.max(30))).await;
            if my_gen != GENERATION.load(Ordering::SeqCst) {
                break; // a newer loop has taken over
            }
            refresh_once(&app, &cfg, &snap).await;
        }
    });
}

/// Start a fresh loop (incrementing generation) so any prior loop exits.
pub fn restart(app: &AppHandle, poll_seconds: u64) {
    start(app.clone(), poll_seconds);
}
