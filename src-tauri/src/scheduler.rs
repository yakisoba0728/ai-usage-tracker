//! Background polling loop. A generation counter lets `restart` supersede an
//! older loop so config changes take effect without overlapping fetches.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::commands::{refresh_once, ConfigStore, SnapshotStore};

static GENERATION: AtomicU64 = AtomicU64::new(0);

fn next_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

fn is_current(my_gen: u64) -> bool {
    my_gen == GENERATION.load(Ordering::SeqCst)
}

pub fn start(app: AppHandle, poll_seconds: u64) {
    let my_gen = next_generation();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let snap = app.state::<SnapshotStore>().inner().clone();
        let cfg = app.state::<ConfigStore>().inner().clone();
        // Skip even the immediate fetch if a newer loop already superseded us,
        // so a rapid restart can't run two refreshes concurrently.
        if is_current(my_gen) {
            refresh_once(&app, &cfg, &snap).await;
        }
        loop {
            tokio::time::sleep(Duration::from_secs(poll_seconds.max(30))).await;
            if !is_current(my_gen) {
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

#[cfg(test)]
mod tests {
    use super::{is_current, next_generation};

    #[test]
    fn newer_generation_supersedes_older() {
        let g1 = next_generation();
        assert!(is_current(g1), "a freshly minted generation is current");
        let g2 = next_generation();
        assert!(g2 > g1, "generation strictly increases");
        assert!(!is_current(g1), "an older generation is no longer current");
        assert!(is_current(g2), "the newest generation is current");
    }
}
