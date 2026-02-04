use russh_sftp::client::SftpSession;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use lazy_static::lazy_static;
use crate::ssh_manager::ssh::SSHClient;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::io::AsyncReadExt;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;
use std::time::Instant;
use crate::db::DatabaseManager;

pub mod edit;

lazy_static! {
    static ref SFTP_SESSIONS: Mutex<HashMap<String, Arc<SftpSession>>> = Mutex::new(HashMap::new());
    static ref TRANSFER_TASKS: Mutex<HashMap<String, Arc<AtomicBool>>> = Mutex::new(HashMap::new());
}

#[derive(Serialize, Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: u64,
    pub link_target: Option<String>,
    pub target_is_dir: Option<bool>,
    pub permissions: Option<u32>,
}

#[derive(Serialize, Clone, Debug)]
pub struct TransferProgress {
    pub task_id: String,
    pub type_: String, // "download" or "upload"
    pub session_id: String,
    pub file_name: String,
    pub source: String,
    pub destination: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub speed: f64, // bytes per second
    pub eta: Option<u64>, // estimated seconds remaining
    pub status: String, // "pending", "transferring", "completed", "failed", "cancelled"
    pub error: Option<String>,
}

pub struct SftpManager;

impl SftpManager {
    pub async fn get_session(session_id: &str) -> Result<Arc<SftpSession>, String> {
        let mut sessions = SFTP_SESSIONS.lock().await;
        if let Some(s) = sessions.get(session_id) {
            return Ok(s.clone());
        }

        let ssh_session = SSHClient::get_session_handle(session_id).await
            .ok_or("SSH session not found")?;

        let channel = ssh_session.channel_open_session().await
            .map_err(|e| format!("Failed to open channel: {}", e))?;

        channel.request_subsystem(true, "sftp").await
            .map_err(|e| format!("Failed to request SFTP subsystem: {}", e))?;

        let sftp = SftpSession::new(channel.into_stream()).await
             .map_err(|e| format!("Failed to init SFTP session: {}", e))?;

        let sftp = Arc::new(sftp);
        sessions.insert(session_id.to_string(), sftp.clone());

        Ok(sftp)
    }

    pub async fn list_dir(session_id: &str, path: &str) -> Result<Vec<FileEntry>, String> {
        let sftp = Self::get_session(session_id).await?;
        let path = if path.is_empty() { "." } else { path };
        
        let entries = sftp.read_dir(path).await.map_err(|e| e.to_string())?;

        let mut files = Vec::new();
        for entry in entries {
             let file_name = entry.file_name();
             if file_name == "." || file_name == ".." { continue; }

              let metadata = entry.metadata();
              let is_dir = metadata.is_dir();
              let is_symlink = metadata.is_symlink();
              let size = metadata.len();
              let permissions = metadata.permissions;
              let modified = metadata.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                 .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
             
             let full_path = if path == "." || path == "/" {
                 format!("/{}", file_name)
             } else {
                 format!("{}/{}", path.trim_end_matches('/'), file_name)
             };

             let mut link_target = None;
             let mut target_is_dir = None;
             if is_symlink {
                 if let Ok(target) = sftp.read_link(&full_path).await {
                     link_target = Some(target.to_string());
                 }
                 if let Ok(metadata) = sftp.metadata(&full_path).await {
                     target_is_dir = Some(metadata.is_dir());
                 }
             }

              files.push(FileEntry {
                  name: file_name,
                  path: full_path,
                  is_dir,
                  is_symlink,
                  size,
                  modified,
                  link_target,
                  target_is_dir,
                  permissions,
              });
        }
        
        files.sort_by(|a, b| {
            if a.is_dir == b.is_dir {
                a.name.cmp(&b.name)
            } else {
                b.is_dir.cmp(&a.is_dir)
            }
        });

        Ok(files)
    }

