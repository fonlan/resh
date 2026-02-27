// src-tauri/src/commands/config.rs

use crate::ai::manager::AiManager;
use crate::config::{Config, ConfigManager, SyncManager};
use crate::db::DatabaseManager;
use crate::master_password::MasterPasswordManager;
use crate::sftp_manager::edit::SftpEditManager;
use std::sync::Arc;
use tauri::{Emitter, State, Window};
use tokio::sync::Mutex;

use dashmap::DashMap;
use tokio_util::sync::CancellationToken;

pub struct AppState {
    pub config_manager: ConfigManager,
    pub password_manager: MasterPasswordManager,
    pub db_manager: DatabaseManager,
    pub config: Mutex<Config>,
    pub ai_cancellation_tokens: DashMap<String, CancellationToken>,
    pub ai_manager: AiManager,
    pub sftp_edit_manager: SftpEditManager,
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
    window: Window,
) -> Result<(), String> {
    let mut current_config = state.config.lock().await;

    // Calculate removed IDs to prevent them from being resurrected by sync
    let removed_ids = find_removed_ids(&current_config, &config);

    *current_config = config.clone();

    let local_path = state.config_manager.local_config_path();
    state.config_manager.save_config(&config, &local_path)?;
    tracing::debug!("Local config saved to {:?}", local_path);

    // Update log level
    crate::logger::set_log_level(config.general.debug_enabled);

    // If sync is enabled, trigger async sync in background without blocking
    // Local save is already complete, sync failures should not block the operation
    if config.general.webdav.enabled && !config.general.webdav.url.is_empty() {
        let proxy = config
            .general
            .webdav
            .proxy_id
            .as_ref()
            .and_then(|id| config.proxies.iter().find(|p| &p.id == id).cloned());

        let sync_manager = SyncManager::new(
            config.general.webdav.url.clone(),
            config.general.webdav.username.clone(),
            config.general.webdav.password.clone(),
            proxy,
        );

        let app_state = state.inner().clone();
        let sync_config = config.clone();
        let sync_removed_ids = removed_ids;
        let sync_path = local_path.clone();

        let window = window.clone();
        tokio::spawn(async move {
            let mut local_copy = sync_config;
            if let Err(e) = sync_manager.sync(&mut local_copy, sync_removed_ids).await {
                tracing::warn!("Background sync failed: {}", e);
                let _ = window.emit("sync-failed", e);
            } else {
                if let Err(e) = app_state
                    .config_manager
                    .save_config(&local_copy, &sync_path)
                {
                    tracing::error!("Failed to save merged config after sync: {}", e);
                }

                {
                    let mut in_memory = app_state.config.lock().await;
                    *in_memory = local_copy.clone();
                }

                let _ = window.emit("config-updated", local_copy);
            }
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn trigger_sync(state: State<'_, Arc<AppState>>) -> Result<Config, String> {
    let mut config = state.config.lock().await;
    if !config.general.webdav.enabled || config.general.webdav.url.is_empty() {
        return Err("WebDAV sync is not enabled or configured".to_string());
    }

    let proxy = config
        .general
        .webdav
        .proxy_id
        .as_ref()
        .and_then(|id| config.proxies.iter().find(|p| &p.id == id).cloned());

    let sync_manager = SyncManager::new(
        config.general.webdav.url.clone(),
        config.general.webdav.username.clone(),
        config.general.webdav.password.clone(),
        proxy,
    );

    sync_manager.sync(&mut config, vec![]).await?;

    let local_path = state.config_manager.local_config_path();
    state.config_manager.save_config(&config, &local_path)?;

    Ok(config.clone())
}

fn find_removed_ids(old: &Config, new: &Config) -> Vec<String> {
    let mut removed = Vec::new();

    let new_server_ids: Vec<&String> = new.servers.iter().map(|s| &s.id).collect();
    for server in &old.servers {
        if !new_server_ids.contains(&&server.id) {
            removed.push(server.id.clone());
        }
    }

    let new_auth_ids: Vec<&String> = new.authentications.iter().map(|a| &a.id).collect();
    for auth in &old.authentications {
        if !new_auth_ids.contains(&&auth.id) {
            removed.push(auth.id.clone());
        }
    }

    let new_proxy_ids: Vec<&String> = new.proxies.iter().map(|p| &p.id).collect();
    for proxy in &old.proxies {
        if !new_proxy_ids.contains(&&proxy.id) {
            removed.push(proxy.id.clone());
        }
    }

    let new_snippet_ids: Vec<&String> = new.snippets.iter().map(|s| &s.id).collect();
    for snippet in &old.snippets {
        if !new_snippet_ids.contains(&&snippet.id) {
            removed.push(snippet.id.clone());
        }
    }

    let new_channel_ids: Vec<&String> = new.ai_channels.iter().map(|c| &c.id).collect();
    for channel in &old.ai_channels {
        if !new_channel_ids.contains(&&channel.id) {
            removed.push(channel.id.clone());
        }
    }

    let new_model_ids: Vec<&String> = new.ai_models.iter().map(|m| &m.id).collect();
    for model in &old.ai_models {
        if !new_model_ids.contains(&&model.id) {
            removed.push(model.id.clone());
        }
    }

    removed
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
