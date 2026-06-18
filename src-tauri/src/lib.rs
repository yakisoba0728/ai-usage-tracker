pub mod config;
pub mod http;
pub mod jwt;
pub mod model;
pub mod providers;
pub mod secrets;

// Re-export the scaffold's mobile entry point so the app still runs while the
// integration layer is wired up in a later step.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
