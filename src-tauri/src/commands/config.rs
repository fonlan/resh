// src-tauri/src/commands/config.rs

use crate::ai::manager::AiManager;
use crate::app_paths::{resolve_app_data_dir_from_default, APP_DATA_DIR_NAME};
use crate::config::{Config, ConfigManager, SyncManager};
use crate::db::DatabaseManager;
use crate::sftp_manager::edit::SftpEditManager;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, Window};
use tokio::sync::Mutex;

use dashmap::DashMap;
use tokio_util::sync::CancellationToken;

/// Active AI run for a session: one current request + its cancellation token.
#[derive(Clone, Debug)]
pub struct AiRunEntry {
    pub request_id: String,
    pub token: CancellationToken,
}

/// Minimal registry surface shared by AppState and unit tests.
#[derive(Default)]
pub struct AiRunRegistry {
    entries: DashMap<String, AiRunEntry>,
}

impl AiRunRegistry {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    /// Register a new AI run. Cancels any previous run for the same session first.
    pub fn register(&self, session_id: &str, request_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        if let Some((_, previous)) = self.entries.remove(session_id) {
            previous.token.cancel();
        }
        self.entries.insert(
            session_id.to_string(),
            AiRunEntry {
                request_id: request_id.to_string(),
                token: token.clone(),
            },
        );
        token
    }

    /// Cancel only when the session's current run matches `request_id`.
    pub fn cancel(&self, session_id: &str, request_id: &str) -> bool {
        if let Some(entry) = self.entries.get(session_id) {
            if entry.request_id == request_id {
                entry.token.cancel();
                return true;
            }
        }
        false
    }

    /// Remove the registry entry only if it still belongs to this request.
    pub fn clear_if_matches(&self, session_id: &str, request_id: &str) {
        self.entries
            .remove_if(session_id, |_, entry| entry.request_id == request_id);
    }

    pub fn current_request_id(&self, session_id: &str) -> Option<String> {
        self.entries
            .get(session_id)
            .map(|entry| entry.request_id.clone())
    }
}

pub struct AppState {
    pub config_manager: ConfigManager,
    pub db_manager: DatabaseManager,
    pub config: Mutex<Config>,
    /// session_id -> current AI run (request_id + token). At most one run per session.
    pub ai_cancellation_tokens: AiRunRegistry,
    pub ai_manager: AiManager,
    pub sftp_edit_manager: SftpEditManager,
}

impl AppState {
    pub fn register_ai_run(&self, session_id: &str, request_id: &str) -> CancellationToken {
        self.ai_cancellation_tokens.register(session_id, request_id)
    }

    pub fn cancel_ai_run(&self, session_id: &str, request_id: &str) -> bool {
        self.ai_cancellation_tokens.cancel(session_id, request_id)
    }

    pub fn clear_ai_run_if_matches(&self, session_id: &str, request_id: &str) {
        self.ai_cancellation_tokens
            .clear_if_matches(session_id, request_id);
    }
}

/// Drops by clearing only this request's registry entry (safe under request replacement).
pub struct AiRunGuard {
    state: Arc<AppState>,
    session_id: String,
    request_id: String,
}

impl AiRunGuard {
    pub fn new(state: Arc<AppState>, session_id: String, request_id: String) -> Self {
        Self {
            state,
            session_id,
            request_id,
        }
    }
}

