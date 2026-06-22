pub mod anchor;
pub mod commands;
pub mod config;
pub mod http;
pub mod jwt;
pub mod login;
pub mod model;
pub mod oauth_login;
pub mod providers;
pub mod scheduler;
pub mod secrets;
pub mod store;
pub mod util;

use tauri::{
    menu::{Menu, MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Emitter, Listener, Manager, WindowEvent, Wry,
};

use crate::model::{Provider, UsageSnapshot};

/// Toggle the macOS Dock presence: `Regular` shows the Dock icon (a normal
/// app), `Accessory` hides it so the app lives only in the menu bar. No-op off
/// macOS. We flip to Accessory whenever the main window is closed, and back to
/// Regular when it's reopened from the tray menu.
#[cfg(target_os = "macos")]
fn set_dock_visible(app: &tauri::AppHandle, visible: bool) {
    let policy = if visible {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    };
    let _ = app.set_activation_policy(policy);
}

#[cfg(not(target_os = "macos"))]
fn set_dock_visible(_app: &tauri::AppHandle, _visible: bool) {}

/// Show the full dashboard window (+ Dock icon) from a tray menu item.
fn open_dashboard(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
    set_dock_visible(app, true);
}

/// Canonical display name for a provider (matches the frontend `PROVIDER_LABEL`).
fn provider_label(p: Provider) -> &'static str {
    match p {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
        Provider::Gemini => "Gemini",
        Provider::Copilot => "GitHub Copilot",
        Provider::Cursor => "Cursor",
        Provider::Zai => "z.ai",
    }
}

/// Build the native tray menu: one (disabled, info-only) row per connected
/// provider showing its headline usage, then the actions. Rebuilt on every
/// usage refresh so the menu always shows live numbers — this is the real macOS
/// `NSMenu`, not a webview, so it looks/behaves exactly like the system one.
fn build_tray_menu(
    app: &tauri::AppHandle,
    snapshot: Option<&UsageSnapshot>,
) -> tauri::Result<Menu<Wry>> {
    let mut mb = MenuBuilder::new(app);

    match snapshot {
        None => {
            mb = mb.item(
                &MenuItemBuilder::with_id("status:loading", "Fetching usage…")
                    .enabled(false)
                    .build(app)?,
            );
        }
        Some(snap) => {
            let connected: Vec<_> = snap.services.iter().filter(|s| s.connected).collect();
            if connected.is_empty() {
                mb = mb.item(
                    &MenuItemBuilder::with_id("status:none", "No connected providers")
                        .enabled(false)
                        .build(app)?,
                );
            } else {
                for s in connected {
                    let name = provider_label(s.provider);
                    let pct = s.windows.first().and_then(|w| w.used_percent);
                    // Show REMAINING headroom (100 − used) so an unused plan
                    // reads as 100% rather than 0%.
                    let label = match pct {
                        Some(p) => {
                            let remaining = (100.0 - p).round().clamp(0.0, 100.0) as i64;
                            format!("{name}  ·  {remaining}%")
                        }
                        None => name.to_string(),
                    };
                    mb = mb.item(
                        &MenuItemBuilder::with_id(format!("svc:{}", s.id), label).build(app)?,
                    );
                }
            }
        }
    }

    mb.separator()
        .text("refresh", "Refresh now")
        .text("show", "Show dashboard")
        .separator()
        .text("quit", "Quit")
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(commands::empty_snapshot_store())
        .manage(commands::default_config_store())
        .invoke_handler(tauri::generate_handler![
            commands::get_usage,
            commands::refresh_now,
            commands::get_config,
            commands::set_config,
            commands::start_login,
            commands::list_accounts,
            commands::remove_account,
            commands::login_oauth,
            commands::add_session_key,
            commands::cancel_login,
            commands::send_anchor_now,
            commands::refresh_account,
            commands::rename_account,
        ])
        .setup(|app| {
            // --- Tray with a native menu (usage rows + actions) ---
            let menu = build_tray_menu(app.handle(), None)?;

            let mut builder = TrayIconBuilder::with_id("main").tooltip("AI Usage Tracker");
            if let Some(ic) = app.default_window_icon() {
                builder = builder.icon(ic.clone());
            }
            builder
                .menu(&menu)
                // Show the native menu on a normal click (the system behavior).
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| {
                    let id = event.id().as_ref();
                    match id {
                        "show" => open_dashboard(app),
                        "refresh" => {
                            let _ = app.emit("trigger-refresh", ());
                        }
                        "quit" => app.exit(0),
                        // Clicking a provider usage row opens the dashboard.
                        _ if id.starts_with("svc:") => open_dashboard(app),
                        _ => {}
                    }
                })
                .build(app)?;

            // Rebuild the tray menu with live numbers on every usage refresh.
            let handle = app.handle().clone();
            app.handle().listen("usage-updated", move |event| {
                let Ok(snap) = serde_json::from_str::<UsageSnapshot>(event.payload()) else {
                    return;
                };
                let h = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    if let Some(tray) = h.tray_by_id("main") {
                        if let Ok(menu) = build_tray_menu(&h, Some(&snap)) {
                            let _ = tray.set_menu(Some(menu));
                        }
                    }
                });
            });

            // Launch into the menu bar only — no Dock icon until the main window
            // is opened from the tray menu.
            set_dock_visible(app.handle(), false);

            // --- Background poller ---
            scheduler::start(app.handle().clone(), config::AppConfig::load().poll_seconds);
            Ok(())
        })
        .on_window_event(|window, event| {
            // close-to-tray: the main window hides instead of closing, dropping
            // the app back to the menu bar (Dock icon removed, tray kept).
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                    set_dock_visible(window.app_handle(), false);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
