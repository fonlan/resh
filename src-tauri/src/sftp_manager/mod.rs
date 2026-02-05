use russh_sftp::client::RawSftpSession;
use russh_sftp::protocol::{OpenFlags, FileAttributes, StatusCode};
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use std::io::SeekFrom;
use tokio::io::AsyncSeekExt;
use futures::FutureExt;
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
use tokio::sync::oneshot;

pub mod edit;

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
    pub type_: String,
    pub session_id: String,
    pub file_name: String,
    pub source: String,
    pub destination: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub speed: f64,
    pub eta: Option<u64>,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct FileConflict {
    pub task_id: String,
    pub session_id: String,
    pub file_path: String,
    pub local_size: Option<u64>,
    pub remote_size: Option<u64>,
    pub local_modified: Option<u64>,
    pub remote_modified: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum ConflictResolution {
    Overwrite,
    Skip,
    Cancel,
}

lazy_static! {
    static ref SFTP_SESSIONS: Mutex<HashMap<String, Arc<RawSftpSession>>> = Mutex::new(HashMap::new());
    static ref TRANSFER_TASKS: Mutex<HashMap<String, Arc<AtomicBool>>> = Mutex::new(HashMap::new());
    static ref CONFLICT_RESPONSES: Mutex<HashMap<String, oneshot::Sender<ConflictResolution>>> = Mutex::new(HashMap::new());
}


pub struct SftpManager;

impl SftpManager {
    pub async fn metadata(session_id: &str, path: &str) -> Result<russh_sftp::protocol::Attrs, String> {
        let sftp = Self::get_session(session_id).await?;
        sftp.stat(path).await.map_err(|e| e.to_string())
    }

    async fn check_file_exists(sftp: &Arc<RawSftpSession>, path: &str) -> Result<Option<FileAttributes>, String> {
        match sftp.stat(path).await {
            Ok(attrs) => Ok(Some(attrs.attrs)),
            Err(russh_sftp::client::error::Error::Status(status)) if status.status_code == StatusCode::NoSuchFile => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    async fn wait_for_conflict_resolution(
        app: &AppHandle,
        task_id: &str,
        session_id: &str,
        remote_path: &str,
        local_metadata: &std::fs::Metadata,
        remote_attrs: &FileAttributes,
    ) -> Result<ConflictResolution, String> {
        let (tx, rx) = oneshot::channel();
        
        {
            let mut responses = CONFLICT_RESPONSES.lock().await;
            responses.insert(task_id.to_string(), tx);
        }

        let conflict = FileConflict {
            task_id: task_id.to_string(),
            session_id: session_id.to_string(),
            file_path: remote_path.to_string(),
            local_size: Some(local_metadata.len()),
            remote_size: remote_attrs.size,
            local_modified: local_metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()),
            remote_modified: remote_attrs.mtime.map(|m| m as u64),
        };

        app.emit("sftp-file-conflict", conflict)
            .map_err(|e| e.to_string())?;

        match tokio::time::timeout(std::time::Duration::from_secs(300), rx).await {
            Ok(Ok(resolution)) => Ok(resolution),
            Ok(Err(_)) => Err("Conflict resolution sender dropped".to_string()),
            Err(_) => Err("Conflict resolution timeout (5 minutes)".to_string()),
        }
    }

    pub async fn resolve_conflict(task_id: String, resolution: String) -> Result<(), String> {
        let mut responses = CONFLICT_RESPONSES.lock().await;
        if let Some(tx) = responses.remove(&task_id) {
            let res = match resolution.as_str() {
                "overwrite" => ConflictResolution::Overwrite,
                "skip" => ConflictResolution::Skip,
                "cancel" => ConflictResolution::Cancel,
                _ => return Err("Invalid resolution".to_string()),
            };
            tx.send(res).map_err(|_| "Failed to send resolution".to_string())?;
            Ok(())
        } else {
            Err("Conflict not found".to_string())
        }
    }

    pub async fn get_session(session_id: &str) -> Result<Arc<RawSftpSession>, String> {
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

        let sftp = RawSftpSession::new(channel.into_stream());
        sftp.init().await.map_err(|e| format!("Failed to init SFTP session: {}", e))?;

        let sftp = Arc::new(sftp);
        sessions.insert(session_id.to_string(), sftp.clone());

        Ok(sftp)
    }

    pub async fn remove_session(session_id: &str) {
        let mut sessions = SFTP_SESSIONS.lock().await;
        sessions.remove(session_id);
    }

    pub async fn list_dir(session_id: &str, path: &str) -> Result<Vec<FileEntry>, String> {
        let sftp = Self::get_session(session_id).await?;
        let path = if path.is_empty() { "." } else { path };
        
        let handle = sftp.opendir(path).await.map_err(|e| e.to_string())?.handle;

        let mut files = Vec::new();
        loop {
            match sftp.readdir(&handle).await {
                Ok(name) => {
                    for entry in name.files {
                        let file_name = entry.filename;
                        if file_name == "." || file_name == ".." { continue; }

                        let is_dir = entry.attrs.is_dir();
                        let is_symlink = entry.attrs.is_symlink();
                        let size = entry.attrs.size.unwrap_or(0);
                        let permissions = entry.attrs.permissions;
                        let modified = entry.attrs.mtime.unwrap_or(0) as u64;
                        
                        let full_path = if path == "." || path == "/" {
                            format!("/{}", file_name)
                        } else {
                            format!("{}/{}", path.trim_end_matches('/'), file_name)
                        };

                        let mut link_target = None;
                        let mut target_is_dir = None;
                        if is_symlink {
                            if let Ok(target_name) = sftp.readlink(&full_path).await {
                                if let Some(first) = target_name.files.first() {
                                    link_target = Some(first.filename.clone());
                                }
                            }
                            if let Ok(attrs) = sftp.stat(&full_path).await {
                                target_is_dir = Some(attrs.attrs.is_dir());
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
                }
                Err(russh_sftp::client::error::Error::Status(status)) if status.status_code == StatusCode::Eof => break,
                Err(e) => {
                    let _ = sftp.close(handle).await;
                    return Err(e.to_string());
                }
            }
        }
        
        let _ = sftp.close(handle).await;

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
        let _session_id_clone = session_id.clone();
        let _remote_path_clone = remote_path.clone();
        let _local_path_clone = local_path.clone();
        let ai_session_id_clone = ai_session_id.clone();

        let task_id_inner = task_id.clone();
        let session_id_inner = session_id.clone();
        let remote_path_inner = remote_path.clone();
        let local_path_inner = local_path.clone();

        tokio::spawn(async move {
            let metadata = match sftp.stat(&remote_path_inner).await {
                Ok(m) => m.attrs,
                Err(e) => {
                    let _ = app.emit("transfer-progress", TransferProgress {
                        task_id: task_id_inner,
                        type_: "download".to_string(),
                        session_id: session_id_inner,
                        file_name: std::path::Path::new(&remote_path_inner).file_name().unwrap_or_default().to_string_lossy().to_string(),
                        source: remote_path_inner.clone(),
                        destination: local_path_inner.clone(),
                        total_bytes: 0,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "failed".to_string(),
                        error: Some(e.to_string()),
                    });
                    return;
                }
            };
            
            let file_name = std::path::Path::new(&remote_path_inner)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            
            let result = async {
                if metadata.is_dir() {
                    Self::download_dir_recursive(&app, &sftp, &remote_path_inner, &local_path_inner, &task_id_inner, &session_id_inner, &cancel_token).await
                } else {
                    let total_bytes = metadata.size.unwrap_or(0);
                    let handle = sftp.open(&remote_path_inner, OpenFlags::READ, FileAttributes::default()).await.map_err(|e| e.to_string())?.handle;
                    let mut local_file = tokio::fs::OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(&local_path_inner)
                        .await
                        .map_err(|e| e.to_string())?;
                    
                    let mut transferred = 0u64;
                    let chunk_size = 256 * 1024;
                    let max_concurrent_requests = 64;
                    let start_time = Instant::now();
                    let mut last_emit = Instant::now();

                    let _ = app.emit("transfer-progress", TransferProgress {
                        task_id: task_id_inner.clone(),
                        type_: "download".to_string(),
                        session_id: session_id_inner.clone(),
                        file_name: file_name.clone(),
                        source: remote_path_inner.clone(),
                        destination: local_path_inner.clone(),
                        total_bytes,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "transferring".to_string(),
                        error: None,
                    });

                    let mut futures = FuturesUnordered::new();
                    let mut next_offset = 0u64;

                    for _ in 0..max_concurrent_requests {
                        if next_offset >= total_bytes { break; }
                        let current_chunk_size = std::cmp::min(chunk_size, total_bytes - next_offset);
                        let offset = next_offset;
                        let sftp_clone = sftp.clone();
                        let handle_clone = handle.clone();
                        
                        futures.push(async move {
                            let data = sftp_clone.read(handle_clone, offset, current_chunk_size as u32).await;
                            (offset, data)
                        }.boxed());
                        next_offset += current_chunk_size;
                    }

                    while let Some((offset, result)) = futures.next().await {
                        if cancel_token.load(Ordering::SeqCst) {
                            return Err("Cancelled".to_string());
                        }

                        let data = result.map_err(|e| e.to_string())?.data;
                        
                        local_file.seek(SeekFrom::Start(offset)).await.map_err(|e| e.to_string())?;
                        local_file.write_all(&data).await.map_err(|e| e.to_string())?;
                        
                        transferred += data.len() as u64;

                        if last_emit.elapsed().as_millis() > 500 {
                            let duration = start_time.elapsed().as_secs_f64();
                            let speed = if duration > 0.0 { transferred as f64 / duration } else { 0.0 };
                            let eta = if speed > 0.0 {
                                Some(((total_bytes.saturating_sub(transferred)) as f64 / speed) as u64)
                            } else {
                                None
                            };
                            
                            let _ = app.emit("transfer-progress", TransferProgress {
                                task_id: task_id_inner.clone(),
                                type_: "download".to_string(),
                                session_id: session_id_inner.clone(),
                                file_name: file_name.clone(),
                                source: remote_path_inner.clone(),
                                destination: local_path_inner.clone(),
                                total_bytes,
                                transferred_bytes: transferred,
                                speed,
                                eta,
                                status: "transferring".to_string(),
                                error: None,
                            });
                            last_emit = Instant::now();
                        }

                        if next_offset < total_bytes {
                            let current_chunk_size = std::cmp::min(chunk_size, total_bytes - next_offset);
                            let offset = next_offset;
                            let sftp_clone = sftp.clone();
                            let handle_clone = handle.clone();
                            
                            futures.push(Box::pin(async move {
                                let data = sftp_clone.read(handle_clone, offset, current_chunk_size as u32).await;
                                (offset, data)
                            }));
                            next_offset += current_chunk_size;
                        }
                    }
                    local_file.flush().await.map_err(|e| e.to_string())?;
                    let _ = sftp.close(handle).await;
                    Ok(())
                }
            }.await;

            let is_dir = metadata.is_dir();
            let final_total_bytes = if is_dir { 0 } else { metadata.size.unwrap_or(0) };

            let final_status = match &result {
                Ok(_) => "completed",
                Err(e) if e == "Cancelled" => "cancelled",
                Err(_) => "failed",
            };
            
            let final_error = result.as_ref().err().cloned();

            let _ = app.emit("transfer-progress", TransferProgress {
                task_id: task_id_inner,
                type_: "download".to_string(),
                session_id: session_id_inner,
                file_name: file_name.clone(),
                source: remote_path_inner.clone(),
                destination: local_path_inner.clone(),
                total_bytes: final_total_bytes,
                transferred_bytes: if final_status == "completed" { final_total_bytes } else { 0 },
                speed: 0.0,
                eta: None,
                status: final_status.to_string(),
                error: final_error.clone(),
            });

            if let Some(ai_sid) = ai_session_id_clone {
                let msg_id = Uuid::new_v4().to_string();
                let content = match &result {
                    Ok(_) => format!("SFTP Download completed successfully: {} -> {}", remote_path_inner, local_path_inner),
                    Err(e) => format!("SFTP Download failed: {}. Path: {}", e, remote_path_inner),
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
        sftp: &Arc<RawSftpSession>,
        remote_dir: &str,
        local_dir: &str,
        task_id: &str,
        session_id: &str,
        cancel_token: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        tokio::fs::create_dir_all(local_dir).await.map_err(|e| e.to_string())?;
        
        let dir_handle = sftp.opendir(remote_dir).await.map_err(|e| e.to_string())?.handle;
        loop {
            if cancel_token.load(Ordering::SeqCst) {
                let _ = sftp.close(dir_handle).await;
                return Err("Cancelled".to_string());
            }

            match sftp.readdir(&dir_handle).await {
                Ok(name) => {
                    for entry in name.files {
                        let file_name = entry.filename;
                        if file_name == "." || file_name == ".." { continue; }

                        let remote_path = format!("{}/{}", remote_dir.trim_end_matches('/'), file_name);
                        let local_path = std::path::Path::new(local_dir).join(&file_name).to_string_lossy().to_string();
                        let is_dir = entry.attrs.is_dir();

                        if is_dir {
                            Box::pin(Self::download_dir_recursive(app, sftp, &remote_path, &local_path, task_id, session_id, cancel_token)).await?;
                        } else {
                            let metadata = sftp.stat(&remote_path).await.map_err(|e| e.to_string())?.attrs;
                            let total_bytes = metadata.size.unwrap_or(0);
                            let handle = sftp.open(&remote_path, OpenFlags::READ, FileAttributes::default()).await.map_err(|e| e.to_string())?.handle;
                            let mut local_file = tokio::fs::File::create(&local_path).await.map_err(|e| e.to_string())?;
                            let mut transferred = 0u64;
                            let mut last_emit = Instant::now();
                            
                            let _ = app.emit("transfer-progress", TransferProgress {
                                task_id: task_id.to_string(),
                                type_: "download".to_string(),
                                session_id: session_id.to_string(),
                                file_name: file_name.clone(),
                                source: remote_path.clone(),
                                destination: local_path.clone(),
                                total_bytes,
                                transferred_bytes: 0,
                                speed: 0.0,
                                eta: None,
                                status: "transferring".to_string(),
                                error: None,
                            });

                            loop {
                                if cancel_token.load(Ordering::SeqCst) {
                                    let _ = sftp.close(handle).await;
                                    let _ = sftp.close(dir_handle).await;
                                    let _ = app.emit("transfer-progress", TransferProgress {
                                        task_id: task_id.to_string(),
                                        type_: "download".to_string(),
                                        session_id: session_id.to_string(),
                                        file_name: file_name.clone(),
                                        source: remote_path.clone(),
                                        destination: local_path.clone(),
                                        total_bytes,
                                        transferred_bytes: transferred,
                                        speed: 0.0,
                                        eta: None,
                                        status: "cancelled".to_string(),
                                        error: None,
                                    });
                                    return Err("Cancelled".to_string());
                                }
                                match sftp.read(&handle, transferred, (256 * 1024) as u32).await {
                                    Ok(data) => {
                                        if data.data.is_empty() { break; }
                                        local_file.write_all(&data.data).await.map_err(|e| e.to_string())?;
                                        transferred += data.data.len() as u64;
                                        
                                        if last_emit.elapsed().as_millis() > 500 {
                                            let _ = app.emit("transfer-progress", TransferProgress {
                                                task_id: task_id.to_string(),
                                                type_: "download".to_string(),
                                                session_id: session_id.to_string(),
                                                file_name: file_name.clone(),
                                                source: remote_path.clone(),
                                                destination: local_path.clone(),
                                                total_bytes,
                                                transferred_bytes: transferred,
                                                speed: 0.0,
                                                eta: None,
                                                status: "transferring".to_string(),
                                                error: None,
                                            });
                                            last_emit = Instant::now();
                                        }
                                    }
                                    Err(russh_sftp::client::error::Error::Status(status)) if status.status_code == StatusCode::Eof => break,
                                    Err(e) => {
                                        let _ = app.emit("transfer-progress", TransferProgress {
                                            task_id: task_id.to_string(),
                                            type_: "download".to_string(),
                                            session_id: session_id.to_string(),
                                            file_name: file_name.clone(),
                                            source: remote_path.clone(),
                                            destination: local_path.clone(),
                                            total_bytes,
                                            transferred_bytes: transferred,
                                            speed: 0.0,
                                            eta: None,
                                            status: "failed".to_string(),
                                            error: Some(e.to_string()),
                                        });
                                        return Err(e.to_string());
                                    }
                                }
                            }
                            let _ = app.emit("transfer-progress", TransferProgress {
                                task_id: task_id.to_string(),
                                type_: "download".to_string(),
                                session_id: session_id.to_string(),
                                file_name: file_name.clone(),
                                source: remote_path.clone(),
                                destination: local_path.clone(),
                                total_bytes,
                                transferred_bytes: total_bytes,
                                speed: 0.0,
                                eta: None,
                                status: "completed".to_string(),
                                error: None,
                            });
                            local_file.flush().await.map_err(|e| e.to_string())?;
                            let _ = sftp.close(handle).await;
                        }
                    }
                }
                Err(russh_sftp::client::error::Error::Status(status)) if status.status_code == StatusCode::Eof => break,
                Err(e) => {
                    let _ = sftp.close(dir_handle).await;
                    return Err(e.to_string());
                }
            }
        }
        let _ = sftp.close(dir_handle).await;
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
        let _session_id_clone = session_id.clone();
        let _remote_path_clone = remote_path.clone();
        let _local_path_clone = local_path.clone();
        let ai_session_id_clone = ai_session_id.clone();

        let task_id_inner = task_id.clone();
        let session_id_inner = session_id.clone();
        let remote_path_inner = remote_path.clone();
        let local_path_inner = local_path.clone();

        tokio::spawn(async move {
            let local_metadata = match tokio::fs::metadata(&local_path_inner).await {
                Ok(m) => m,
                Err(e) => {
                    let _ = app.emit("transfer-progress", TransferProgress {
                        task_id: task_id_inner,
                        type_: "upload".to_string(),
                        session_id: session_id_inner,
                        file_name: std::path::Path::new(&local_path_inner).file_name().unwrap_or_default().to_string_lossy().to_string(),
                        source: local_path_inner.clone(),
                        destination: remote_path_inner.clone(),
                        total_bytes: 0,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "failed".to_string(),
                        error: Some(e.to_string()),
                    });
                    return;
                }
            };
            let file_name = std::path::Path::new(&local_path_inner)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            
            let result = async {
                if local_metadata.is_dir() {
                    Self::upload_dir_recursive(&app, &sftp, &local_path_inner, &remote_path_inner, &task_id_inner, &session_id_inner, &cancel_token).await
                } else {
                    // Check if remote file exists
                    if let Some(remote_attrs) = Self::check_file_exists(&sftp, &remote_path_inner).await? {
                        let std_metadata = std::fs::metadata(&local_path_inner).map_err(|e| e.to_string())?;
                        let resolution = Self::wait_for_conflict_resolution(
                            &app,
                            &task_id_inner,
                            &session_id_inner,
                            &remote_path_inner,
                            &std_metadata,
                            &remote_attrs,
                        ).await?;

                        match resolution {
                            ConflictResolution::Skip => {
                                return Err("Skipped".to_string());
                            },
                            ConflictResolution::Cancel => {
                                return Err("Cancelled".to_string());
                            },
                            ConflictResolution::Overwrite => {
                                // Continue with upload
                            }
                        }
                    }

                    let total_bytes = local_metadata.len();
                    let mut local_file = tokio::fs::File::open(&local_path_inner).await.map_err(|e| e.to_string())?;
                    let handle = sftp.open(&remote_path_inner, OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE, FileAttributes::default()).await.map_err(|e| e.to_string())?.handle;
                    
                    let mut transferred = 0u64;
                    let chunk_size = 32 * 1024; // 32KB chunks
                    let max_concurrent_requests = 16; // 16 concurrent requests
                    let start_time = Instant::now();
                    let mut last_emit = Instant::now();

                    let _ = app.emit("transfer-progress", TransferProgress {
                        task_id: task_id_inner.clone(),
                        type_: "upload".to_string(),
                        session_id: session_id_inner.clone(),
                        file_name: file_name.clone(),
                        source: local_path_inner.clone(),
                        destination: remote_path_inner.clone(),
                        total_bytes,
                        transferred_bytes: 0,
                        speed: 0.0,
                        eta: None,
                        status: "transferring".to_string(),
                        error: None,
                    });

                    let mut futures = FuturesUnordered::new();
                    let mut next_offset = 0u64;

                    for _ in 0..max_concurrent_requests {
                        if next_offset >= total_bytes { break; }
                        let current_chunk_size = std::cmp::min(chunk_size, total_bytes - next_offset);
                        let offset = next_offset;
                        
                        let mut buffer = vec![0u8; current_chunk_size as usize];
                        local_file.seek(SeekFrom::Start(offset)).await.map_err(|e| e.to_string())?;
                        local_file.read_exact(&mut buffer).await.map_err(|e| e.to_string())?;
                        
                        let sftp_clone = sftp.clone();
                        let handle_clone = handle.clone();
                        
                        futures.push(async move {
                            // 30 seconds timeout per chunk
                            let res = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                sftp_clone.write(handle_clone, offset, buffer)
                            ).await;
                            match res {
                                Ok(r) => r,
                                Err(_) => Err(russh_sftp::client::error::Error::IO(
                                    "Chunk upload timeout (30s)".to_string()
                                ))
                            }
                        }.boxed());
                        next_offset += current_chunk_size;
                    }

                    while let Some(result) = futures.next().await {
                        if cancel_token.load(Ordering::SeqCst) {
                            return Err("Cancelled".to_string());
                        }

                        result.map_err(|e| e.to_string())?;
                        
                        transferred += chunk_size; 
                        if transferred > total_bytes { transferred = total_bytes; }

                        if last_emit.elapsed().as_millis() > 500 {
                            let duration = start_time.elapsed().as_secs_f64();
                            let speed = if duration > 0.0 { transferred as f64 / duration } else { 0.0 };
                            let eta = if speed > 0.0 {
                                Some(((total_bytes.saturating_sub(transferred)) as f64 / speed) as u64)
                            } else {
                                None
                            };
                            
                            let _ = app.emit("transfer-progress", TransferProgress {
                                task_id: task_id_inner.clone(),
                                type_: "upload".to_string(),
                                session_id: session_id_inner.clone(),
                                file_name: file_name.clone(),
                                source: local_path_inner.clone(),
                                destination: remote_path_inner.clone(),
                                total_bytes,
                                transferred_bytes: transferred,
                                speed,
                                eta,
                                status: "transferring".to_string(),
                                error: None,
                            });
                            last_emit = Instant::now();
                        }

                        if next_offset < total_bytes {
                            let current_chunk_size = std::cmp::min(chunk_size, total_bytes - next_offset);
                            let offset = next_offset;
                            
                            let mut buffer = vec![0u8; current_chunk_size as usize];
                            local_file.seek(SeekFrom::Start(offset)).await.map_err(|e| e.to_string())?;
                            local_file.read_exact(&mut buffer).await.map_err(|e| e.to_string())?;

                            let sftp_clone = sftp.clone();
                            let handle_clone = handle.clone();
                            
                            futures.push(Box::pin(async move {
                                // 30 seconds timeout per chunk
                                let res = tokio::time::timeout(
                                    std::time::Duration::from_secs(30),
                                    sftp_clone.write(handle_clone, offset, buffer)
                                ).await;
                                match res {
                                    Ok(r) => r,
                                    Err(_) => Err(russh_sftp::client::error::Error::IO(
                                        "Chunk upload timeout (30s)".to_string()
                                    ))
                                }
                            }));
                            next_offset += current_chunk_size;
                        }
                    }
                    let _ = sftp.close(handle).await;
                    Ok(())
                }
            }.await;

            let is_dir = local_metadata.is_dir();
            let final_total_bytes = if is_dir { 0 } else { local_metadata.len() };

            let final_status = match &result {
                Ok(_) => "completed",
                Err(e) if e == "Cancelled" => "cancelled",
                Err(e) if e == "Skipped" => "cancelled",
                Err(_) => "failed",
            };
            
            let final_error = match &result {
                Err(e) if e == "Skipped" => Some("Skipped by user".to_string()),
                Err(e) if e == "Cancelled" => Some("Cancelled by user".to_string()),
                Ok(_) => None,
                Err(e) => Some(e.clone()),
            };

            let _ = app.emit("transfer-progress", TransferProgress {
                task_id: task_id_inner,
                type_: "upload".to_string(),
                session_id: session_id_inner,
                file_name: file_name.clone(),
                source: local_path_inner.clone(),
                destination: remote_path_inner.clone(),
                total_bytes: final_total_bytes,
                transferred_bytes: if final_status == "completed" { final_total_bytes } else { 0 },
                speed: 0.0,
                eta: None,
                status: final_status.to_string(),
                error: final_error,
            });

            if let Some(ai_sid) = ai_session_id_clone {
                let msg_id = Uuid::new_v4().to_string();
                let content = match &result {
                    Ok(_) => format!("SFTP Upload completed successfully: {} -> {}", local_path_inner, remote_path_inner),
                    Err(e) => format!("SFTP Upload failed: {}. Path: {}", e, local_path_inner),
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
        app: &AppHandle,
        sftp: &Arc<RawSftpSession>,
        local_dir: &str,
        remote_dir: &str,
        task_id: &str,
        session_id: &str,
        cancel_token: &Arc<AtomicBool>,
    ) -> Result<(), String> {
        sftp.mkdir(remote_dir, FileAttributes::default()).await.ok();
        
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
                Box::pin(Self::upload_dir_recursive(app, sftp, &local_path, &remote_path, task_id, session_id, cancel_token)).await?;
            } else {
                // Check if remote file exists
                if let Some(remote_attrs) = Self::check_file_exists(sftp, &remote_path).await? {
                    let std_metadata = std::fs::metadata(&local_path).map_err(|e| e.to_string())?;
                    let conflict_task_id = format!("{}-{}", task_id, file_name.to_string_lossy());
                    let resolution = Self::wait_for_conflict_resolution(
                        app,
                        &conflict_task_id,
                        session_id,
                        &remote_path,
                        &std_metadata,
                        &remote_attrs,
                    ).await?;

                    match resolution {
                        ConflictResolution::Skip => {
                            // Send cancelled status for skipped file in folder upload
                            let _ = app.emit("transfer-progress", TransferProgress {
                                task_id: format!("{}-{}", task_id, file_name.to_string_lossy()),
                                type_: "upload".to_string(),
                                session_id: session_id.to_string(),
                                file_name: file_name.to_string_lossy().to_string(),
                                source: local_path.clone(),
                                destination: remote_path.clone(),
                                total_bytes: std_metadata.len(),
                                transferred_bytes: 0,
                                speed: 0.0,
                                eta: None,
                                status: "cancelled".to_string(),
                                error: Some("Skipped by user".to_string()),
                            });
                            continue;
                        },
                        ConflictResolution::Cancel => {
                            return Err("Cancelled".to_string());
                        },
                        ConflictResolution::Overwrite => {
                            // Continue with upload
                        }
                    }
                }

                let local_metadata = tokio::fs::metadata(&local_path).await.map_err(|e| e.to_string())?;
                let total_bytes = local_metadata.len();
                let mut local_file = tokio::fs::File::open(&local_path).await.map_err(|e| e.to_string())?;
                let handle = sftp.open(&remote_path, OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE, FileAttributes::default()).await.map_err(|e| e.to_string())?.handle;
                let mut buffer = [0u8; 256 * 1024];
                let mut offset = 0u64;
                let mut transferred = 0u64;
                let mut last_emit = Instant::now();
                
                let _ = app.emit("transfer-progress", TransferProgress {
                    task_id: task_id.to_string(),
                    type_: "upload".to_string(),
                    session_id: session_id.to_string(),
                    file_name: file_name.to_string_lossy().to_string(),
                    source: local_path.clone(),
                    destination: remote_path.clone(),
                    total_bytes,
                    transferred_bytes: 0,
                    speed: 0.0,
                    eta: None,
                    status: "transferring".to_string(),
                    error: None,
                });

                loop {
                    if cancel_token.load(Ordering::SeqCst) {
                        let _ = sftp.close(handle).await;
                        let _ = app.emit("transfer-progress", TransferProgress {
                            task_id: task_id.to_string(),
                            type_: "upload".to_string(),
                            session_id: session_id.to_string(),
                            file_name: file_name.to_string_lossy().to_string(),
                            source: local_path.clone(),
                            destination: remote_path.clone(),
                            total_bytes,
                            transferred_bytes: transferred,
                            speed: 0.0,
                            eta: None,
                            status: "cancelled".to_string(),
                            error: Some("Cancelled by user".to_string()),
                        });
                        return Err("Cancelled".to_string());
                    }
                    let n = local_file.read(&mut buffer).await.map_err(|e| e.to_string())?;
                    if n == 0 { break; }
                    match sftp.write(&handle, offset, buffer[..n].to_vec()).await {
                        Ok(_) => {
                            offset += n as u64;
                            transferred += n as u64;
                            
                            if last_emit.elapsed().as_millis() > 500 {
                                let _ = app.emit("transfer-progress", TransferProgress {
                                    task_id: task_id.to_string(),
                                    type_: "upload".to_string(),
                                    session_id: session_id.to_string(),
                                    file_name: file_name.to_string_lossy().to_string(),
                                    source: local_path.clone(),
                                    destination: remote_path.clone(),
                                    total_bytes,
                                    transferred_bytes: transferred,
                                    speed: 0.0,
                                    eta: None,
                                    status: "transferring".to_string(),
                                    error: None,
                                });
                                last_emit = Instant::now();
                            }
                        }
                        Err(e) => {
                            let _ = app.emit("transfer-progress", TransferProgress {
                                task_id: task_id.to_string(),
                                type_: "upload".to_string(),
                                session_id: session_id.to_string(),
                                file_name: file_name.to_string_lossy().to_string(),
                                source: local_path.clone(),
                                destination: remote_path.clone(),
                                total_bytes,
                                transferred_bytes: transferred,
                                speed: 0.0,
                                eta: None,
                                status: "failed".to_string(),
                                error: Some(e.to_string()),
                            });
                            return Err(e.to_string());
                        }
                    }
                }
                let _ = app.emit("transfer-progress", TransferProgress {
                    task_id: task_id.to_string(),
                    type_: "upload".to_string(),
                    session_id: session_id.to_string(),
                    file_name: file_name.to_string_lossy().to_string(),
                    source: local_path.clone(),
                    destination: remote_path.clone(),
                    total_bytes,
                    transferred_bytes: total_bytes,
                    speed: 0.0,
                    eta: None,
                    status: "completed".to_string(),
                    error: None,
                });
                let _ = sftp.close(handle).await;
            }
        }
        Ok(())
    }

    pub async fn create_directory(session_id: &str, path: &str) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        sftp.mkdir(path, FileAttributes::default()).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn create_file(session_id: &str, path: &str) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        let handle = sftp.open(path, OpenFlags::CREATE | OpenFlags::WRITE, FileAttributes::default()).await.map_err(|e| e.to_string())?.handle;
        let _ = sftp.close(handle).await;
        Ok(())
    }

    pub async fn delete_item(session_id: &str, path: &str, is_dir: bool) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;

        if !is_dir {
            sftp.remove(path).await.map_err(|e| e.to_string())?;
        } else {
            Self::remove_dir_recursive(&sftp, path).await?;
        }

        Ok(())
    }

    async fn remove_dir_recursive(sftp: &Arc<RawSftpSession>, path: &str) -> Result<(), String> {
        let handle = sftp.opendir(path).await.map_err(|e| e.to_string())?.handle;

        loop {
            match sftp.readdir(&handle).await {
                Ok(name) => {
                    for entry in name.files {
                        let file_name = entry.filename;
                        if file_name == "." || file_name == ".." {
                            continue;
                        }

                        let entry_path = format!("{}/{}", path.trim_end_matches('/'), file_name);
                        let is_dir = entry.attrs.is_dir();

                        if is_dir {
                            Box::pin(Self::remove_dir_recursive(sftp, &entry_path)).await?;
                        } else {
                            sftp.remove(&entry_path).await.map_err(|e| e.to_string())?;
                        }
                    }
                }
                Err(russh_sftp::client::error::Error::Status(status)) if status.status_code == StatusCode::Eof => break,
                Err(e) => {
                    let _ = sftp.close(handle).await;
                    return Err(e.to_string());
                }
            }
        }

        let _ = sftp.close(handle).await;
        sftp.rmdir(path).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn chmod(session_id: &str, path: &str, mode: u32) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;

        let mut attrs = sftp.stat(path).await.map_err(|e| e.to_string())?.attrs;
        attrs.permissions = Some(mode);

        sftp.setstat(path, attrs).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn rename_item(session_id: &str, old_path: &str, new_path: &str) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        sftp.rename(old_path, new_path).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn copy_item(session_id: &str, source_path: &str, dest_path: &str) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;

        let metadata = sftp.stat(source_path).await.map_err(|e| e.to_string())?.attrs;
        let is_dir = metadata.is_dir();

        if is_dir {
            Self::copy_dir_recursive(&sftp, source_path, dest_path).await?;
        } else {
            Self::copy_file(&sftp, source_path, dest_path).await?;
        }

        Ok(())
    }

    async fn copy_file(sftp: &Arc<RawSftpSession>, source: &str, dest: &str) -> Result<(), String> {
        let handle = sftp.open(source, OpenFlags::READ, FileAttributes::default()).await.map_err(|e| e.to_string())?.handle;

        let mut attrs = FileAttributes::default();
        if let Ok(source_attrs) = sftp.stat(source).await {
            attrs.permissions = source_attrs.attrs.permissions;
        }

        let dest_handle = sftp.open(dest, OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE, attrs).await.map_err(|e| e.to_string())?.handle;

        let total_bytes = sftp.fstat(&handle).await.map_err(|e| e.to_string())?.attrs.size.unwrap_or(0);
        let chunk_size = 256 * 1024;

        let mut futures = FuturesUnordered::new();
        let mut next_offset = 0u64;
        let max_concurrent_requests = 64;

        for _ in 0..max_concurrent_requests {
            if next_offset >= total_bytes { break; }
            let current_chunk_size = std::cmp::min(chunk_size, total_bytes - next_offset);
            let offset = next_offset;
            let sftp_clone = sftp.clone();
            let handle_clone = handle.clone();

            futures.push(async move {
                let data = sftp_clone.read(handle_clone, offset, current_chunk_size as u32).await;
                (offset, data)
            }.boxed());
            next_offset += current_chunk_size;
        }

        while let Some((offset, result)) = futures.next().await {
            let data = result.map_err(|e| e.to_string())?.data;
            sftp.write(&dest_handle, offset, data).await.map_err(|e| e.to_string())?;

            if next_offset < total_bytes {
                let current_chunk_size = std::cmp::min(chunk_size, total_bytes - next_offset);
                let offset = next_offset;
                let sftp_clone = sftp.clone();
                let handle_clone = handle.clone();

                futures.push(Box::pin(async move {
                    let data = sftp_clone.read(handle_clone, offset, current_chunk_size as u32).await;
                    (offset, data)
                }));
                next_offset += current_chunk_size;
            }
        }

        let _ = sftp.close(handle).await;
        let _ = sftp.close(dest_handle).await;
        Ok(())
    }

    async fn copy_dir_recursive(sftp: &Arc<RawSftpSession>, source: &str, dest: &str) -> Result<(), String> {
        sftp.mkdir(dest, FileAttributes::default()).await.map_err(|e| e.to_string())?;

        let handle = sftp.opendir(source).await.map_err(|e| e.to_string())?.handle;

        loop {
            match sftp.readdir(&handle).await {
                Ok(name) => {
                    for entry in name.files {
                        let file_name = entry.filename;
                        if file_name == "." || file_name == ".." { continue; }

                        let source_path = format!("{}/{}", source.trim_end_matches('/'), file_name);
                        let dest_path = format!("{}/{}", dest.trim_end_matches('/'), file_name);
                        let is_dir = entry.attrs.is_dir();

                        if is_dir {
                            Box::pin(Self::copy_dir_recursive(sftp, &source_path, &dest_path)).await?;
                        } else {
                            Box::pin(async {
                                Self::copy_file(sftp, &source_path, &dest_path).await
                            }).await?;
                        }
                    }
                }
                Err(russh_sftp::client::error::Error::Status(status)) if status.status_code == StatusCode::Eof => break,
                Err(e) => {
                    let _ = sftp.close(handle).await;
                    return Err(e.to_string());
                }
            }
        }

        let _ = sftp.close(handle).await;
        Ok(())
    }
}