impl Drop for AiRunGuard {
    fn drop(&mut self) {
        self.state
            .clear_ai_run_if_matches(&self.session_id, &self.request_id);
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackendSmokeCheck {
    pub status: &'static str,
    pub app_data_dir_name: &'static str,
}

#[tauri::command]
pub async fn backend_smoke_check() -> Result<BackendSmokeCheck, String> {
    Ok(BackendSmokeCheck {
        status: "ok",
        app_data_dir_name: APP_DATA_DIR_NAME,
    })
}

fn resolve_app_data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let default_app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    Ok(resolve_app_data_dir_from_default(&default_app_data_dir))
}

#[tauri::command]
pub async fn get_config(app: AppHandle) -> Result<Config, String> {
    let app_data_dir = resolve_app_data_dir(&app)?;
    let config_manager = ConfigManager::new(app_data_dir);
    config_manager.load_local_config()
}

#[tauri::command]
pub async fn save_config(
    mut config: Config,
    state: State<'_, Arc<AppState>>,
    window: Window,
) -> Result<(), String> {
    if config.normalize_legacy_defaults() {
        tracing::info!(
            transfer_profile = %config.general.sftp.transfer_profile,
            migrated_download_max_inflight = config.general.sftp.download_max_inflight,
            migrated_chunk_size_min = config.general.sftp.chunk_size_min,
            "normalized legacy SFTP throughput defaults before saving config"
        );
    }

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

fn apply_server_connection(config: &mut Config, server_id: &str) -> Result<(), String> {
    if !config.servers.iter().any(|server| server.id == server_id) {
        return Err(format!("Server not found: {}", server_id));
    }

    config
        .general
        .recent_server_ids
        .retain(|id| id != server_id);
    config
        .general
        .recent_server_ids
        .insert(0, server_id.to_string());

    let limit = std::cmp::max(20, config.general.max_recent_servers.saturating_mul(2)) as usize;
    config.general.recent_server_ids.truncate(limit);

    let count = config
        .general
        .server_connection_counts
        .entry(server_id.to_string())
        .or_insert(0);
    *count = count.saturating_add(1);

    Ok(())
}

#[tauri::command]
pub async fn record_server_connection(
    server_id: String,
    state: State<'_, Arc<AppState>>,
) -> Result<Config, String> {
    let mut config = state.config.lock().await;
    apply_server_connection(&mut config, &server_id)?;
    state.config_manager.save_local_config(&config)?;
    Ok(config.clone())
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
    if config.normalize_legacy_defaults() {
        tracing::info!(
            transfer_profile = %config.general.sftp.transfer_profile,
            migrated_download_max_inflight = config.general.sftp.download_max_inflight,
            migrated_chunk_size_min = config.general.sftp.chunk_size_min,
            "normalized legacy SFTP throughput defaults after sync merge"
        );
    }

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
pub async fn get_app_data_dir(app: AppHandle) -> Result<String, String> {
    Ok(resolve_app_data_dir(&app)?.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn backend_smoke_command_returns_stable_payload() {
        let payload = backend_smoke_check().await.unwrap();

        assert_eq!(
            payload,
            BackendSmokeCheck {
                status: "ok",
                app_data_dir_name: "Resh",
            }
        );
    }
    fn sample_server(id: &str) -> crate::config::types::Server {
        crate::config::types::Server {
            id: id.to_string(),
            name: id.to_string(),
            group: String::new(),
            host: "example.test".to_string(),
            port: 22,
            username: "alice".to_string(),
            auth_id: None,
            proxy_id: None,
            jumphost_id: None,
            port_forwards: vec![],
            keep_alive: 0,
            auto_exec_commands: vec![],
            snippets: vec![],
            ai_models: vec![],
            sftp_custom_commands: vec![],
            sftp_favorite_paths: vec![],
            additional_prompt: None,
            synced: true,
            created_at: None,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn apply_server_connection_moves_recent_truncates_and_increments_count() {
        let mut config = Config::empty();
        config.general.max_recent_servers = 3;
        config.servers = (0..25)
            .map(|index| sample_server(&format!("srv-{index:02}")))
            .collect();
        config.general.recent_server_ids = (0..25).map(|index| format!("srv-{index:02}")).collect();
        config
            .general
            .server_connection_counts
            .insert("srv-10".to_string(), 4);

        apply_server_connection(&mut config, "srv-10").unwrap();

        assert_eq!(config.general.recent_server_ids.len(), 20);
        assert_eq!(config.general.recent_server_ids.first().unwrap(), "srv-10");
        assert_eq!(
            config
                .general
                .recent_server_ids
                .iter()
                .filter(|id| id.as_str() == "srv-10")
                .count(),
            1
        );
        assert_eq!(config.general.server_connection_counts["srv-10"], 5);
    }

    #[test]
    fn apply_server_connection_rejects_unknown_server() {
        let mut config = Config::empty();

        assert!(apply_server_connection(&mut config, "missing").is_err());
        assert!(config.general.recent_server_ids.is_empty());
        assert!(config.general.server_connection_counts.is_empty());
    }

    #[test]
    fn register_replaces_and_cancels_previous_run() {
        let registry = AiRunRegistry::new();
        let token_a = registry.register("sess-1", "req-a");
        let token_b = registry.register("sess-1", "req-b");

        assert!(token_a.is_cancelled());
        assert!(!token_b.is_cancelled());
        assert_eq!(
            registry.current_request_id("sess-1").as_deref(),
            Some("req-b")
        );
    }

    #[test]
    fn cancel_only_matches_current_request_id() {
        let registry = AiRunRegistry::new();
        let token = registry.register("sess-1", "req-b");

        assert!(!registry.cancel("sess-1", "req-a"));
        assert!(!token.is_cancelled());

        assert!(registry.cancel("sess-1", "req-b"));
        assert!(token.is_cancelled());
    }

    #[test]
    fn clear_if_matches_does_not_remove_newer_request() {
        let registry = AiRunRegistry::new();
        let _token_a = registry.register("sess-1", "req-a");
        let token_b = registry.register("sess-1", "req-b");

        registry.clear_if_matches("sess-1", "req-a");
        assert_eq!(
            registry.current_request_id("sess-1").as_deref(),
            Some("req-b")
        );
        assert!(!token_b.is_cancelled());

        registry.clear_if_matches("sess-1", "req-b");
        assert!(registry.current_request_id("sess-1").is_none());
    }
}
