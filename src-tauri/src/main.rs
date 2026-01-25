#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod config;
mod ssh_manager;

use commands::AppState;
use config::ConfigManager;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let app_data_dir = tauri::api::path::app_data_dir(
        &tauri::Config::default(),
        &Default::default(),
    )
    .expect("failed to resolve app data dir")
    .join("Resh");

    let config_manager = ConfigManager::new(app_data_dir);
    let state = Arc::new(AppState { config_manager });

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::config::get_merged_config,
            commands::config::save_config,
            commands::config::get_app_data_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
