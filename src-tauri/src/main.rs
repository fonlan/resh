#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use resh::commands;
use resh::config::ConfigManager;
use resh::master_password::MasterPasswordManager;
use resh::logger;
use std::sync::Arc;
use tauri::Manager;
use tauri::image::Image;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Set window icon
            if let Some(window) = app.get_webview_window("main") {
                let icon_bytes = include_bytes!("../icons/icon.png");
                if let Ok(img) = image::load_from_memory(icon_bytes) {
                    let rgba = img.to_rgba8();
                    let (width, height) = rgba.dimensions();
                    let icon = Image::new_owned(rgba.into_raw(), width, height);
                    let _ = window.set_icon(icon);
                }
            }

            // Get the default app data dir (e.g., %AppData%/com.resh.ssh)
            let default_app_data_dir = app.path()
                .app_data_dir()
                .expect("failed to resolve app data dir");

            // We want %AppData%/Resh directly, so we go up one level and join "Resh"
            let app_data_dir = default_app_data_dir
                .parent()
                .map(|p| p.join("Resh"))
                .unwrap_or_else(|| default_app_data_dir.join("Resh"));

            let config_manager = ConfigManager::new(app_data_dir.clone());
            let master_password_manager = MasterPasswordManager::new(app_data_dir.clone());

            // Load initial config
            let local_config = config_manager.load_local_config().unwrap_or_else(|_| resh::config::Config::empty());
            let debug_enabled = local_config.general.debug_enabled;

            // Initialize logging
            logger::init_logging(app_data_dir.clone(), debug_enabled);
            tracing::info!("Logging initialized. Debug mode: {}", debug_enabled);

            let state = Arc::new(commands::AppState {
                config_manager,
                password_manager: master_password_manager,
                config: Mutex::new(local_config),
            });

            app.manage(state);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::config::get_config,
            commands::config::save_config,
            commands::config::trigger_sync,
            commands::config::get_app_data_dir,
            commands::config::log_event,
            commands::connection::connect_to_server,
            commands::connection::send_command,
            commands::connection::resize_terminal,
            commands::connection::close_session,
            commands::master_password::get_master_password_status,
            commands::master_password::set_master_password,
            commands::master_password::verify_master_password,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
