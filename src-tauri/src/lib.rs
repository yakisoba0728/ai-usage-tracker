pub mod commands;
pub mod config;
pub mod http;
pub mod jwt;
pub mod keychain;
pub mod login;
pub mod model;
pub mod oauth_login;
pub mod providers;
pub mod scheduler;
pub mod secrets;
pub mod store;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, LogicalPosition, Manager, WindowEvent,
};

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
        ])
        .setup(|app| {
            // --- Tray popover window (borderless, hidden initially) ---
            let popover = tauri::WebviewWindowBuilder::new(
                app,
                "popover",
                tauri::WebviewUrl::App("index.html?window=popover".into()),
            )
            .title("")
            .decorations(false)
            .skip_taskbar(true)
            .resizable(false)
            .visible(false)
            .inner_size(340.0, 440.0)
            .build()?;
            let _ = popover.set_position(LogicalPosition::new(1e6, 1e6)); // off-screen until first toggle

            // --- Tray (icon only, no title) ---
            let show = MenuItem::with_id(app, "show", "Show dashboard", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let refresh_item =
                MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&refresh_item, &show, &quit])?;

            let mut builder = TrayIconBuilder::with_id("main").tooltip("AI Usage Tracker");
            if let Some(ic) = app.default_window_icon() {
                builder = builder.icon(ic.clone());
            }
            builder
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "refresh" => {
                        let _ = app.emit("trigger-refresh", ());
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        // Left-click toggles the custom popover (not the
                        // main dashboard window). The popover is a borderless
                        // window positioned just below the tray icon.
                        if let Some(popover) = app.get_webview_window("popover") {
                            if popover.is_visible().unwrap_or(false) {
                                let _ = popover.hide();
                            } else {
                                // Position below the tray icon. On macOS the
                                // tray lives in the menu bar at the top of the
                                // screen; place the popover just below it,
                                // right-aligned to the icon area.
                                if let Some(monitor) = popover.primary_monitor().ok().flatten() {
                                    let mon = monitor.size();
                                    let mon_pos = monitor.position();
                                    let _ = popover.set_position(LogicalPosition::new(
                                        mon_pos.x as f64
                                            + mon.width as f64 / monitor.scale_factor()
                                            - 360.0,
                                        (mon_pos.y as f64) / monitor.scale_factor() + 6.0,
                                    ));
                                }
                                let _ = popover.show();
                                let _ = popover.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // --- Background poller ---
            scheduler::start(app.handle().clone(), config::AppConfig::load().poll_seconds);
            Ok(())
        })
        .on_window_event(|window, event| {
            let label = window.label();
            // close-to-tray: main window hides instead of closing.
            // Popover always hides (it has no close button — clicking
            // outside or pressing Escape dismisses it).
            if label == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
            } else if label == "popover" {
                if let WindowEvent::Focused(false) = event {
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