    pub async fn cancel_transfer(task_id: &str) -> Result<(), String> {
        let tasks = TRANSFER_TASKS.lock().await;
        if let Some(token) = tasks.get(task_id) {
            token.store(true, Ordering::SeqCst);
            Ok(())
        } else {
            Err("Task not found".to_string())
        }
    }

    pub async fn download_file(
        app: AppHandle,
        db_manager: DatabaseManager,
        session_id: String,
        remote_path: String,
        local_path: String,
        ai_session_id: Option<String>,
    ) -> Result<String, String> {
        let sftp = Self::get_session(&session_id).await?;
        let task_id = Uuid::new_v4().to_string();
        let cancel_token = Arc::new(AtomicBool::new(false));
        
        {
            let mut tasks = TRANSFER_TASKS.lock().await;
            tasks.insert(task_id.clone(), cancel_token.clone());
        }

        let task_id_clone = task_id.clone();
        let session_id_clone = session_id.clone();
        let remote_path_clone = remote_path.clone();
        let local_path_clone = local_path.clone();
        let ai_session_id_clone = ai_session_id.clone();

        tokio::spawn(async move {
            let result = async {
                let metadata = sftp.metadata(&remote_path_clone).await.map_err(|e| e.to_string())?;
                
                let file_name = std::path::Path::new(&remote_path_clone)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                if metadata.is_dir() {
                    Self::download_dir_recursive(&app, &sftp, &remote_path_clone, &local_path_clone, &task_id_clone, &session_id_clone, &cancel_token).await
                } else {
                    let total_bytes = metadata.len();
                    let mut remote_file = sftp.open(&remote_path_clone).await.map_err(|e| e.to_string())?;
                    let mut local_file = tokio::fs::File::create(&local_path_clone).await.map_err(|e| e.to_string())?;
                    
                    let mut transferred = 0u64;
                    let mut buffer = [0u8; 32 * 1024];
                    let start_time = Instant::now();
                    let mut last_emit = Instant::now();

                    let _ = app.emit("transfer-progress", TransferProgress {
                        task_id: task_id_clone.clone(),
                        type_: "download".to_string(),
                        session_id: session_id_clone.clone(),
                        file_name: file_name.clone(),
                        source: remote_path_clone.clone(),
                        destination: local_path_clone.clone(),
                        total_bytes,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "transferring".to_string(),
                        error: None,
                    });

                    loop {
                        if cancel_token.load(Ordering::SeqCst) {
                            return Err("Cancelled".to_string());
                        }

                        let n = remote_file.read(&mut buffer).await.map_err(|e| e.to_string())?;
                        if n == 0 { break; }
                        
                        local_file.write_all(&buffer[..n]).await.map_err(|e| e.to_string())?;
                        transferred += n as u64;

                        if last_emit.elapsed().as_millis() > 500 {
                            let duration = start_time.elapsed().as_secs_f64();
                            let speed = if duration > 0.0 { transferred as f64 / duration } else { 0.0 };
                            let eta = if speed > 0.0 {
                                Some(((total_bytes.saturating_sub(transferred)) as f64 / speed) as u64)
                            } else {
                                None
                            };
                            
                            let _ = app.emit("transfer-progress", TransferProgress {
                                task_id: task_id_clone.clone(),
                                type_: "download".to_string(),
                                session_id: session_id_clone.clone(),
                                file_name: file_name.clone(),
                                source: remote_path_clone.clone(),
                                destination: local_path_clone.clone(),
                                total_bytes,
                                transferred_bytes: transferred,
                                speed,
                                eta,
                                status: "transferring".to_string(),
                                error: None,
                            });
                            last_emit = Instant::now();
                        }
                    }
                    local_file.flush().await.map_err(|e| e.to_string())?;
                    Ok(())
                }
            }.await;

            let final_status = match &result {
                Ok(_) => "completed",
                Err(e) if e == "Cancelled" => "cancelled",
                Err(_) => "failed",
            };
            
            let final_error = result.as_ref().err().cloned();

            let _ = app.emit("transfer-progress", TransferProgress {
                task_id: task_id_clone.clone(),
                type_: "download".to_string(),
                session_id: session_id_clone,
                file_name: std::path::Path::new(&remote_path_clone).file_name().unwrap_or_default().to_string_lossy().to_string(),
                source: remote_path_clone.clone(),
                destination: local_path_clone.clone(),
                total_bytes: 0, 
                transferred_bytes: 0,
                speed: 0.0,
                eta: None,
                status: final_status.to_string(),
                error: final_error.clone(),
            });

            if let Some(ai_sid) = ai_session_id_clone {
                let msg_id = Uuid::new_v4().to_string();
                let content = match &result {
                    Ok(_) => format!("SFTP Download completed successfully: {} -> {}", remote_path_clone, local_path_clone),
                    Err(e) => format!("SFTP Download failed: {}. Path: {}", e, remote_path_clone),
                };


                let conn = db_manager.get_connection();
                let conn = conn.lock().unwrap();
                let _ = conn.execute(
                    "INSERT INTO ai_messages (id, session_id, role, content) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![msg_id, ai_sid, "system", content],
                );
                
                let _ = app.emit(&format!("ai-message-batch-{}", ai_sid), vec![
                    serde_json::json!({
                        "role": "system",
                        "content": content
                    })
                ]);
                let _ = app.emit(&format!("ai-done-{}", ai_sid), "DONE");
            }

            let mut tasks = TRANSFER_TASKS.lock().await;
            tasks.remove(&task_id_clone);
        });

        Ok(task_id)
    }

