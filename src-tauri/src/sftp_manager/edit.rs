use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{error, info};
use crate::sftp_manager::{SftpManager, TransferProgress};
use tokio::io::AsyncWriteExt;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

pub struct SftpEditManager {
    watched_files: Arc<Mutex<HashMap<PathBuf, (String, String)>>>,
    // watcher must be kept alive
    watcher: Arc<Mutex<Option<RecommendedWatcher>>>,
    tx: mpsc::Sender<PathBuf>,
    app: AppHandle,
}

impl SftpEditManager {
    pub fn new(app: AppHandle) -> Self {
        let (tx, mut rx) = mpsc::channel::<PathBuf>(100);
        let watched_files = Arc::new(Mutex::new(HashMap::<PathBuf, (String, String)>::new()));
        let watched_files_clone = watched_files.clone();
        let app_clone = app.clone();

        // Spawn background task to handle file changes
        tokio::spawn(async move {
            while let Some(path) = rx.recv().await {
                info!("File modified: {:?}", path);
                
                let (session_id, remote_path) = {
                    let files = watched_files_clone.lock().unwrap();
                    if let Some((sid, rpath)) = files.get(&path) {
                        (sid.clone(), rpath.clone())
                    } else {
                        continue;
                    }
                };

                info!("Uploading modified file {:?} to remote {}", path, remote_path);

                let task_id = Uuid::new_v4().to_string();
                let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

                // 1. Emit Initial Progress (Pending/Started)
                let _ = app_clone.emit("transfer-progress", TransferProgress {
                    task_id: task_id.clone(),
                    type_: "upload".to_string(),
                    session_id: session_id.clone(),
                    file_name: file_name.clone(),
                    source: path.to_string_lossy().to_string(),
                    destination: remote_path.clone(),
                    total_bytes: 0,
                    transferred_bytes: 0,
                    speed: 0.0,
                    eta: None,
                    status: "transferring".to_string(),
                    error: None,
                });

                // Perform upload
                let result = async {
                    let content = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
                    let total_bytes = content.len() as u64;
                    
                    let sftp = SftpManager::get_session(&session_id).await?;
                    let mut remote_file = sftp.create(&remote_path).await.map_err(|e| e.to_string())?;
                    remote_file.write_all(&content).await.map_err(|e| e.to_string())?;
                    
                    Ok(total_bytes)
                }.await;

                // 2. Emit Final Progress (Completed/Failed)
                let (final_status, final_error, total_bytes) = match result {
                    Ok(bytes) => ("completed".to_string(), None, bytes),
                    Err(e) => {
                        error!("Upload failed: {}", e);
                        ("failed".to_string(), Some(e), 0)
                    }
                };

                let _ = app_clone.emit("transfer-progress", TransferProgress {
                    task_id,
                    type_: "upload".to_string(),
                    session_id,
                    file_name,
                    source: path.to_string_lossy().to_string(),
                    destination: remote_path,
                    total_bytes,
                    transferred_bytes: total_bytes,
                    speed: 0.0,
                    eta: None,
                    status: final_status,
                    error: final_error,
                });
            }
        });

        let tx_clone = tx.clone();
        let watcher = RecommendedWatcher::new(move |res: Result<notify::Event, notify::Error>| {
            match res {
                Ok(event) => {
                    // Editors often use atomic writes (rename), so we should listen to Create/Modify/Remove/Rename
                    // We just forward the path, the loop checks if it's a watched file
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
            watcher: Arc::new(Mutex::new(watcher)),
            tx,
            app,
        }
    }

    pub fn watch_file(&self, local_path: PathBuf, session_id: String, remote_path: String) -> Result<(), String> {
        let mut files = self.watched_files.lock().unwrap();
        files.insert(local_path.clone(), (session_id, remote_path));

        let mut watcher_guard = self.watcher.lock().unwrap();
        if let Some(watcher) = watcher_guard.as_mut() {
            // Watch the parent directory to catch atomic renames/replacements
            if let Some(parent) = local_path.parent() {
                if let Err(e) = watcher.watch(parent, RecursiveMode::NonRecursive) {
                    return Err(format!("Failed to watch directory: {}", e));
                }
            } else {
                // Fallback to watching file if no parent (unlikely)
                if let Err(e) = watcher.watch(&local_path, RecursiveMode::NonRecursive) {
                    return Err(format!("Failed to watch file: {}", e));
                }
            }
        } else {
            return Err("Watcher not initialized".to_string());
        }
        
        Ok(())
    }

    pub fn stop_watching(&self, local_path: &PathBuf) {
        let mut files = self.watched_files.lock().unwrap();
        files.remove(local_path);
        
        let mut watcher_guard = self.watcher.lock().unwrap();
        if let Some(watcher) = watcher_guard.as_mut() {
            let _ = watcher.unwatch(local_path);
        }
    }
}
