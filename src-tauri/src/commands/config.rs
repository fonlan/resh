// src-tauri/src/commands/config.rs

use crate::ai::manager::AiManager;
use crate::app_paths::{resolve_app_data_dir_from_default, APP_DATA_DIR_NAME};
use crate::config::{Config, ConfigManager, SyncManager};
use crate::db::DatabaseManager;
use crate::sftp_manager::edit::SftpEditManager;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
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

#[derive(Debug, Default)]
struct AiRunSlot {
    latest_generation: u64,
    active: Option<AiRunEntry>,
}

/// Minimal registry surface shared by AppState and unit tests.
#[derive(Default)]
pub struct AiRunRegistry {
    slots: DashMap<String, AiRunSlot>,
    next_generation: AtomicU64,
}

impl AiRunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserves a stable start order for an AI request without changing the active token.
    /// The reservation is only committed when its run successfully attaches a token.
    pub fn reserve_generation(&self) -> u64 {
        self.next_generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Register a new AI run. Cancels any previous run for the same session first.
    pub fn register(&self, session_id: &str, request_id: &str) -> CancellationToken {
        let generation = self.reserve_generation();
        if let Some(token) = self.register_if_not_superseded(session_id, request_id, generation) {
            return token;
        }

        // A later request attached first while this caller was waiting for the
        // per-session lock. Preserve that newer owner and make this stale start
        // follow the ordinary cancellation path.
        let token = CancellationToken::new();
        token.cancel();
        token
    }

    /// Attach a delayed run only when no later same-session run has attached.
    /// This prevents an approval that completed persistence work late from replacing
    /// or cancelling a newer user request.
    pub fn register_if_not_superseded(
        &self,
        session_id: &str,
        request_id: &str,
        generation: u64,
    ) -> Option<CancellationToken> {
        let token = CancellationToken::new();
        let mut slot = self.slots.entry(session_id.to_string()).or_default();
        if slot.latest_generation > generation {
            return None;
        }
        slot.latest_generation = generation;

        let previous = slot.active.replace(AiRunEntry {
            request_id: request_id.to_string(),
            token: token.clone(),
        });
        if let Some(previous) = previous {
            // Swap before cancellation so the registry always points at the new token.
            previous.token.cancel();
        }
        Some(token)
    }

    /// Cancel only when the session's current run matches `request_id`.
    pub fn cancel(&self, session_id: &str, request_id: &str) -> bool {
        let Some(slot) = self.slots.get(session_id) else {
            return false;
        };
        let Some(entry) = slot.active.as_ref() else {
            return false;
        };
        if entry.request_id != request_id {
            return false;
        }
        entry.token.cancel();
        true
    }

    /// Clear the active registry entry only if it still belongs to this request.
    /// The generation watermark remains so an older delayed operation cannot revive.
    pub fn clear_if_matches(&self, session_id: &str, request_id: &str) {
        if let Some(mut slot) = self.slots.get_mut(session_id) {
            if slot
                .active
                .as_ref()
                .is_some_and(|entry| entry.request_id == request_id)
            {
                slot.active = None;
            }
        }
    }

    pub fn current_request_id(&self, session_id: &str) -> Option<String> {
        self.slots
            .get(session_id)
            .and_then(|slot| slot.active.as_ref().map(|entry| entry.request_id.clone()))
    }
}

/// Opaque resolution attempt retained only for the current app process. The token itself is
/// cryptographically bound to remote ETag and conflict hashes; this record adds the local config
/// generation guard so a save invalidates any open conflict dialog.
#[derive(Clone, Debug)]
pub struct PendingSyncConflictAttempt {
    pub token: String,
    pub config_generation: u64,
}

