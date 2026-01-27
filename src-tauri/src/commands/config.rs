// src-tauri/src/commands/config.rs

use crate::config::{Config, ConfigManager, SyncManager};
use crate::master_password::MasterPasswordManager;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

pub struct AppState {
    pub config_manager: ConfigManager,
    pub password_manager: MasterPasswordManager,
    pub config: Mutex<Config>,
}

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<Config, String> {
    let config = state.config.lock().await;
    Ok(config.clone())
}

#[tauri::command]
pub async fn save_config(
    config: Config,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut current_config = state.config.lock().await;
    *current_config = config.clone();
    
    let local_path = state.config_manager.local_config_path();
    state.config_manager.save_config(&config, &local_path)?;

    // Update log level
    crate::logger::set_log_level(config.general.debug_enabled);

    // If sync is enabled, we could trigger sync here or wait for user to click sync
    // Given the "automatic trigger" recommendation accepted by user:
    if config.general.webdav.enabled && !config.general.webdav.url.is_empty() {
        let sync_manager = SyncManager::new(
            config.general.webdav.url.clone(),
            config.general.webdav.username.clone(),
            config.general.webdav.password.clone(),
        );
        // Run sync in background or await? 
        // Better to await to ensure sync completes before returning success if it's "save and sync"
        let mut local_copy = config.clone();
        if let Err(e) = sync_manager.sync(&mut local_copy).await {
            tracing::error!("Sync failed: {}", e);
            // We don't necessarily want to fail the save if sync fails, 
            // but maybe return a specific error or warning?
            // For now, let's just log and continue.
        } else {
            *current_config = local_copy;
            state.config_manager.save_config(&current_config, &local_path)?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn trigger_sync(state: State<'_, Arc<AppState>>) -> Result<Config, String> {
    let mut config = state.config.lock().await;
    if !config.general.webdav.enabled || config.general.webdav.url.is_empty() {
        return Err("WebDAV sync is not enabled or configured".to_string());
    }

    let sync_manager = SyncManager::new(
        config.general.webdav.url.clone(),
        config.general.webdav.username.clone(),
        config.general.webdav.password.clone(),
    );

    sync_manager.sync(&mut config).await?;
    
    let local_path = state.config_manager.local_config_path();
    state.config_manager.save_config(&config, &local_path)?;
    
    Ok(config.clone())
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

#[tauri::command]
pub async fn get_app_data_dir(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    Ok(state
        .config_manager
        .local_config_path()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string())
}