    async fn download_dir_recursive(
        app: &AppHandle,
        sftp: &Arc<SftpSession>,
        remote_dir: &str,
        local_dir: &str,
        task_id: &str,
        session_id: &str,
        cancel_token: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        tokio::fs::create_dir_all(local_dir).await.map_err(|e| e.to_string())?;
        
        let entries = sftp.read_dir(remote_dir).await.map_err(|e| e.to_string())?;
        for entry in entries {
            if cancel_token.load(Ordering::SeqCst) {
                return Err("Cancelled".to_string());
            }

            let file_name = entry.file_name();
            if file_name == "." || file_name == ".." { continue; }

            let remote_path = format!("{}/{}", remote_dir.trim_end_matches('/'), file_name);
            let local_path = std::path::Path::new(local_dir).join(&file_name).to_string_lossy().to_string();
            let metadata = entry.metadata();

            if metadata.is_dir() {
                Box::pin(Self::download_dir_recursive(app, sftp, &remote_path, &local_path, task_id, session_id, cancel_token)).await?;
            } else {
                let mut remote_file = sftp.open(&remote_path).await.map_err(|e| e.to_string())?;
                let mut local_file = tokio::fs::File::create(&local_path).await.map_err(|e| e.to_string())?;
                let mut buffer = [0u8; 32 * 1024];
                
                loop {
                    if cancel_token.load(Ordering::SeqCst) {
                        return Err("Cancelled".to_string());
                    }
                    let n = remote_file.read(&mut buffer).await.map_err(|e| e.to_string())?;
                    if n == 0 { break; }
                    local_file.write_all(&buffer[..n]).await.map_err(|e| e.to_string())?;
                }
                local_file.flush().await.map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    pub async fn upload_file(
        app: AppHandle,
        db_manager: DatabaseManager,
        session_id: String,
        local_path: String,
        remote_path: String,
        ai_session_id: Option<String>,
    ) -> Result<String, String> {
        let sftp = Self::get_session(&session_id).await?;
        let task_id = Uuid::new_v4().to_string();
        let cancel_token = Arc::new(AtomicBool::new(false));
        
        {
            let mut tasks = TRANSFER_TASKS.lock().await;
            tasks.insert(task_id.clone(), cancel_token.clone());
        }

        let task_id_clone = task_id.clone();
        let session_id_clone = session_id.clone();
        let remote_path_clone = remote_path.clone();
        let local_path_clone = local_path.clone();
        let ai_session_id_clone = ai_session_id.clone();

        tokio::spawn(async move {
            let result = async {
                let local_metadata = tokio::fs::metadata(&local_path_clone).await.map_err(|e| e.to_string())?;
                
                let file_name = std::path::Path::new(&local_path_clone)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                if local_metadata.is_dir() {
                    Self::upload_dir_recursive(&app, &sftp, &local_path_clone, &remote_path_clone, &task_id_clone, &session_id_clone, &cancel_token).await
                } else {
                    let total_bytes = local_metadata.len();
                    let mut local_file = tokio::fs::File::open(&local_path_clone).await.map_err(|e| e.to_string())?;
                    let mut remote_file = sftp.create(&remote_path_clone).await.map_err(|e| e.to_string())?;
                    
                    let mut transferred = 0u64;
                    let mut buffer = [0u8; 32 * 1024];
                    let start_time = Instant::now();
                    let mut last_emit = Instant::now();

                    let _ = app.emit("transfer-progress", TransferProgress {
                        task_id: task_id_clone.clone(),
                        type_: "upload".to_string(),
                        session_id: session_id_clone.clone(),
                        file_name: file_name.clone(),
                        source: local_path_clone.clone(),
                        destination: remote_path_clone.clone(),
                        total_bytes,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "transferring".to_string(),
                        error: None,
                    });

                    loop {
                        if cancel_token.load(Ordering::SeqCst) {
                            return Err("Cancelled".to_string());
                        }

                        let n = local_file.read(&mut buffer).await.map_err(|e| e.to_string())?;
                        if n == 0 { break; }
                        
                        remote_file.write_all(&buffer[..n]).await.map_err(|e| e.to_string())?;
                        transferred += n as u64;

                        if last_emit.elapsed().as_millis() > 500 {
                            let duration = start_time.elapsed().as_secs_f64();
                            let speed = if duration > 0.0 { transferred as f64 / duration } else { 0.0 };
                            let eta = if speed > 0.0 {
                                Some(((total_bytes.saturating_sub(transferred)) as f64 / speed) as u64)
                            } else {
                                None
                            };
                            
                            let _ = app.emit("transfer-progress", TransferProgress {
                                task_id: task_id_clone.clone(),
                                type_: "upload".to_string(),
                                session_id: session_id_clone.clone(),
                                file_name: file_name.clone(),
                                source: local_path_clone.clone(),
                                destination: remote_path_clone.clone(),
                                total_bytes,
                                transferred_bytes: transferred,
                                speed,
                                eta,
                                status: "transferring".to_string(),
                                error: None,
                            });
                            last_emit = Instant::now();
                        }
                    }
                    Ok(())
                }
            }.await;

            let final_status = match &result {
                Ok(_) => "completed",
                Err(e) if e == "Cancelled" => "cancelled",
                Err(_) => "failed",
            };
            
            let final_error = result.as_ref().err().cloned();

            let _ = app.emit("transfer-progress", TransferProgress {
                task_id: task_id_clone.clone(),
                type_: "upload".to_string(),
                session_id: session_id_clone,
                file_name: std::path::Path::new(&local_path_clone).file_name().unwrap_or_default().to_string_lossy().to_string(),
                source: local_path_clone.clone(),
                destination: remote_path_clone.clone(),
                total_bytes: 0, 
                transferred_bytes: 0,
                speed: 0.0,
                eta: None,
                status: final_status.to_string(),
                error: final_error,
            });

            if let Some(ai_sid) = ai_session_id_clone {
                let msg_id = Uuid::new_v4().to_string();
                let content = match &result {
                    Ok(_) => format!("SFTP Upload completed successfully: {} -> {}", local_path_clone, remote_path_clone),
                    Err(e) => format!("SFTP Upload failed: {}. Path: {}", e, local_path_clone),
                };

                let conn = db_manager.get_connection();
                let conn = conn.lock().unwrap();
                let _ = conn.execute(
                    "INSERT INTO ai_messages (id, session_id, role, content) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![msg_id, ai_sid, "system", content],
                );
                
                let _ = app.emit(&format!("ai-message-batch-{}", ai_sid), vec![
                    serde_json::json!({
                        "role": "system",
                        "content": content
                    })
                ]);
                let _ = app.emit(&format!("ai-done-{}", ai_sid), "DONE");
            }

            let mut tasks = TRANSFER_TASKS.lock().await;
            tasks.remove(&task_id_clone);
        });

        Ok(task_id)
    }

    async fn upload_dir_recursive(
        _app: &AppHandle,
        sftp: &Arc<SftpSession>,
        local_dir: &str,
        remote_dir: &str,
        _task_id: &str,
        _session_id: &str,
        cancel_token: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        sftp.create_dir(remote_dir).await.ok();
        
        let mut entries = tokio::fs::read_dir(local_dir).await.map_err(|e| e.to_string())?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
            if cancel_token.load(Ordering::SeqCst) {
                return Err("Cancelled".to_string());
            }

            let file_name = entry.file_name();
            let local_path = entry.path().to_string_lossy().to_string();
            let remote_path = format!("{}/{}", remote_dir.trim_end_matches('/'), file_name.to_string_lossy());
            let metadata = entry.metadata().await.map_err(|e| e.to_string())?;

            if metadata.is_dir() {
                Box::pin(Self::upload_dir_recursive(_app, sftp, &local_path, &remote_path, _task_id, _session_id, cancel_token)).await?;
            } else {
                let mut local_file = tokio::fs::File::open(&local_path).await.map_err(|e| e.to_string())?;
                let mut remote_file = sftp.create(&remote_path).await.map_err(|e| e.to_string())?;
                let mut buffer = [0u8; 32 * 1024];
                
                loop {
                    if cancel_token.load(Ordering::SeqCst) {
                        return Err("Cancelled".to_string());
                    }
                    let n = local_file.read(&mut buffer).await.map_err(|e| e.to_string())?;
                    if n == 0 { break; }
                    remote_file.write_all(&buffer[..n]).await.map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }

    pub async fn create_directory(session_id: &str, path: &str) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        sftp.create_dir(path).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn create_file(session_id: &str, path: &str) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        let mut file = sftp.create(path).await.map_err(|e| e.to_string())?;
        file.shutdown().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn delete_item(session_id: &str, path: &str, is_dir: bool) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;

        if !is_dir {
            sftp.remove_file(path).await.map_err(|e| e.to_string())?;
        } else {
            Self::remove_dir_recursive(&sftp, path).await?;
        }

        Ok(())
    }

    async fn remove_dir_recursive(sftp: &Arc<SftpSession>, path: &str) -> Result<(), String> {
        let entries = sftp.read_dir(path).await.map_err(|e| e.to_string())?;

        for entry in entries {
            let file_name = entry.file_name();
            if file_name == "." || file_name == ".." {
                continue;
            }

            let entry_path = format!("{}/{}", path.trim_end_matches('/'), file_name);

            let metadata = entry.metadata();
            let is_dir = metadata.is_dir();

            if is_dir {
                Box::pin(Self::remove_dir_recursive(sftp, &entry_path)).await?;
            } else {
                sftp.remove_file(&entry_path).await.map_err(|e| e.to_string())?;
            }
        }

        sftp.remove_dir(path).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn chmod(session_id: &str, path: &str, mode: u32) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;

        let current = sftp.metadata(path).await.map_err(|e| e.to_string())?;
        let mut attrs = current.clone();
        attrs.permissions = Some(mode);

        sftp.set_metadata(path, attrs).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn rename_item(session_id: &str, old_path: &str, new_path: &str) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        sftp.rename(old_path, new_path).await.map_err(|e| e.to_string())?;
        Ok(())
    }
}