pub struct AppState {
    pub config_manager: ConfigManager,
    pub db_manager: DatabaseManager,
    pub config: Mutex<Config>,
    /// Serializes local config mutations with the full locally-originated WebDAV sync lifecycle.
    /// A save waits for an in-flight GET→merge→conditional-PUT so it cannot invalidate a conflict
    /// attempt after its final generation check but before the remote write.
    pub config_sync_gate: Mutex<()>,
    pub config_sync_generation: AtomicU64,
    /// The only resolution attempt currently eligible to commit. It is cleared by any local
    /// config generation change and revalidated against a freshly downloaded remote document.
    pub pending_sync_conflict_attempt: Mutex<Option<PendingSyncConflictAttempt>>,
    /// session_id -> current AI run (request_id + token). At most one run per session.
    pub ai_cancellation_tokens: AiRunRegistry,
    pub ai_manager: AiManager,
    pub sftp_edit_manager: SftpEditManager,
    /// Tracks config/sync/SFTP write work for safe update restart draining.
    pub operation_coordinator: std::sync::Arc<crate::updater::OperationCoordinator>,
}

impl AppState {
    pub fn next_config_sync_generation(&self) -> u64 {
        self.config_sync_generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn is_current_config_sync_generation(&self, generation: u64) -> bool {
        self.config_sync_generation.load(Ordering::Acquire) == generation
    }

    pub fn reserve_ai_run_generation(&self) -> u64 {
        self.ai_cancellation_tokens.reserve_generation()
    }

    pub fn register_ai_run(&self, session_id: &str, request_id: &str) -> CancellationToken {
        self.ai_cancellation_tokens.register(session_id, request_id)
    }

    pub fn register_ai_run_if_not_superseded(
        &self,
        session_id: &str,
        request_id: &str,
        generation: u64,
    ) -> Option<CancellationToken> {
        self.ai_cancellation_tokens
            .register_if_not_superseded(session_id, request_id, generation)
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

/// Structured result for manual and startup WebDAV sync. Expected remote conflicts and concurrent
/// changes remain successful IPC calls so callers can inspect their safe, actionable outcome.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSyncResult {
    pub config: Option<Config>,
    pub outcome: crate::config::sync_protocol::SyncOutcome,
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
    use crate::updater::OperationCategory;

    // Hold configWrite for the entire local save so restart draining sees it.
    let write_permit = state
        .operation_coordinator
        .try_acquire(OperationCategory::ConfigWrite)
        .await?;

    if config.normalize_legacy_defaults() {
        tracing::info!(
            transfer_profile = %config.general.sftp.transfer_profile,
            migrated_download_max_inflight = config.general.sftp.download_max_inflight,
            migrated_chunk_size_min = config.general.sftp.chunk_size_min,
            "normalized legacy SFTP throughput defaults before saving config"
        );
    }

    // Serialize a local mutation with the full remote sync lifecycle. In particular, conflict
    // resolution verifies a generation before its conditional PUT; allowing a save to advance the
    // generation while that request is in flight could still commit stale choices remotely.
    // Holding this gate only covers the local write here, while sync paths hold it through their
    // GET→merge→conditional-PUT sequence.
    let _sync_gate = state.config_sync_gate.lock().await;

    let mut current_config = state.config.lock().await;

    *current_config = config.clone();

    let local_path = state.config_manager.local_config_path();
    state.config_manager.save_config(&config, &local_path)?;
    let sync_generation = state.next_config_sync_generation();
    // Do not hold the shared config mutex while acquiring a WebDAV permit or waiting on network.
    // The generation captured above protects the later background projection instead.
    drop(current_config);
    *state.pending_sync_conflict_attempt.lock().await = None;
    drop(_sync_gate);
    tracing::debug!("Local config saved to {:?}", local_path);

    // Update log level
    crate::logger::set_log_level(config.general.debug_enabled);

    // If sync is enabled, trigger async sync in background without blocking
    // Local save is already complete, sync failures should not block the operation
    // Acquire webdavSync permit BEFORE spawn so restart never sees an idle gap.
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
        )
        .with_state_store(state.config_manager.app_data_dir().to_path_buf());

        let app_state = state.inner().clone();
        let sync_config = config.clone();
        // A background sync must never overwrite an edit saved while its network request was in
        // flight. The byte snapshot is intentionally exact: any persisted local change wins and
        // its own save schedules the next sync.
        let sync_snapshot = serde_json::to_vec(&config)
            .map_err(|e| format!("Failed to snapshot config for sync: {}", e))?;
        let sync_path = local_path.clone();

        let window = window.clone();
        let sync_permit = state
            .operation_coordinator
            .try_acquire(OperationCategory::WebdavSync)
            .await?;
        tokio::spawn(async move {
            // Serialize app-originated syncs. If a newer save was queued first, this stale
            // snapshot must not even issue its PUT; the newer generation owns the remote write.
            let _sync_gate = app_state.config_sync_gate.lock().await;
            if !app_state.is_current_config_sync_generation(sync_generation) {
                tracing::info!(
                    "Skipped stale background sync before remote write; a newer config save is queued"
                );
                sync_permit.release().await;
                return;
            }

            let mut local_copy = sync_config;
            match sync_manager.sync(&mut local_copy, vec![]).await {
                Ok(outcome) => match outcome {
                    crate::config::SyncOutcome::Applied { .. } => {
                        let mut in_memory = app_state.config.lock().await;
                        if app_state.is_current_config_sync_generation(sync_generation)
                            && config_matches_snapshot(&in_memory, &sync_snapshot)
                        {
                            if let Err(e) = app_state
                                .config_manager
                                .save_config(&local_copy, &sync_path)
                            {
                                tracing::error!("Failed to save merged config after sync: {}", e);
                            } else {
                                *in_memory = local_copy.clone();
                                let _ = window.emit("config-updated", local_copy);
                            }
                        } else {
                            tracing::info!(
                                "Discarded stale background sync projection; a newer local config is already saved"
                            );
                        }
                    }
                    crate::config::SyncOutcome::Conflicts {
                        conflicts,
                        attempt_token,
                    } => {
                        // A background conflict is only actionable for the exact local snapshot
                        // that produced it. Do not replace a newer pending attempt or publish an
                        // inevitably stale token after a local save.
                        let is_current_attempt =
                            app_state.is_current_config_sync_generation(sync_generation) && {
                                let in_memory = app_state.config.lock().await;
                                config_matches_snapshot(&in_memory, &sync_snapshot)
                            };
                        if is_current_attempt {
                            tracing::info!(
                                "Background sync has {} conflict(s); waiting for user resolution",
                                conflicts.len()
                            );
                            *app_state.pending_sync_conflict_attempt.lock().await =
                                Some(PendingSyncConflictAttempt {
                                    token: attempt_token.clone(),
                                    config_generation: sync_generation,
                                });
                            let _ = window.emit(
                                "sync-conflicts",
                                crate::config::SyncOutcome::Conflicts {
                                    conflicts,
                                    attempt_token,
                                },
                            );
                        } else {
                            tracing::info!(
                                "Discarded stale background sync conflicts; a newer local config is already saved"
                            );
                        }
                    }
                    crate::config::SyncOutcome::ConcurrentRemoteChange { message } => {
                        tracing::warn!("Background sync concurrent remote change: {}", message);
                        let _ = window.emit("sync-failed", message);
                    }
                    crate::config::SyncOutcome::Failed { error } => {
                        tracing::warn!("Background sync failed: {}", error.message);
                        let _ = window.emit("sync-failed", error.message);
                    }
                },
                Err(e) => {
                    tracing::warn!("Background sync failed: {}", e);
                    let _ = window.emit("sync-failed", e);
                }
            }
            sync_permit.release().await;
        });
    }

