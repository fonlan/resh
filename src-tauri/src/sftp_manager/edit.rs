use crate::commands::AppState;
use crate::sftp_manager::edit_revision::{
    compare_remote_metadata, metadata_matches, read_remote_revision, read_remote_snapshot,
    sha256_hex, RemoteFileRevision, RemoteRevisionComparison,
};
use crate::sftp_manager::{SftpManager, TransferProgress};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex as AsyncMutex, OwnedMutexGuard};
use tracing::{error, info, warn};
use uuid::Uuid;

const EDIT_UPLOAD_CHUNK_SIZE: usize = 32 * 1024;
const EDIT_UPLOAD_CHUNK_TIMEOUT_SECS: u64 = 30;
const WATCHER_SUPPRESS_WINDOW: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WatchedUploadStatus {
    Idle,
    Uploading,
    PausedConflict,
}

#[derive(Clone, Debug)]
struct PendingConflictState {
    conflict_id: String,
    reason: String,
    current_revision: RemoteFileRevision,
}

#[derive(Clone, Debug)]
struct WatchedFile {
    edit_id: String,
    session_id: String,
    remote_path: String,
    baseline_revision: RemoteFileRevision,
    upload_status: WatchedUploadStatus,
    /// Local saves while paused are coalesced into one pending local version.
    pending_local_changes: bool,
    conflict: Option<PendingConflictState>,
    /// Absolute wall-clock suppress window for internal local rewrites (adopt remote).
    suppress_until: Option<Instant>,
    /// Content-hash suppress so an internal rewrite does not re-upload itself.
    suppress_content_sha256: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpEditConflictEvent {
    pub edit_id: String,
    pub conflict_id: String,
    pub session_id: String,
    pub remote_path: String,
    pub local_path: String,
    pub reason: String,
    pub expected_revision: RemoteFileRevision,
    pub current_revision: RemoteFileRevision,
    pub pending_local_changes: bool,
    pub snapshot_error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum SftpResolveEditConflictOutcome {
    Resolved {
        edit_id: String,
        conflict_id: String,
        session_id: String,
        remote_path: String,
        local_path: String,
        action: String,
        revision: Option<RemoteFileRevision>,
    },
    Conflict {
        edit_id: String,
        conflict_id: String,
        session_id: String,
        remote_path: String,
        local_path: String,
        reason: String,
        expected_revision: RemoteFileRevision,
        current_revision: RemoteFileRevision,
        pending_local_changes: bool,
        snapshot_error: Option<String>,
    },
}

enum WatchedUploadResult {
    Saved(RemoteFileRevision),
    Conflict {
        reason: &'static str,
        current_revision: RemoteFileRevision,
        snapshot_error: Option<String>,
    },
}

pub struct SftpEditManager {
    watched_files: Arc<Mutex<HashMap<PathBuf, WatchedFile>>>,
    watched_directories: Arc<Mutex<HashMap<PathBuf, usize>>>,
    /// One lock per remote file prevents Resh's editor and local-editor watcher
    /// from racing each other between the metadata check and TRUNCATE write.
    remote_write_locks: Arc<Mutex<HashMap<(String, String), Arc<AsyncMutex<()>>>>>,
    // watcher must be kept alive
    watcher: Arc<Mutex<Option<RecommendedWatcher>>>,
    _tx: mpsc::Sender<PathBuf>,
    _app: AppHandle,
}

impl SftpEditManager {
    pub fn new(app: AppHandle) -> Self {
        let (tx, mut rx) = mpsc::channel::<PathBuf>(100);
        let watched_files = Arc::new(Mutex::new(HashMap::<PathBuf, WatchedFile>::new()));
        let watched_files_clone = watched_files.clone();
        let app_clone = app.clone();

        // Spawn background task to handle file changes with debouncing.
        tokio::spawn(async move {
            let mut pending_updates: HashMap<PathBuf, tokio::time::Instant> = HashMap::new();
            let mut check_interval = tokio::time::interval(Duration::from_millis(100));

            loop {
                tokio::select! {
                    Some(path) = rx.recv() => {
                        let decision = {
                            let mut files = watched_files_clone.lock().unwrap();
                            if let Some(file) = files.get_mut(&path) {
                                if Self::should_suppress_event(file, &path) {
                                    EventDecision::Suppress
                                } else if file.upload_status
                                    == WatchedUploadStatus::PausedConflict
                                {
                                    // Coalesce further local saves while conflicted.
                                    // Only emit once when transitioning to pending so the UI
                                    // can show "still has a local version" without spam.
                                    let already_pending = file.pending_local_changes;
                                    file.pending_local_changes = true;
                                    if already_pending {
                                        EventDecision::Ignore
                                    } else if let Some(conflict) = file.conflict.as_ref() {
                                        EventDecision::MarkPending {
                                            event: SftpEditConflictEvent {
                                                edit_id: file.edit_id.clone(),
                                                conflict_id: conflict.conflict_id.clone(),
                                                session_id: file.session_id.clone(),
                                                remote_path: file.remote_path.clone(),
                                                local_path: path
                                                    .to_string_lossy()
                                                    .to_string(),
                                                reason: conflict.reason.clone(),
                                                expected_revision: file
                                                    .baseline_revision
                                                    .clone(),
                                                current_revision: conflict
                                                    .current_revision
                                                    .clone(),
                                                pending_local_changes: true,
                                                snapshot_error: None,
                                            },
                                        }
                                    } else {
                                        EventDecision::Ignore
                                    }
                                } else {
                                    // Idle or already uploading: debounce another attempt so
                                    // the latest local bytes are considered after in-flight work.
                                    EventDecision::QueueUpload
                                }
                            } else {
                                EventDecision::Ignore
                            }
                        };

                        match decision {
                            EventDecision::QueueUpload => {
                                // Delay upload by 500ms to allow atomic writes to complete.
                                pending_updates.insert(
                                    path,
                                    tokio::time::Instant::now() + Duration::from_millis(500),
                                );
                            }
                            EventDecision::MarkPending { event } => {
                                let _ = app_clone.emit("sftp-edit-conflict", event);
                            }
                            EventDecision::Suppress | EventDecision::Ignore => {}
                        }
                    }
                    _ = check_interval.tick() => {
                        let now = tokio::time::Instant::now();
                        let mut ready_paths = Vec::new();

                        pending_updates.retain(|path, deadline| {
                            if now >= *deadline {
                                ready_paths.push(path.clone());
                                false
                            } else {
                                true
                            }
                        });

                        for path in ready_paths {
                            let watched_file = {
                                let files = watched_files_clone.lock().unwrap();
                                files.get(&path).cloned()
                            };

                            let Some(watched_file) = watched_file else {
                                continue;
                            };
                            if watched_file.upload_status == WatchedUploadStatus::PausedConflict {
                                continue;
                            }

                            let app_inner = app_clone.clone();
                            let path_inner = path.clone();
                            let watched_files_inner = watched_files_clone.clone();

                            tokio::spawn(async move {
                                let session_id = watched_file.session_id.clone();
                                let remote_path = watched_file.remote_path.clone();
                                info!(
                                    "Uploading modified file {:?} to remote {}",
                                    path_inner, remote_path
                                );

                                let Some(state) = app_inner.try_state::<Arc<AppState>>() else {
                                    error!(
                                        "Skipping SFTP edit auto-upload: application state is unavailable"
                                    );
                                    return;
                                };
                                let permit = match state
                                    .operation_coordinator
                                    .try_acquire(crate::updater::OperationCategory::SftpEditUpload)
                                    .await
                                {
                                    Ok(permit) => permit,
                                    Err(error_message) => {
                                        warn!(
                                            "Skipping SFTP edit auto-upload (restart barrier): {}",
                                            error_message
                                        );
                                        Self::emit_transfer_result(
                                            &app_inner,
                                            Uuid::new_v4().to_string(),
                                            &session_id,
                                            &path_inner,
                                            &remote_path,
                                            0,
                                            "failed",
                                            Some(error_message),
                                        );
                                        return;
                                    }
                                };

                                let can_start = {
                                    let mut files = watched_files_inner.lock().unwrap();
                                    if let Some(current) = files.get_mut(&path_inner) {
                                        if current.edit_id == watched_file.edit_id
                                            && current.upload_status
                                                != WatchedUploadStatus::PausedConflict
                                        {
                                            current.upload_status =
                                                WatchedUploadStatus::Uploading;
                                            true
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                };
                                if !can_start {
                                    permit.release().await;
                                    return;
                                }

                                let task_id = Uuid::new_v4().to_string();
                                let initial_total_bytes = tokio::fs::metadata(&path_inner)
                                    .await
                                    .map(|metadata| metadata.len())
                                    .unwrap_or(0);
                                Self::emit_transfer_result(
                                    &app_inner,
                                    task_id.clone(),
                                    &session_id,
                                    &path_inner,
                                    &remote_path,
                                    initial_total_bytes,
                                    "transferring",
                                    None,
                                );

                                let path_lock = state
                                    .sftp_edit_manager
                                    .acquire_remote_path_lock(&session_id, &remote_path)
                                    .await;
                                // A second filesystem event may have been queued while an
                                // earlier upload for this path was in flight. Re-read the
                                // shared state after acquiring the path lock so it compares
                                // with that earlier upload's newly established baseline,
                                // rather than the stale state captured by this task.
                                let current_watched_file = {
                                    let files = watched_files_inner.lock().unwrap();
                                    files.get(&path_inner).cloned()
                                };
                                let Some(current_watched_file) = current_watched_file else {
                                    drop(path_lock);
                                    permit.release().await;
                                    return;
                                };
                                if current_watched_file.upload_status
                                    == WatchedUploadStatus::PausedConflict
                                    || current_watched_file.session_id != session_id
                                    || current_watched_file.remote_path != remote_path
                                    || current_watched_file.edit_id != watched_file.edit_id
                                {
                                    drop(path_lock);
                                    Self::restore_idle_if_uploading(
                                        &watched_files_inner,
                                        &path_inner,
                                        &watched_file.edit_id,
                                    );
                                    permit.release().await;
                                    return;
                                }

                                if Self::should_suppress_event_snapshot(
                                    &current_watched_file,
                                    &path_inner,
                                )
                                .await
                                {
                                    drop(path_lock);
                                    Self::restore_idle_if_uploading(
                                        &watched_files_inner,
                                        &path_inner,
                                        &watched_file.edit_id,
                                    );
                                    permit.release().await;
                                    return;
                                }

                                let current_revision =
                                    match read_remote_revision(&session_id, &remote_path).await {
                                        Ok(revision) => revision,
                                        Err(error_message) => {
                                            Self::restore_idle_if_uploading(
                                                &watched_files_inner,
                                                &path_inner,
                                                &watched_file.edit_id,
                                            );
                                            permit.release().await;
                                            error!(
                                                "SFTP edit pre-save stat failed: {}",
                                                error_message
                                            );
                                            Self::emit_transfer_result(
                                                &app_inner,
                                                task_id,
                                                &session_id,
                                                &path_inner,
                                                &remote_path,
                                                initial_total_bytes,
                                                "failed",
                                                Some(error_message),
                                            );
                                            drop(path_lock);
                                            return;
                                        }
                                    };

                                let result = match compare_remote_metadata(
                                    &current_watched_file.baseline_revision,
                                    &current_revision,
                                ) {
                                    RemoteRevisionComparison::MetadataUnchanged => {
                                        Self::upload_modified_file(
                                            &session_id,
                                            &path_inner,
                                            &remote_path,
                                        )
                                        .await
                                        .map(WatchedUploadResult::Saved)
                                    }
                                    RemoteRevisionComparison::MetadataChanged => {
                                        // Only download a remote snapshot when the conflict UI
                                        // needs a hashed revision. Never open TRUNCATE here.
                                        match read_remote_snapshot(&session_id, &remote_path).await
                                        {
                                            Ok(snapshot) => Ok(WatchedUploadResult::Conflict {
                                                reason: if snapshot.revision.exists {
                                                    "metadataChanged"
                                                } else {
                                                    "deleted"
                                                },
                                                current_revision: snapshot.revision,
                                                snapshot_error: None,
                                            }),
                                            Err(error_message) => {
                                                Ok(WatchedUploadResult::Conflict {
                                                    reason: "metadataChanged",
                                                    current_revision,
                                                    snapshot_error: Some(error_message),
                                                })
                                            }
                                        }
                                    }
                                    RemoteRevisionComparison::Deleted => {
                                        Ok(WatchedUploadResult::Conflict {
                                            reason: "deleted",
                                            current_revision,
                                            snapshot_error: None,
                                        })
                                    }
                                };
                                drop(path_lock);

                                match result {
                                    Ok(WatchedUploadResult::Saved(revision)) => {
                                        {
                                            let mut files = watched_files_inner.lock().unwrap();
                                            if let Some(current) = files.get_mut(&path_inner) {
                                                if current.edit_id == watched_file.edit_id
                                                    && current.upload_status
                                                        != WatchedUploadStatus::PausedConflict
                                                {
                                                    current.baseline_revision = revision;
                                                    current.upload_status =
                                                        WatchedUploadStatus::Idle;
                                                    current.pending_local_changes = false;
                                                    current.conflict = None;
                                                    current.suppress_content_sha256 = None;
                                                    current.suppress_until = None;
                                                }
                                            }
                                        }
                                        Self::emit_transfer_result(
                                            &app_inner,
                                            task_id,
                                            &session_id,
                                            &path_inner,
                                            &remote_path,
                                            initial_total_bytes,
                                            "completed",
                                            None,
                                        );
                                    }
                                    Ok(WatchedUploadResult::Conflict {
                                        reason,
                                        current_revision,
                                        snapshot_error,
                                    }) => {
                                        let conflict_id = Uuid::new_v4().to_string();
                                        let event = {
                                            let mut files = watched_files_inner.lock().unwrap();
                                            if let Some(current) = files.get_mut(&path_inner) {
                                                if current.edit_id == watched_file.edit_id
                                                    && current.session_id == session_id
                                                    && current.remote_path == remote_path
                                                {
                                                    // Keep a single open conflict for this edit.
                                                    let conflict_id = current
                                                        .conflict
                                                        .as_ref()
                                                        .map(|c| c.conflict_id.clone())
                                                        .unwrap_or(conflict_id);
                                                    current.upload_status =
                                                        WatchedUploadStatus::PausedConflict;
                                                    // Local file that triggered this upload is the
                                                    // pending version; further saves stay coalesced.
                                                    current.pending_local_changes = true;
                                                    current.conflict = Some(PendingConflictState {
                                                        conflict_id: conflict_id.clone(),
                                                        reason: reason.to_string(),
                                                        current_revision: current_revision.clone(),
                                                    });
                                                    Some(SftpEditConflictEvent {
                                                        edit_id: current.edit_id.clone(),
                                                        conflict_id,
                                                        session_id: session_id.clone(),
                                                        remote_path: remote_path.clone(),
                                                        local_path: path_inner
                                                            .to_string_lossy()
                                                            .to_string(),
                                                        reason: reason.to_string(),
                                                        expected_revision: current
                                                            .baseline_revision
                                                            .clone(),
                                                        current_revision,
                                                        pending_local_changes: true,
                                                        snapshot_error,
                                                    })
                                                } else {
                                                    None
                                                }
                                            } else {
                                                None
                                            }
                                        };
                                        if let Some(event) = event {
                                            let _ = app_inner.emit("sftp-edit-conflict", event);
                                        }
                                        Self::emit_transfer_result(
                                            &app_inner,
                                            task_id,
                                            &session_id,
                                            &path_inner,
                                            &remote_path,
                                            initial_total_bytes,
                                            "failed",
                                            Some(
                                                "Remote file changed; automatic upload is paused"
                                                    .to_string(),
                                            ),
                                        );
                                    }
                                    Err(error_message) => {
                                        Self::restore_idle_if_uploading(
                                            &watched_files_inner,
                                            &path_inner,
                                            &watched_file.edit_id,
                                        );
                                        error!(
                                            "SFTP edit auto-upload failed: {}",
                                            error_message
                                        );
                                        Self::emit_transfer_result(
                                            &app_inner,
                                            task_id,
                                            &session_id,
                                            &path_inner,
                                            &remote_path,
                                            initial_total_bytes,
                                            "failed",
                                            Some(error_message),
                                        );
                                    }
                                }

                                // User decision wait happens after this release.
                                permit.release().await;
                            });
                        }
                    }
                }
            }
        });

        let tx_clone = tx.clone();
        let watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    for path in event.paths {
                        let _ = tx_clone.blocking_send(path);
                    }
                }
                Err(e) => error!("Watch error: {:?}", e),
            },
            Config::default().with_poll_interval(Duration::from_secs(1)),
        )
        .ok();

        if watcher.is_none() {
            error!("Failed to create file watcher");
        }

        SftpEditManager {
            watched_files,
            watched_directories: Arc::new(Mutex::new(HashMap::new())),
            remote_write_locks: Arc::new(Mutex::new(HashMap::new())),
            watcher: Arc::new(Mutex::new(watcher)),
            _tx: tx,
            _app: app,
        }
    }

    pub async fn acquire_remote_path_lock(
        &self,
        session_id: &str,
        remote_path: &str,
    ) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.remote_write_locks.lock().unwrap();
            locks
                .entry((session_id.to_string(), remote_path.to_string()))
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }

