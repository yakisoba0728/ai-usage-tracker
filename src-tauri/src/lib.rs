pub mod anchor;
pub mod commands;
pub mod config;
pub mod http;
pub mod jwt;
pub mod login;
pub mod model;
pub mod notify;
pub mod oauth_login;
pub mod providers;
pub mod scheduler;
pub mod secrets;
pub mod store;
pub mod update;
pub mod util;

use tauri::{
    menu::{Menu, MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Emitter, Listener, Manager, WindowEvent, Wry,
};

use crate::model::{UsageSnapshot, EVENT_TRIGGER_REFRESH, EVENT_USAGE_UPDATED};

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

use crate::notify::provider_label;

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

/// Reconcile the OS login item with the persisted `launch_at_login` intent
/// (FEAT-4, spec §6.5). If the config says launch-at-login is ON but the OS
/// reports the login item is gone (a user removed it manually), re-enable it so
/// the stored preference stays the source of truth. If the config says OFF we do
/// NOT force-disable — the user may have enabled it via the OS UI directly. All
/// failures are logged and swallowed; a flaky login-item API must never block
/// startup.
fn reconcile_launch_at_login(app: &tauri::AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    if !config::AppConfig::load().launch_at_login {
        return;
    }
    let manager = app.autolaunch();
    match manager.is_enabled() {
        Ok(true) => {} // already in the desired state
        Ok(false) => {
            if let Err(e) = manager.enable() {
                eprintln!("autostart: failed to reconcile (re-enable) login item: {e}");
            }
        }
        Err(e) => eprintln!("autostart: could not query login-item state: {e}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
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
            commands::check_update_now,
            commands::set_launch_at_login,
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
                            let _ = app.emit(EVENT_TRIGGER_REFRESH, ());
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
            app.handle().listen(EVENT_USAGE_UPDATED, move |event| {
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

            // --- Launch-at-login reconcile (FEAT-4) ---
            // If the user wants launch-at-login but the OS login item was removed
            // out-of-band (e.g. manually deleted in System Settings), re-enable it
            // so the persisted intent stays authoritative. Failures are logged,
            // never fatal.
            reconcile_launch_at_login(app.handle());

            // --- Background poller ---
            scheduler::start(app.handle().clone(), config::AppConfig::load().poll_seconds);

            // --- Update notifier (FEAT-5): one check at startup + every 24h ---
            update::start(app.handle().clone());
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