    write_permit.release().await;
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
    use crate::updater::OperationCategory;

    // Local-only write; still blocked during restart draining.
    let write_permit = state
        .operation_coordinator
        .try_acquire(OperationCategory::ConfigWrite)
        .await?;
    let mut config = state.config.lock().await;
    apply_server_connection(&mut config, &server_id)?;
    state.config_manager.save_local_config(&config)?;
    let result = config.clone();
    write_permit.release().await;
    Ok(result)
}

#[tauri::command]
pub async fn trigger_sync(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<TriggerSyncResult, String> {
    use crate::config::sync_protocol::{SyncError, SyncErrorKind, SyncOutcome};
    use crate::updater::OperationCategory;

    // Full-lifecycle sync permit so restart wait covers the whole call.
    let sync_permit = state
        .operation_coordinator
        .try_acquire(OperationCategory::WebdavSync)
        .await?;

    let result = async {
        // Manual and background syncs share one GET→merge→conditional-PUT critical section. Bump
        // the generation so any older background snapshot queued behind this operation is stale.
        let _sync_gate = state.config_sync_gate.lock().await;
        let sync_generation = state.next_config_sync_generation();
        *state.pending_sync_conflict_attempt.lock().await = None;

        // Snapshot under the mutex, but do not keep the mutex while the network operation runs.
        // A save that arrives meanwhile advances the generation and owns the eventual projection.
        let (sync_manager, mut sync_candidate, sync_snapshot, local_path) = {
            let config = state.config.lock().await;
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
            )
            .with_state_store(state.config_manager.app_data_dir().to_path_buf());
            let snapshot = serde_json::to_vec(&*config)
                .map_err(|error| format!("Failed to snapshot config for sync: {error}"))?;
            (
                sync_manager,
                config.clone(),
                snapshot,
                state.config_manager.local_config_path(),
            )
        };

        let outcome = sync_manager.sync(&mut sync_candidate, vec![]).await?;
        let (config, outcome) = match outcome {
            outcome @ SyncOutcome::Applied { .. } => {
                if sync_candidate.normalize_legacy_defaults() {
                    tracing::info!(
                        transfer_profile = %sync_candidate.general.sftp.transfer_profile,
                        migrated_download_max_inflight = sync_candidate.general.sftp.download_max_inflight,
                        migrated_chunk_size_min = sync_candidate.general.sftp.chunk_size_min,
                        "normalized legacy SFTP throughput defaults after sync merge"
                    );
                }

                let mut in_memory = state.config.lock().await;
                if !state.is_current_config_sync_generation(sync_generation)
                    || !config_matches_snapshot(&in_memory, &sync_snapshot)
                {
                    (
                        None,
                        SyncOutcome::Failed {
                            error: SyncError {
                                kind: SyncErrorKind::ConcurrentLocalChange,
                                message: "Local configuration changed while synchronization was running; the newer save will be synchronized instead".into(),
                            },
                        },
                    )
                } else {
                    state.config_manager.save_config(&sync_candidate, &local_path)?;
                    *in_memory = sync_candidate.clone();
                    (Some(sync_candidate), outcome)
                }
            }
            outcome => (None, outcome),
        };

        if let SyncOutcome::Conflicts { attempt_token, .. } = &outcome {
            *state.pending_sync_conflict_attempt.lock().await =
                Some(PendingSyncConflictAttempt {
                    token: attempt_token.clone(),
                    config_generation: sync_generation,
                });
        }

        emit_sync_outcome(&app, &outcome);
        Ok(TriggerSyncResult { config, outcome })
    }
    .await;

    sync_permit.release().await;
    result
}

