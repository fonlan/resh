// src-tauri/src/commands/config.rs

use crate::config::{Config, ConfigManager};
use crate::master_password::MasterPasswordManager;
use std::sync::Arc;
use tauri::State;

pub struct AppState {
    pub config_manager: ConfigManager,
    pub password_manager: MasterPasswordManager,
}

#[tauri::command]
pub async fn get_merged_config(state: State<'_, Arc<AppState>>) -> Result<Config, String> {
    let sync_config = state.config_manager.load_sync_config()?;
    let local_config = state.config_manager.load_local_config()?;
    Ok(state.config_manager.merge_configs(sync_config, local_config))
}

#[tauri::command]
pub async fn save_config(
    sync_part: Config,
    local_part: Config,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let sync_path = state.config_manager.sync_config_path();
    let local_path = state.config_manager.local_config_path();

    state.config_manager.save_config(&sync_part, &sync_path)?;
    state.config_manager.save_config(&local_part, &local_path)?;

    // Update log level if it changed in local_part
    crate::logger::set_log_level(local_part.general.debug_enabled);

    Ok(())
}

#[tauri::command]
pub async fn log_event(level: String, message: String) {
    match level.as_str() {
        "trace" => tracing::trace!("{}", message),
        "debug" => tracing::debug!("{}", message),
        "info" => tracing::info!("{}", message),
        "warn" => tracing::warn!("{}", message),
        "error" => tracing::error!("{}", message),
        _ => tracing::info!("{}", message),
    }
}

#[allow(dead_code)]
#[tauri::command]
pub async fn get_merged_config_encrypted(
    password: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Config, String> {
    let sync_config = state.config_manager.load_encrypted_sync_config(&password)?;
    let local_config = state.config_manager.load_encrypted_local_config(&password)?;
    Ok(state.config_manager.merge_configs(sync_config, local_config))
}

#[allow(dead_code)]
#[tauri::command]
pub async fn save_config_encrypted(
    sync_part: Config,
    local_part: Config,
    password: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state
        .config_manager
        .save_encrypted_sync_config(&sync_part, &password)?;
    state
        .config_manager
        .save_encrypted_local_config(&local_part, &password)?;

    // Update log level if it changed in local_part
    crate::logger::set_log_level(local_part.general.debug_enabled);

    Ok(())
}

#[tauri::command]
pub async fn get_app_data_dir(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    Ok(state
        .config_manager
        .sync_config_path()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string())
}
