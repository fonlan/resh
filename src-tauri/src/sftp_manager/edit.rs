use crate::commands::AppState;
use crate::sftp_manager::edit_revision::{
    compare_remote_metadata, read_remote_revision, RemoteFileRevision, RemoteRevisionComparison,
};
use crate::sftp_manager::{SftpManager, TransferProgress};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, Mutex as AsyncMutex, OwnedMutexGuard};
use tracing::{error, info, warn};
use uuid::Uuid;

const EDIT_UPLOAD_CHUNK_SIZE: usize = 32 * 1024;
const EDIT_UPLOAD_CHUNK_TIMEOUT_SECS: u64 = 30;

#[derive(Clone, Debug)]
struct WatchedFile {
    session_id: String,
    remote_path: String,
    baseline_revision: RemoteFileRevision,
    upload_paused_for_conflict: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SftpEditConflictEvent {
    session_id: String,
    remote_path: String,
    local_path: String,
    reason: String,
    expected_revision: RemoteFileRevision,
    current_revision: RemoteFileRevision,
}

enum WatchedUploadResult {
    Saved(RemoteFileRevision),
    Conflict {
        reason: &'static str,
        current_revision: RemoteFileRevision,
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
                        let is_uploadable = {
                            let files = watched_files_clone.lock().unwrap();
                            files
                                .get(&path)
                                .is_some_and(|file| !file.upload_paused_for_conflict)
                        };

                        if is_uploadable {
                            // Delay upload by 500ms to allow atomic writes to complete.
                            pending_updates.insert(path, tokio::time::Instant::now() + Duration::from_millis(500));
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
                            if watched_file.upload_paused_for_conflict {
                                continue;
                            }

                            let app_inner = app_clone.clone();
                            let path_inner = path.clone();
                            let watched_files_inner = watched_files_clone.clone();

                            tokio::spawn(async move {
                                let session_id = watched_file.session_id.clone();
                                let remote_path = watched_file.remote_path.clone();
                                info!("Uploading modified file {:?} to remote {}", path_inner, remote_path);

                                let Some(state) = app_inner.try_state::<Arc<AppState>>() else {
                                    error!("Skipping SFTP edit auto-upload: application state is unavailable");
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
                                if current_watched_file.upload_paused_for_conflict
                                    || current_watched_file.session_id != session_id
                                    || current_watched_file.remote_path != remote_path
                                {
                                    drop(path_lock);
                                    permit.release().await;
                                    return;
                                }

                                let current_revision = match read_remote_revision(&session_id, &remote_path).await {
                                    Ok(revision) => revision,
                                    Err(error_message) => {
                                        permit.release().await;
                                        error!("SFTP edit pre-save stat failed: {}", error_message);
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
                                    RemoteRevisionComparison::MetadataUnchanged => Self::upload_modified_file(
                                        &session_id,
                                        &path_inner,
                                        &remote_path,
                                    )
                                    .await
                                    .map(WatchedUploadResult::Saved),
                                    RemoteRevisionComparison::MetadataChanged => Ok(
                                        WatchedUploadResult::Conflict {
                                            reason: "metadataChanged",
                                            current_revision,
                                        },
                                    ),
                                    RemoteRevisionComparison::Deleted => Ok(WatchedUploadResult::Conflict {
                                        reason: "deleted",
                                        current_revision,
                                    }),
                                };
                                drop(path_lock);

                                match result {
                                    Ok(WatchedUploadResult::Saved(revision)) => {
                                        {
                                            let mut files = watched_files_inner.lock().unwrap();
                                            if let Some(current) = files.get_mut(&path_inner) {
                                                if current.session_id == session_id
                                                    && current.remote_path == remote_path
                                                    && !current.upload_paused_for_conflict
                                                {
                                                    current.baseline_revision = revision;
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
                                    }) => {
                                        {
                                            let mut files = watched_files_inner.lock().unwrap();
                                            if let Some(current) = files.get_mut(&path_inner) {
                                                if current.session_id == session_id
                                                    && current.remote_path == remote_path
                                                {
                                                    current.upload_paused_for_conflict = true;
                                                }
                                            }
                                        }
                                        let _ = app_inner.emit(
                                            "sftp-edit-conflict",
                                            SftpEditConflictEvent {
                                                session_id: session_id.clone(),
                                                remote_path: remote_path.clone(),
                                                local_path: path_inner.to_string_lossy().to_string(),
                                                reason: reason.to_string(),
                                                expected_revision: current_watched_file.baseline_revision,
                                                current_revision,
                                            },
                                        );
                                        Self::emit_transfer_result(
                                            &app_inner,
                                            task_id,
                                            &session_id,
                                            &path_inner,
                                            &remote_path,
                                            initial_total_bytes,
                                            "failed",
                                            Some("Remote file changed; automatic upload is paused".to_string()),
                                        );
                                    }
                                    Err(error_message) => {
                                        error!("SFTP edit auto-upload failed: {}", error_message);
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

    pub fn watch_file(
        &self,
        local_path: PathBuf,
        session_id: String,
        remote_path: String,
        baseline_revision: RemoteFileRevision,
    ) -> Result<(), String> {
        let mut files = self.watched_files.lock().unwrap();

        // If already watching, refresh the baseline and resume automatic uploads.
        let is_new = !files.contains_key(&local_path);
        files.insert(
            local_path.clone(),
            WatchedFile {
                session_id,
                remote_path,
                baseline_revision,
                upload_paused_for_conflict: false,
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

        Ok(())
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