#[tauri::command]
pub async fn resolve_sync_conflicts(
    app: AppHandle,
    attempt_token: String,
    resolutions: Vec<crate::config::sync_protocol::SyncResolution>,
    state: State<'_, Arc<AppState>>,
) -> Result<TriggerSyncResult, String> {
    use crate::config::sync_protocol::{SyncError, SyncErrorKind, SyncOutcome};
    use crate::updater::OperationCategory;

    let sync_permit = state
        .operation_coordinator
        .try_acquire(OperationCategory::WebdavSync)
        .await?;

    let result = async {
        let _sync_gate = state.config_sync_gate.lock().await;
        let pending = state.pending_sync_conflict_attempt.lock().await.clone();
        let Some(pending) = pending else {
            return Ok(TriggerSyncResult {
                config: None,
                outcome: SyncOutcome::Failed {
                    error: SyncError {
                        kind: SyncErrorKind::ConcurrentLocalChange,
                        message: "This sync conflict attempt is no longer active; refresh synchronization before applying choices".into(),
                    },
                },
            });
        };

        if pending.token != attempt_token
            || !state.is_current_config_sync_generation(pending.config_generation)
        {
            *state.pending_sync_conflict_attempt.lock().await = None;
            return Ok(TriggerSyncResult {
                config: None,
                outcome: SyncOutcome::Failed {
                    error: SyncError {
                        kind: SyncErrorKind::ConcurrentLocalChange,
                        message: "Local configuration changed after the conflicts were displayed; refresh synchronization before applying choices".into(),
                    },
                },
            });
        }

        let (sync_manager, mut sync_candidate, sync_snapshot, local_path) = {
            let config = state.config.lock().await;
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
            )
            .with_state_store(state.config_manager.app_data_dir().to_path_buf());
            let snapshot = serde_json::to_vec(&*config)
                .map_err(|error| format!("Failed to snapshot config for sync resolution: {error}"))?;
            (
                sync_manager,
                config.clone(),
                snapshot,
                state.config_manager.local_config_path(),
            )
        };

        let outcome = sync_manager
            .resolve_conflicts(&mut sync_candidate, &resolutions, &attempt_token)
            .await?;
        let outcome = if matches!(outcome, SyncOutcome::Applied { .. }) {
            outcome
        } else {
            let current_matches_snapshot = {
                let in_memory = state.config.lock().await;
                config_matches_snapshot(&in_memory, &sync_snapshot)
            };
            if state.is_current_config_sync_generation(pending.config_generation)
                && current_matches_snapshot
            {
                outcome
            } else {
                SyncOutcome::Failed {
                    error: SyncError {
                        kind: SyncErrorKind::ConcurrentLocalChange,
                        message: "Local configuration changed while conflict resolutions were being checked; refresh synchronization before applying choices".into(),
                    },
                }
            }
        };
        let (config, outcome) = match outcome {
            outcome @ SyncOutcome::Applied { .. } => {
                let mut in_memory = state.config.lock().await;
                if !state.is_current_config_sync_generation(pending.config_generation)
                    || !config_matches_snapshot(&in_memory, &sync_snapshot)
                {
                    (
                        None,
                        SyncOutcome::Failed {
                            error: SyncError {
                                kind: SyncErrorKind::ConcurrentLocalChange,
                                message: "Local configuration changed while conflict resolutions were being applied; refresh synchronization before applying choices".into(),
                            },
                        },
                    )
                } else {
                    state.config_manager.save_config(&sync_candidate, &local_path)?;
                    *in_memory = sync_candidate.clone();
                    (Some(sync_candidate), outcome)
                }
            }
            outcome => (None, outcome),
        };

        match &outcome {
            SyncOutcome::Conflicts { attempt_token, .. } => {
                *state.pending_sync_conflict_attempt.lock().await =
                    Some(PendingSyncConflictAttempt {
                        token: attempt_token.clone(),
                        config_generation: pending.config_generation,
                    });
            }
            _ => *state.pending_sync_conflict_attempt.lock().await = None,
        }

        emit_sync_outcome(&app, &outcome);
        Ok(TriggerSyncResult { config, outcome })
    }
    .await;

    sync_permit.release().await;
    result
}