    fn emit_transfer_result(
        app: &AppHandle,
        task_id: String,
        session_id: &str,
        local_path: &Path,
        remote_path: &str,
        total_bytes: u64,
        status: &str,
        error: Option<String>,
    ) {
        let file_name = local_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let transferred_bytes = if status == "completed" {
            total_bytes
        } else {
            0
        };
        let _ = app.emit(
            "transfer-progress",
            TransferProgress {
                task_id,
                type_: "upload".to_string(),
                session_id: session_id.to_string(),
                file_name,
                source: local_path.to_string_lossy().to_string(),
                destination: remote_path.to_string(),
                total_bytes,
                transferred_bytes,
                speed: 0.0,
                eta: None,
                status: status.to_string(),
                error,
            },
        );
    }

    fn restore_idle_if_uploading(
        watched_files: &Arc<Mutex<HashMap<PathBuf, WatchedFile>>>,
        path: &Path,
        edit_id: &str,
    ) {
        let mut files = watched_files.lock().unwrap();
        if let Some(current) = files.get_mut(path) {
            if current.edit_id == edit_id && current.upload_status == WatchedUploadStatus::Uploading
            {
                current.upload_status = WatchedUploadStatus::Idle;
            }
        }
    }

    fn should_suppress_event(file: &mut WatchedFile, path: &Path) -> bool {
        if let Some(until) = file.suppress_until {
            if Instant::now() < until {
                return true;
            }
            file.suppress_until = None;
        }

        if let Some(expected_hash) = file.suppress_content_sha256.clone() {
            if let Ok(bytes) = std::fs::read(path) {
                if sha256_hex(&bytes) == expected_hash {
                    file.suppress_content_sha256 = None;
                    return true;
                }
            }
            // Content no longer matches the internal rewrite; clear stale marker.
            file.suppress_content_sha256 = None;
        }

        false
    }

