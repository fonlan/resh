#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod config;
mod ssh_manager;
mod master_password;
mod webdav;

use commands::AppState;
use config::ConfigManager;
use master_password::MasterPasswordManager;
use std::sync::Arc;
use tauri::Manager;
use tauri::image::Image;

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

            let state = Arc::new(AppState {
                config_manager,
                password_manager: master_password_manager,
            });

            app.manage(state);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::config::get_merged_config,
            commands::config::save_config,
            commands::config::get_app_data_dir,
            commands::connection::connect_to_server,
            commands::connection::send_command,
            commands::connection::resize_terminal,
            commands::connection::close_session,
            commands::master_password::get_master_password_status,
            commands::master_password::set_master_password,
            commands::master_password::verify_master_password,
            commands::sync::sync_webdav,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
