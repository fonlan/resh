use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tracing::{error, info};
use crate::sftp_manager::{SftpManager, TransferProgress};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

const EDIT_UPLOAD_CHUNK_SIZE: usize = 32 * 1024;
const EDIT_UPLOAD_CHUNK_TIMEOUT_SECS: u64 = 30;

pub struct SftpEditManager {
    watched_files: Arc<Mutex<HashMap<PathBuf, (String, String)>>>,
    watched_directories: Arc<Mutex<HashMap<PathBuf, usize>>>,
    // watcher must be kept alive
    watcher: Arc<Mutex<Option<RecommendedWatcher>>>,
    _tx: mpsc::Sender<PathBuf>,
    _app: AppHandle,
}

impl SftpEditManager {
    pub fn new(app: AppHandle) -> Self {
        let (tx, mut rx) = mpsc::channel::<PathBuf>(100);
        let watched_files = Arc::new(Mutex::new(HashMap::<PathBuf, (String, String)>::new()));
        let watched_files_clone = watched_files.clone();
        let app_clone = app.clone();

        // Spawn background task to handle file changes with debouncing
        tokio::spawn(async move {
            let mut pending_updates: HashMap<PathBuf, tokio::time::Instant> = HashMap::new();
            let mut check_interval = tokio::time::interval(Duration::from_millis(100));

            loop {
                tokio::select! {
                    Some(path) = rx.recv() => {
                        // Only debounce files we are actually watching
                        let is_watched = {
                            let files = watched_files_clone.lock().unwrap();
                            files.contains_key(&path)
                        };
                        
                        if is_watched {
                            // Delay upload by 500ms to allow atomic writes to complete
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
                            let info = {
                                let files = watched_files_clone.lock().unwrap();
                                files.get(&path).cloned()
                            };

                            if let Some((session_id, remote_path)) = info {
                                let app_inner = app_clone.clone();
                                let path_inner = path.clone();
                                
                                tokio::spawn(async move {
                                    info!("Uploading modified file {:?} to remote {}", path_inner, remote_path);

                                    let task_id = Uuid::new_v4().to_string();
                                    let file_name = path_inner.file_name().unwrap_or_default().to_string_lossy().to_string();
                                    let initial_total_bytes = tokio::fs::metadata(&path_inner).await.map(|m| m.len()).unwrap_or(0);

                                    // 1. Emit Initial Progress
                                    let _ = app_inner.emit("transfer-progress", TransferProgress {
                                        task_id: task_id.clone(),
                                        type_: "upload".to_string(),
                                        session_id: session_id.clone(),
                                        file_name: file_name.clone(),
                                        source: path_inner.to_string_lossy().to_string(),
                                        destination: remote_path.clone(),
                                        total_bytes: initial_total_bytes,
                                        transferred_bytes: 0,
                                        speed: 0.0,
                                        eta: None,
                                        status: "transferring".to_string(),
                                        error: None,
                                    });

                                    // Perform upload
                                    let result = Self::upload_modified_file(&session_id, &path_inner, &remote_path).await;

                                    // 2. Emit Final Progress
                                    let (final_status, final_error, total_bytes, transferred_bytes) = match result {
                                        Ok(bytes) => ("completed".to_string(), None, bytes, bytes),
                                        Err(e) => {
                                            error!("Upload failed: {}", e);
                                            ("failed".to_string(), Some(e), initial_total_bytes, 0)
                                        }
                                    };

                                    let _ = app_inner.emit("transfer-progress", TransferProgress {
                                        task_id,
                                        type_: "upload".to_string(),
                                        session_id,
                                        file_name,
                                        source: path_inner.to_string_lossy().to_string(),
                                        destination: remote_path,
                                        total_bytes,
                                        transferred_bytes,
                                        speed: 0.0,
                                        eta: None,
                                        status: final_status,
                                        error: final_error,
                                    });
                                });
                            }
                        }
                    }
                }
            }
        });

        let tx_clone = tx.clone();
        let watcher = RecommendedWatcher::new(move |res: Result<notify::Event, notify::Error>| {
            match res {
                Ok(event) => {
                    for path in event.paths {
                        let _ = tx_clone.blocking_send(path);
                    }
                },
                Err(e) => error!("Watch error: {:?}", e),
            }
        }, Config::default().with_poll_interval(Duration::from_secs(1))).ok();

        if watcher.is_none() {
            error!("Failed to create file watcher");
        }

        SftpEditManager {
            watched_files,
            watched_directories: Arc::new(Mutex::new(HashMap::new())),
            watcher: Arc::new(Mutex::new(watcher)),
            _tx: tx,
            _app: app,
        }
    }

    pub fn watch_file(&self, local_path: PathBuf, session_id: String, remote_path: String) -> Result<(), String> {
        let mut files = self.watched_files.lock().unwrap();
        
        // If already watching, just update info
        let is_new = !files.contains_key(&local_path);
        files.insert(local_path.clone(), (session_id, remote_path));

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
                            // Cleanup if failed
                            *count -= 1;
                            if *count == 0 { dirs.remove(parent); }
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
            files.iter()
                .filter(|(_, (sid, rp))| sid == session_id && rp == remote_path)
                .map(|(lp, _)| lp.clone())
                .collect::<Vec<_>>()
        };

        for lp in to_remove {
            info!("Stopping existing watch for remote file {} in session {}", remote_path, session_id);
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
            files.iter()
                .filter(|(_, (sid, _))| sid == session_id)
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

    async fn upload_modified_file(session_id: &str, local_path: &Path, remote_path: &str) -> Result<u64, String> {
        let mut local_file = tokio::fs::File::open(local_path).await.map_err(|e| e.to_string())?;
        let total_bytes = local_file.metadata().await.map_err(|e| e.to_string())?.len();
        let sftp = SftpManager::get_session(session_id).await?;
        let handle = sftp.open(
            remote_path,
            russh_sftp::protocol::OpenFlags::CREATE
                | russh_sftp::protocol::OpenFlags::TRUNCATE
                | russh_sftp::protocol::OpenFlags::WRITE,
            russh_sftp::protocol::FileAttributes::default(),
        ).await.map_err(|e| e.to_string())?.handle;

        let upload_result: Result<(), String> = async {
            let mut offset = 0u64;

            while offset < total_bytes {
                let chunk_size = std::cmp::min(EDIT_UPLOAD_CHUNK_SIZE as u64, total_bytes - offset) as usize;
                let mut chunk = vec![0u8; chunk_size];
                local_file.read_exact(&mut chunk).await.map_err(|e| e.to_string())?;

                let write_result = tokio::time::timeout(
                    Duration::from_secs(EDIT_UPLOAD_CHUNK_TIMEOUT_SECS),
                    sftp.write(&handle, offset, chunk),
                ).await;

                match write_result {
                    Ok(res) => {
                        let _ = res.map_err(|e| e.to_string())?;
                    }
                    Err(_) => {
                        return Err(format!(
                            "Chunk upload timeout ({}s) at offset {}",
                            EDIT_UPLOAD_CHUNK_TIMEOUT_SECS,
                            offset
                        ));
                    }
                }

                offset += chunk_size as u64;
            }

            Ok(())
        }.await;

        let _ = sftp.close(handle).await;
        upload_result.map(|_| total_bytes)
    }
}