    async fn should_suppress_event_snapshot(file: &WatchedFile, path: &Path) -> bool {
        if let Some(until) = file.suppress_until {
            if Instant::now() < until {
                return true;
            }
        }
        if let Some(expected_hash) = &file.suppress_content_sha256 {
            if let Ok(bytes) = tokio::fs::read(path).await {
                return sha256_hex(&bytes) == *expected_hash;
            }
        }
        false
    }

    pub fn watch_file(
        &self,
        local_path: PathBuf,
        session_id: String,
        remote_path: String,
        baseline_revision: RemoteFileRevision,
    ) -> Result<String, String> {
        let mut files = self.watched_files.lock().unwrap();

        // If already watching, refresh the baseline and resume automatic uploads.
        let is_new = !files.contains_key(&local_path);
        let edit_id = files
            .get(&local_path)
            .map(|file| file.edit_id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        files.insert(
            local_path.clone(),
            WatchedFile {
                edit_id: edit_id.clone(),
                session_id,
                remote_path,
                baseline_revision,
                upload_status: WatchedUploadStatus::Idle,
                pending_local_changes: false,
                conflict: None,
                suppress_until: None,
                suppress_content_sha256: None,
            },
        );

        if is_new {
            if let Some(parent) = local_path.parent() {
                let mut dirs = self.watched_directories.lock().unwrap();
                let count = dirs.entry(parent.to_path_buf()).or_insert(0);
                *count += 1;

                if *count == 1 {
                    let mut watcher_guard = self.watcher.lock().unwrap();
                    if let Some(watcher) = watcher_guard.as_mut() {
                        info!("Starting to watch directory: {:?}", parent);
                        if let Err(e) = watcher.watch(parent, RecursiveMode::NonRecursive) {
                            error!("Failed to watch directory {:?}: {}", parent, e);
                            *count -= 1;
                            if *count == 0 {
                                dirs.remove(parent);
                            }
                            files.remove(&local_path);
                            return Err(format!("Failed to watch directory: {}", e));
                        }
                    }
                }
            }
        }

        Ok(edit_id)
    }

    pub fn stop_watching(&self, local_path: &Path) {
        let mut files = self.watched_files.lock().unwrap();
        if files.remove(local_path).is_some() {
            if let Some(parent) = local_path.parent() {
                let mut dirs = self.watched_directories.lock().unwrap();
                if let Some(count) = dirs.get_mut(parent) {
                    if *count > 0 {
                        *count -= 1;
                        if *count == 0 {
                            dirs.remove(parent);
                            let mut watcher_guard = self.watcher.lock().unwrap();
                            if let Some(watcher) = watcher_guard.as_mut() {
                                info!("Stopping watch for directory: {:?}", parent);
                                let _ = watcher.unwatch(parent);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn stop_watching_remote(&self, session_id: &str, remote_path: &str) {
        let to_remove = {
            let files = self.watched_files.lock().unwrap();
            files
                .iter()
                .filter(|(_, watched)| {
                    watched.session_id == session_id && watched.remote_path == remote_path
                })
                .map(|(lp, _)| lp.clone())
                .collect::<Vec<_>>()
        };

        for lp in to_remove {
            info!(
                "Stopping existing watch for remote file {} in session {}",
                remote_path, session_id
            );
            self.stop_watching(&lp);
            // Clean up the old temp directory
            if let Some(parent) = lp.parent() {
                if parent.to_string_lossy().contains("resh_sftp") {
                    let _ = std::fs::remove_dir_all(parent);
                }
            }
        }
    }

    pub fn cleanup_session(&self, session_id: &str) {
        let to_remove = {
            let files = self.watched_files.lock().unwrap();
            files
                .iter()
                .filter(|(_, watched)| watched.session_id == session_id)
                .map(|(lp, _)| lp.clone())
                .collect::<Vec<_>>()
        };

        for path in to_remove {
            info!("Cleaning up edit session for file: {:?}", path);
            self.stop_watching(&path);

            if let Some(parent) = path.parent() {
                if parent.to_string_lossy().contains("resh_sftp") {
                    let _ = std::fs::remove_dir_all(parent);
                }
            }
        }
    }

    fn find_by_edit_id(&self, edit_id: &str) -> Option<(PathBuf, WatchedFile)> {
        let files = self.watched_files.lock().unwrap();
        files
            .iter()
            .find(|(_, file)| file.edit_id == edit_id)
            .map(|(path, file)| (path.clone(), file.clone()))
    }

    /// Resolve a paused external-editor auto-upload conflict.
    ///
    /// Permit covers only the actual check/upload/adopt work; waiting for the
    /// user decision happens outside this method.
    pub async fn resolve_edit_conflict(
        &self,
        app: &AppHandle,
        state: &AppState,
        edit_id: String,
        conflict_id: String,
        action: String,
    ) -> Result<SftpResolveEditConflictOutcome, String> {
        let (local_path, watched) = self
            .find_by_edit_id(&edit_id)
            .ok_or_else(|| "External edit session not found".to_string())?;

        let conflict = watched
            .conflict
            .clone()
            .ok_or_else(|| "No pending external edit conflict for this file".to_string())?;
        if conflict.conflict_id != conflict_id {
            return Err("Conflict id is stale; refresh the conflict dialog".to_string());
        }

        match action.as_str() {
            "keepPaused" => Ok(SftpResolveEditConflictOutcome::Resolved {
                edit_id,
                conflict_id,
                session_id: watched.session_id,
                remote_path: watched.remote_path,
                local_path: local_path.to_string_lossy().to_string(),
                action,
                revision: None,
            }),
            "adoptRemote" => {
                self.resolve_adopt_remote(app, state, local_path, watched, conflict)
                    .await
            }
            "overwriteRemote" => {
                self.resolve_overwrite_remote(app, state, local_path, watched, conflict, false)
                    .await
            }
            "recreateRemote" => {
                self.resolve_overwrite_remote(app, state, local_path, watched, conflict, true)
                    .await
            }
            other => Err(format!(
                "Unsupported external edit conflict action: {}",
                other
            )),
        }
    }

    async fn resolve_adopt_remote(
        &self,
        app: &AppHandle,
        state: &AppState,
        local_path: PathBuf,
        watched: WatchedFile,
        conflict: PendingConflictState,
    ) -> Result<SftpResolveEditConflictOutcome, String> {
        if conflict.reason == "deleted" || !conflict.current_revision.exists {
            return Err(
                "Remote file was deleted; choose recreate remote or keep the local file paused"
                    .to_string(),
            );
        }

        let permit = state
            .operation_coordinator
            .try_acquire(crate::updater::OperationCategory::SftpEditUpload)
            .await?;
        let outcome: Result<SftpResolveEditConflictOutcome, String> = async {
            let path_lock = self
                .acquire_remote_path_lock(&watched.session_id, &watched.remote_path)
                .await;

            let snapshot = read_remote_snapshot(&watched.session_id, &watched.remote_path).await?;
            if !snapshot.revision.exists {
                let refreshed = self.pause_with_conflict(
                    &local_path,
                    &watched.edit_id,
                    &conflict.conflict_id,
                    "deleted",
                    snapshot.revision.clone(),
                    None,
                );
                drop(path_lock);
                return Ok(self.conflict_outcome_from_event(refreshed));
            }

            // Re-stat gate: if remote moved again since the dialog opened, refresh.
            if !metadata_matches(&conflict.current_revision, &snapshot.revision) {
                let refreshed = self.pause_with_conflict(
                    &local_path,
                    &watched.edit_id,
                    &conflict.conflict_id,
                    "metadataChanged",
                    snapshot.revision.clone(),
                    None,
                );
                drop(path_lock);
                let event = self.conflict_event_from_state(&local_path, &watched.edit_id);
                if let Some(event) = event {
                    let _ = app.emit("sftp-edit-conflict", event.clone());
                    return Ok(self.conflict_outcome_from_event(event));
                }
                return Ok(self.conflict_outcome_from_event(refreshed));
            }

            let content_hash = sha256_hex(&snapshot.bytes);
            {
                let mut files = self.watched_files.lock().unwrap();
                if let Some(current) = files.get_mut(&local_path) {
                    if current.edit_id != watched.edit_id {
                        return Err("External edit session changed during adopt".to_string());
                    }
                    current.suppress_content_sha256 = Some(content_hash.clone());
                    current.suppress_until = Some(Instant::now() + WATCHER_SUPPRESS_WINDOW);
                } else {
                    return Err("External edit session not found".to_string());
                }
            }

            write_local_file_atomically(&local_path, &snapshot.bytes).await?;

            {
                let mut files = self.watched_files.lock().unwrap();
                if let Some(current) = files.get_mut(&local_path) {
                    if current.edit_id == watched.edit_id {
                        current.baseline_revision = snapshot.revision.clone();
                        current.upload_status = WatchedUploadStatus::Idle;
                        current.pending_local_changes = false;
                        current.conflict = None;
                        // Keep suppress markers so the write-back event is ignored.
                        current.suppress_content_sha256 = Some(content_hash);
                        current.suppress_until = Some(Instant::now() + WATCHER_SUPPRESS_WINDOW);
                    }
                }
            }

            drop(path_lock);
            Ok(SftpResolveEditConflictOutcome::Resolved {
                edit_id: watched.edit_id,
                conflict_id: conflict.conflict_id,
                session_id: watched.session_id,
                remote_path: watched.remote_path,
                local_path: local_path.to_string_lossy().to_string(),
                action: "adoptRemote".to_string(),
                revision: Some(snapshot.revision),
            })
        }
        .await;
        permit.release().await;
        outcome
    }

    async fn resolve_overwrite_remote(
        &self,
        app: &AppHandle,
        state: &AppState,
        local_path: PathBuf,
        watched: WatchedFile,
        conflict: PendingConflictState,
        recreate: bool,
    ) -> Result<SftpResolveEditConflictOutcome, String> {
        if recreate {
            if conflict.reason != "deleted" && conflict.current_revision.exists {
                return Err(
                    "Remote file still exists; use overwrite remote instead of recreate"
                        .to_string(),
                );
            }
        } else if conflict.reason == "deleted" || !conflict.current_revision.exists {
            return Err(
                "Remote file was deleted; choose recreate remote to upload a new file".to_string(),
            );
        }

        let permit = state
            .operation_coordinator
            .try_acquire(crate::updater::OperationCategory::SftpEditUpload)
            .await?;
        let outcome: Result<SftpResolveEditConflictOutcome, String> = async {
            let path_lock = self
                .acquire_remote_path_lock(&watched.session_id, &watched.remote_path)
                .await;

            let current_revision =
                read_remote_revision(&watched.session_id, &watched.remote_path).await?;

            if recreate {
                if current_revision.exists {
                    // Remote reappeared; treat as a fresh conflict instead of overwriting.
                    let snapshot =
                        match read_remote_snapshot(&watched.session_id, &watched.remote_path).await
                        {
                            Ok(snapshot) => snapshot,
                            Err(error_message) => {
                                let event = self.pause_with_conflict(
                                    &local_path,
                                    &watched.edit_id,
                                    &conflict.conflict_id,
                                    "metadataChanged",
                                    current_revision,
                                    Some(error_message),
                                );
                                drop(path_lock);
                                let _ = app.emit("sftp-edit-conflict", event.clone());
                                return Ok(self.conflict_outcome_from_event(event));
                            }
                        };
                    let event = self.pause_with_conflict(
                        &local_path,
                        &watched.edit_id,
                        &conflict.conflict_id,
                        "metadataChanged",
                        snapshot.revision,
                        None,
                    );
                    drop(path_lock);
                    let _ = app.emit("sftp-edit-conflict", event.clone());
                    return Ok(self.conflict_outcome_from_event(event));
                }
            } else if !metadata_matches(&conflict.current_revision, &current_revision) {
                let (reason, revision, snapshot_error) = if !current_revision.exists {
                    ("deleted", current_revision, None)
                } else {
                    match read_remote_snapshot(&watched.session_id, &watched.remote_path).await {
                        Ok(snapshot) => (
                            if snapshot.revision.exists {
                                "metadataChanged"
                            } else {
                                "deleted"
                            },
                            snapshot.revision,
                            None,
                        ),
                        Err(error_message) => {
                            ("metadataChanged", current_revision, Some(error_message))
                        }
                    }
                };
                let event = self.pause_with_conflict(
                    &local_path,
                    &watched.edit_id,
                    &conflict.conflict_id,
                    reason,
                    revision,
                    snapshot_error,
                );
                drop(path_lock);
                let _ = app.emit("sftp-edit-conflict", event.clone());
                return Ok(self.conflict_outcome_from_event(event));
            }

            let revision =
                Self::upload_modified_file(&watched.session_id, &local_path, &watched.remote_path)
                    .await?;

            {
                let mut files = self.watched_files.lock().unwrap();
                if let Some(current) = files.get_mut(&local_path) {
                    if current.edit_id == watched.edit_id {
                        current.baseline_revision = revision.clone();
                        current.upload_status = WatchedUploadStatus::Idle;
                        current.pending_local_changes = false;
                        current.conflict = None;
                        current.suppress_content_sha256 = None;
                        current.suppress_until = None;
                    }
                }
            }

            drop(path_lock);

            let total_bytes = tokio::fs::metadata(&local_path)
                .await
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            Self::emit_transfer_result(
                app,
                Uuid::new_v4().to_string(),
                &watched.session_id,
                &local_path,
                &watched.remote_path,
                total_bytes,
                "completed",
                None,
            );

            Ok(SftpResolveEditConflictOutcome::Resolved {
                edit_id: watched.edit_id,
                conflict_id: conflict.conflict_id,
                session_id: watched.session_id,
                remote_path: watched.remote_path,
                local_path: local_path.to_string_lossy().to_string(),
                action: if recreate {
                    "recreateRemote".to_string()
                } else {
                    "overwriteRemote".to_string()
                },
                revision: Some(revision),
            })
        }
        .await;
        permit.release().await;
        outcome
    }

    fn pause_with_conflict(
        &self,
        local_path: &Path,
        edit_id: &str,
        conflict_id: &str,
        reason: &str,
        current_revision: RemoteFileRevision,
        snapshot_error: Option<String>,
    ) -> SftpEditConflictEvent {
        let mut files = self.watched_files.lock().unwrap();
        if let Some(current) = files.get_mut(local_path) {
            if current.edit_id == edit_id {
                current.upload_status = WatchedUploadStatus::PausedConflict;
                current.pending_local_changes = true;
                current.conflict = Some(PendingConflictState {
                    conflict_id: conflict_id.to_string(),
                    reason: reason.to_string(),
                    current_revision: current_revision.clone(),
                });
                return SftpEditConflictEvent {
                    edit_id: current.edit_id.clone(),
                    conflict_id: conflict_id.to_string(),
                    session_id: current.session_id.clone(),
                    remote_path: current.remote_path.clone(),
                    local_path: local_path.to_string_lossy().to_string(),
                    reason: reason.to_string(),
                    expected_revision: current.baseline_revision.clone(),
                    current_revision,
                    pending_local_changes: current.pending_local_changes,
                    snapshot_error,
                };
            }
        }

        SftpEditConflictEvent {
            edit_id: edit_id.to_string(),
            conflict_id: conflict_id.to_string(),
            session_id: String::new(),
            remote_path: String::new(),
            local_path: local_path.to_string_lossy().to_string(),
            reason: reason.to_string(),
            expected_revision: RemoteFileRevision::missing(),
            current_revision,
            pending_local_changes: true,
            snapshot_error,
        }
    }

    fn conflict_event_from_state(
        &self,
        local_path: &Path,
        edit_id: &str,
    ) -> Option<SftpEditConflictEvent> {
        let files = self.watched_files.lock().unwrap();
        let current = files.get(local_path)?;
        if current.edit_id != edit_id {
            return None;
        }
        let conflict = current.conflict.as_ref()?;
        Some(SftpEditConflictEvent {
            edit_id: current.edit_id.clone(),
            conflict_id: conflict.conflict_id.clone(),
            session_id: current.session_id.clone(),
            remote_path: current.remote_path.clone(),
            local_path: local_path.to_string_lossy().to_string(),
            reason: conflict.reason.clone(),
            expected_revision: current.baseline_revision.clone(),
            current_revision: conflict.current_revision.clone(),
            pending_local_changes: current.pending_local_changes,
            snapshot_error: None,
        })
    }

    fn conflict_outcome_from_event(
        &self,
        event: SftpEditConflictEvent,
    ) -> SftpResolveEditConflictOutcome {
        SftpResolveEditConflictOutcome::Conflict {
            edit_id: event.edit_id,
            conflict_id: event.conflict_id,
            session_id: event.session_id,
            remote_path: event.remote_path,
            local_path: event.local_path,
            reason: event.reason,
            expected_revision: event.expected_revision,
            current_revision: event.current_revision,
            pending_local_changes: event.pending_local_changes,
            snapshot_error: event.snapshot_error,
        }
    }

    async fn upload_modified_file(
        session_id: &str,
        local_path: &Path,
        remote_path: &str,
    ) -> Result<RemoteFileRevision, String> {
        let mut local_file = tokio::fs::File::open(local_path)
            .await
            .map_err(|error| error.to_string())?;
        let total_bytes = local_file
            .metadata()
            .await
            .map_err(|error| error.to_string())?
            .len();
        let sftp = SftpManager::get_session(session_id).await?;
        let handle = sftp
            .open(
                remote_path,
                russh_sftp::protocol::OpenFlags::CREATE
                    | russh_sftp::protocol::OpenFlags::TRUNCATE
                    | russh_sftp::protocol::OpenFlags::WRITE,
                russh_sftp::protocol::FileAttributes::default(),
            )
            .await
            .map_err(|error| error.to_string())?
            .handle;

        let upload_result: Result<String, String> = async {
            let mut offset = 0u64;
            let mut hasher = Sha256::new();

            while offset < total_bytes {
                let chunk_size =
                    std::cmp::min(EDIT_UPLOAD_CHUNK_SIZE as u64, total_bytes - offset) as usize;
                let mut chunk = vec![0u8; chunk_size];
                local_file
                    .read_exact(&mut chunk)
                    .await
                    .map_err(|error| error.to_string())?;
                hasher.update(&chunk);

                let write_result = tokio::time::timeout(
                    Duration::from_secs(EDIT_UPLOAD_CHUNK_TIMEOUT_SECS),
                    sftp.write(&handle, offset, chunk),
                )
                .await;

                match write_result {
                    Ok(result) => {
                        result.map_err(|error| error.to_string())?;
                    }
                    Err(_) => {
                        return Err(format!(
                            "Chunk upload timeout ({}s) at offset {}",
                            EDIT_UPLOAD_CHUNK_TIMEOUT_SECS, offset
                        ));
                    }
                }

                offset += chunk_size as u64;
            }

            Ok(format!("{:x}", hasher.finalize()))
        }
        .await;

        let _ = sftp.close(handle).await;
        let sha256 = upload_result?;
        let revision = read_remote_revision(session_id, remote_path).await?;
        if !revision.exists || revision.size != Some(total_bytes) {
            return Err(format!(
                "Remote save verification failed: expected {} bytes, got {:?}",
                total_bytes, revision.size
            ));
        }

        Ok(revision.with_sha256(sha256))
    }
}

enum EventDecision {
    QueueUpload,
    MarkPending { event: SftpEditConflictEvent },
    Suppress,
    Ignore,
}

async fn write_local_file_atomically(local_path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = local_path
        .parent()
        .ok_or("Invalid local path: missing parent directory")?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| e.to_string())?;

    let file_name = local_path
        .file_name()
        .ok_or("Invalid local path: missing file name")?
        .to_string_lossy()
        .to_string();
    let temp_path = parent.join(format!(".{}.{}.tmp", file_name, Uuid::new_v4()));

    let mut temp_file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| format!("Failed to create temporary file: {}", e))?;
    temp_file
        .write_all(bytes)
        .await
        .map_err(|e| format!("Failed to write temporary file: {}", e))?;
    temp_file
        .flush()
        .await
        .map_err(|e| format!("Failed to flush temporary file: {}", e))?;
    temp_file
        .sync_all()
        .await
        .map_err(|e| format!("Failed to sync temporary file: {}", e))?;
    drop(temp_file);

    tokio::fs::rename(&temp_path, local_path)
        .await
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            format!("Failed to replace local file: {}", e)
        })?;
    Ok(())
}
