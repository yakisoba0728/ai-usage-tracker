pub mod commands;
pub mod config;
pub mod http;
pub mod jwt;
pub mod model;
pub mod providers;
pub mod scheduler;
pub mod secrets;
pub mod cli_login;
pub mod login;
pub mod store;
pub mod oauth_login;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
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
            commands::login_via_cli,
            commands::exchange_code,
            commands::login_options,
            commands::add_session_key,
        ])
        .setup(|app| {
            // --- Tray ---
            let show = MenuItem::with_id(app, "show", "Show dashboard", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("AI Usage Tracker")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
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
                        if let Some(w) = app.get_webview_window("main") {
                            if w.is_visible().unwrap_or(false) {
                                let _ = w.hide();
                            } else {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // --- Background poller ---
            scheduler::start(
                app.handle().clone(),
                config::AppConfig::default().poll_seconds,
            );
            Ok(())
        })
        .on_window_event(|window, event| {
            // close-to-tray: never actually close, just hide
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