fn emit_sync_outcome(app: &AppHandle, outcome: &crate::config::sync_protocol::SyncOutcome) {
    match outcome {
        crate::config::sync_protocol::SyncOutcome::Applied { .. } => {}
        crate::config::sync_protocol::SyncOutcome::Conflicts { .. } => {
            let _ = app.emit("sync-conflicts", outcome);
        }
        crate::config::sync_protocol::SyncOutcome::ConcurrentRemoteChange { message } => {
            let _ = app.emit("sync-failed", message);
        }
        crate::config::sync_protocol::SyncOutcome::Failed { error } => {
            let _ = app.emit("sync-failed", &error.message);
        }
    }
}

fn config_matches_snapshot(config: &Config, snapshot: &[u8]) -> bool {
    serde_json::to_vec(config)
        .map(|current| current == snapshot)
        .unwrap_or(false)
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
    fn config_snapshot_detects_a_newer_local_save() {
        let original = Config::empty();
        let snapshot = serde_json::to_vec(&original).unwrap();
        assert!(config_matches_snapshot(&original, &snapshot));

        let mut newer = original;
        newer.general.theme = "light".to_string();
        assert!(!config_matches_snapshot(&newer, &snapshot));
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

    #[test]
    fn request_a_replaced_by_b_then_a_clear_and_cancel_leave_b() {
        // Full A→B race: A is replaced, then A finishes cleanup + stale cancel.
        let registry = AiRunRegistry::new();
        let token_a = registry.register("sess-race", "req-a");
        let token_b = registry.register("sess-race", "req-b");

        assert!(token_a.is_cancelled(), "register B must cancel A");
        assert!(!token_b.is_cancelled());
        assert_eq!(
            registry.current_request_id("sess-race").as_deref(),
            Some("req-b")
        );

        // Guard Drop for A must not remove B.
        registry.clear_if_matches("sess-race", "req-a");
        assert_eq!(
            registry.current_request_id("sess-race").as_deref(),
            Some("req-b")
        );
        assert!(!token_b.is_cancelled());

        // Stale cancel for A is rejected.
        assert!(!registry.cancel("sess-race", "req-a"));
        assert!(!token_b.is_cancelled());

        // Cancel B succeeds.
        assert!(registry.cancel("sess-race", "req-b"));
        assert!(token_b.is_cancelled());

        registry.clear_if_matches("sess-race", "req-b");
        assert!(registry.current_request_id("sess-race").is_none());
    }

    #[test]
    fn cancel_unknown_session_is_false() {
        let registry = AiRunRegistry::new();
        assert!(!registry.cancel("missing", "req-x"));
        assert!(registry.current_request_id("missing").is_none());
    }

    #[test]
    fn sessions_are_isolated() {
        let registry = AiRunRegistry::new();
        let token_s1 = registry.register("sess-1", "req-1");
        let token_s2 = registry.register("sess-2", "req-2");

        assert!(registry.cancel("sess-1", "req-1"));
        assert!(token_s1.is_cancelled());
        assert!(!token_s2.is_cancelled());
        assert_eq!(
            registry.current_request_id("sess-2").as_deref(),
            Some("req-2")
        );
    }

    #[test]
    fn concurrent_register_on_same_session_keeps_single_live_current() {
        use std::sync::Arc;
        use std::thread;

        let registry = Arc::new(AiRunRegistry::new());
        let session = "sess-concurrent";
        let threads = 16;
        let rounds = 50;

        let mut handles = Vec::with_capacity(threads);
        for t in 0..threads {
            let registry = Arc::clone(&registry);
            handles.push(thread::spawn(move || {
                let mut last_token = None;
                for r in 0..rounds {
                    let request_id = format!("req-t{t}-r{r}");
                    last_token = Some(registry.register(session, &request_id));
                }
                last_token
            }));
        }

        let mut last_tokens = Vec::with_capacity(threads);
        for handle in handles {
            last_tokens.push(handle.join().expect("register stress thread"));
        }

        let current = registry
            .current_request_id(session)
            .expect("one current request must remain after concurrent register");
        assert!(current.starts_with("req-t"));

        // Among each thread's final token: exactly the current id's token (if any)
        // may still be live; every other returned token must already be cancelled
        // because a later register on the same session replaced it.
        let mut live_count = 0usize;
        for token in last_tokens.into_iter().flatten() {
            if token.is_cancelled() {
                continue;
            }
            live_count += 1;
        }
        // At most one of the per-thread final tokens can still be current/live.
        assert!(
            live_count <= 1,
            "at most one live final token expected, got {live_count}"
        );

        assert!(registry.cancel(session, &current));
        registry.clear_if_matches(session, &current);
        assert!(registry.current_request_id(session).is_none());
    }

    #[test]
    fn delayed_registration_cannot_replace_a_newer_request() {
        let registry = AiRunRegistry::new();
        let delayed_generation = registry.reserve_generation();
        let newer = registry.register("sess-delayed", "req-newer");

        assert!(registry
            .register_if_not_superseded("sess-delayed", "req-delayed", delayed_generation)
            .is_none());
        assert!(!newer.is_cancelled());
        assert_eq!(
            registry.current_request_id("sess-delayed").as_deref(),
            Some("req-newer")
        );

        registry.clear_if_matches("sess-delayed", "req-newer");
        assert!(registry
            .register_if_not_superseded("sess-delayed", "req-delayed", delayed_generation)
            .is_none());
    }
}
