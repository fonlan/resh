// src-tauri/src/commands/updater.rs

use crate::commands::AppState;
use crate::updater::{
    ack_restart_session, cancel_update_download, check_for_update, download_update,
    get_pending_restart_session, write_snapshot, CheckForUpdateOptions, CheckUpdateResult,
    OperationSnapshot, PendingRestartSession, PreparedUpdate, RestartSessionSnapshot,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

/// Check GitHub Releases for a newer stable version.
///
/// `force` skips the automatic minimum interval (used by manual checks).
/// All callers share a single in-flight request to avoid duplicate API traffic.
#[tauri::command]
pub async fn check_for_update_cmd(
    state: State<'_, Arc<AppState>>,
    force: Option<bool>,
) -> Result<CheckUpdateResult, String> {
    let (proxies, proxy_id) = {
        let config = state.config.lock().await;
        let proxies = config.proxies.clone();
        let proxy_id = config.general.update.proxy_id.clone();
        (proxies, proxy_id)
    };

    let result = check_for_update(
        &proxies,
        proxy_id.as_deref(),
        CheckForUpdateOptions {
            force: force.unwrap_or(false),
            current_version: None,
            platform: None,
        },
    )
    .await;

    Ok(result)
}

/// Return the compile-time app version used for update comparison.
#[tauri::command]
pub async fn get_app_version_cmd() -> Result<String, String> {
    Ok(crate::updater::current_app_version().to_string())
}

/// Download and verify the install asset for a previously discovered update id.
///
/// Streams to a staging `.part` file, validates `SHA256SUMS.txt` (and optional
/// GitHub digests), then returns an immutable `PreparedUpdate` handle.
/// Only accepts opaque ids issued by `check_for_update_cmd`.
#[tauri::command]
pub async fn download_update_cmd(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    update_id: String,
) -> Result<PreparedUpdate, String> {
    let (proxies, proxy_id) = {
        let config = state.config.lock().await;
        let proxies = config.proxies.clone();
        let proxy_id = config.general.update.proxy_id.clone();
        (proxies, proxy_id)
    };

    download_update(&app, &proxies, proxy_id.as_deref(), &update_id).await
}

/// Cancel the in-progress update download, if any.
#[tauri::command]
pub async fn cancel_update_download_cmd() -> Result<(), String> {
    cancel_update_download().await
}

fn resolve_app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let default_app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;
    Ok(crate::app_paths::resolve_app_data_dir_from_default(
        &default_app_data_dir,
    ))
}

/// Snapshot of backend write operations (for restart UI).
#[tauri::command]
pub async fn get_operation_snapshot_cmd(
    state: State<'_, Arc<AppState>>,
) -> Result<OperationSnapshot, String> {
    Ok(state.operation_coordinator.snapshot().await)
}

/// Enter draining mode: refuse new config/sync/SFTP write permits.
/// Returns `{ snapshot, drainSession }` so cancel can target this drain only.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginDrainingResult {
    pub snapshot: OperationSnapshot,
    pub drain_session: u64,
}

#[tauri::command]
pub async fn begin_restart_draining_cmd(
    state: State<'_, Arc<AppState>>,
) -> Result<BeginDrainingResult, String> {
    let drain_session = state.operation_coordinator.begin_draining().await;
    Ok(BeginDrainingResult {
        snapshot: state.operation_coordinator.snapshot().await,
        drain_session,
    })
}

/// Cancel maintenance for the given drain session (stale sessions are no-ops).
/// Pass `null` for unconditional recovery cancel.
#[tauri::command]
pub async fn cancel_restart_draining_cmd(
    state: State<'_, Arc<AppState>>,
    drain_session: Option<u64>,
) -> Result<OperationSnapshot, String> {
    let _ = state
        .operation_coordinator
        .cancel_draining(drain_session)
        .await;
    Ok(state.operation_coordinator.snapshot().await)
}

/// Wait until coordinator is idle or timeout (no force-kill).
///
/// Returns `{ idle, snapshot }`. On timeout the client may cancel restart or keep waiting.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitUntilIdleResult {
    pub idle: bool,
    pub snapshot: OperationSnapshot,
}

#[tauri::command]
pub async fn wait_until_operations_idle_cmd(
    state: State<'_, Arc<AppState>>,
    timeout_ms: Option<u64>,
) -> Result<WaitUntilIdleResult, String> {
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(30_000).clamp(100, 600_000));
    let idle = state
        .operation_coordinator
        .wait_until_idle(timeout)
        .await?;
    let snapshot = state.operation_coordinator.snapshot().await;
    Ok(WaitUntilIdleResult { idle, snapshot })
}

/// Atomically write a restart session snapshot; returns the one-time token.
#[tauri::command]
pub async fn save_restart_session_snapshot_cmd(
    app: AppHandle,
    snapshot: RestartSessionSnapshot,
) -> Result<String, String> {
    let app_data_dir = resolve_app_data_dir(&app)?;
    write_snapshot(&app_data_dir, snapshot)
}

/// Load pending restore session from CLI token captured at process start.
#[tauri::command]
pub async fn get_pending_restart_session_cmd(
    app: AppHandle,
) -> Result<Option<PendingRestartSession>, String> {
    let app_data_dir = resolve_app_data_dir(&app)?;
    get_pending_restart_session(&app_data_dir)
}

/// Acknowledge successful restore and delete the snapshot.
#[tauri::command]
pub async fn ack_restart_session_cmd(app: AppHandle, token: String) -> Result<(), String> {
    let app_data_dir = resolve_app_data_dir(&app)?;
    ack_restart_session(&app_data_dir, &token)
}

/// Final gate before process exit: coordinator must be idle and snapshot must exist on disk.
#[tauri::command]
pub async fn verify_ready_for_restart_cmd(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    snapshot_token: String,
) -> Result<(), String> {
    if !state.operation_coordinator.is_idle().await {
        return Err("Operations are still in progress".to_string());
    }
    if !state.operation_coordinator.is_draining() {
        return Err("Restart draining is not active".to_string());
    }
    let app_data_dir = resolve_app_data_dir(&app)?;
    // Re-load to ensure fsync'd snapshot is readable (pre-install: do not require
    // target_version == current yet).
    let _ = crate::updater::load_snapshot_with_options(&app_data_dir, &snapshot_token, false)?;
    Ok(())
}
