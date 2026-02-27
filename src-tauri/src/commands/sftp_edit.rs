use tauri::{State, AppHandle, Emitter};
use std::sync::Arc;
use crate::commands::AppState;
use crate::sftp_manager::{SftpManager, TransferProgress};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use std::path::Path;
use uuid::Uuid;
use russh_sftp::protocol::StatusCode;

#[tauri::command]
pub async fn sftp_edit_file(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    remote_path: String,
) -> Result<String, String> {
    // 1. Create unique temp directory for this specific edit task
    let task_uuid = Uuid::new_v4().to_string();
    let temp_dir = std::env::temp_dir()
        .join("resh_sftp")
        .join(&session_id)
        .join(&task_uuid);
    fs::create_dir_all(&temp_dir).await.map_err(|e| e.to_string())?;

    // 2. Determine local filename
    let file_name_str = Path::new(&remote_path)
        .file_name()
        .ok_or("Invalid remote path")?
        .to_string_lossy();
    let local_path = temp_dir.join(file_name_str.as_ref());

    // Kill any existing watch for the same remote file in this session
    // This ensures only one watcher exists per remote file
    state.sftp_edit_manager.stop_watching_remote(&session_id, &remote_path);

    let task_id = Uuid::new_v4().to_string();

    // 3. Download file
    let metadata = SftpManager::metadata(&session_id, &remote_path).await?;
    if metadata.attrs.is_dir() {
        return Err("Not a file (is a directory)".to_string());
    }
    let total_bytes = metadata.attrs.size.unwrap_or(0);

    // Emit Initial Progress
    let _ = app.emit("transfer-progress", TransferProgress {
        task_id: task_id.clone(),
        type_: "download".to_string(),
        session_id: session_id.clone(),
        file_name: file_name_str.to_string(),
        source: remote_path.clone(),
        destination: local_path.to_string_lossy().to_string(),
        total_bytes,
        transferred_bytes: 0,
        speed: 0.0,
        eta: None,
        status: "transferring".to_string(),
        error: None,
    });

    let result: Result<u64, String> = async {
        let sftp = SftpManager::get_session(&session_id).await?;
        let handle = sftp.open(&remote_path, russh_sftp::protocol::OpenFlags::READ, russh_sftp::protocol::FileAttributes::default()).await.map_err(|e| e.to_string())?.handle;
        let mut local_file = fs::File::create(&local_path).await.map_err(|e| e.to_string())?;
        
        let mut offset = 0u64;
        loop {
            let read_len = if total_bytes > 0 {
                let remaining = total_bytes.saturating_sub(offset);
                if remaining == 0 {
                    break;
                }
                std::cmp::min(remaining, (255 * 1024) as u64) as u32
            } else {
                (255 * 1024) as u32
            };

            match sftp.read(&handle, offset, read_len).await {
                Ok(data) => {
                    if data.data.is_empty() {
                        if total_bytes == 0 || offset >= total_bytes {
                            break;
                        }
                        let _ = sftp.close(handle).await;
                        return Err(format!(
                            "Download incomplete: received empty data before EOF ({} / {} bytes)",
                            offset, total_bytes
                        ));
                    }
                    local_file.write_all(&data.data).await.map_err(|e| e.to_string())?;
                    offset += data.data.len() as u64;
                }
                Err(russh_sftp::client::error::Error::Status(status)) if status.status_code == StatusCode::Eof => {
                    if total_bytes > 0 && offset < total_bytes {
                        let _ = sftp.close(handle).await;
                        return Err(format!(
                            "Download incomplete: EOF before full content received ({} / {} bytes)",
                            offset, total_bytes
                        ));
                    }
                    break;
                }
                Err(e) => {
                    let _ = sftp.close(handle).await;
                    return Err(e.to_string());
                }
            }
        }
        local_file.flush().await.map_err(|e| e.to_string())?;
        local_file.sync_all().await.map_err(|e| e.to_string())?;
        drop(local_file);
        let _ = sftp.close(handle).await;
        Ok(offset)
    }.await;

    // Emit Final Progress
    let final_status = if result.is_ok() { "completed" } else { "failed" };
    let downloaded_bytes = result.as_ref().copied().unwrap_or(0);
    let final_error = result.err();

    let _ = app.emit("transfer-progress", TransferProgress {
        task_id,
        type_: "download".to_string(),
        session_id: session_id.clone(),
        file_name: file_name_str.to_string(),
        source: remote_path.clone(),
        destination: local_path.to_string_lossy().to_string(),
        total_bytes,
        transferred_bytes: downloaded_bytes,
        speed: 0.0,
        eta: None,
        status: final_status.to_string(),
        error: final_error.clone(),
    });

    if final_status == "failed" {
        return Err(final_error.unwrap_or_else(|| "Unknown error during download".to_string()));
    }

    // 4. Register watcher
    state.sftp_edit_manager.watch_file(local_path.clone(), session_id, remote_path).map_err(|e| e.to_string())?;

    Ok(local_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_local_editor(
    path: String,
    editor: Option<String>,
) -> Result<(), String> {
    tracing::info!("Opening local editor for path: {}", path);
    
    if let Some(editor_cmd) = editor {
        if !editor_cmd.is_empty() {
             tracing::info!("Using custom editor command: {}", editor_cmd);
             
             std::process::Command::new(&editor_cmd)
                .arg(&path)
                .spawn()
                .map_err(|e| format!("Failed to launch editor: {}", e))?;
             return Ok(());
        }
    }
    
    tracing::info!("Using system default editor");
    open::that(&path).map_err(|e| format!("Failed to open file with default application: {}", e))?;
    
    Ok(())
}

#[tauri::command]
pub async fn pick_folder() -> Result<Option<String>, String> {
    use rfd::FileDialog;
    let folder = tokio::task::spawn_blocking(move || {
        FileDialog::new().pick_folder()
    }).await.map_err(|e| e.to_string())?;

    Ok(folder.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn pick_file() -> Result<Option<String>, String> {
    use rfd::FileDialog;
    let file = tokio::task::spawn_blocking(move || {
        FileDialog::new().pick_file()
    }).await.map_err(|e| e.to_string())?;

    Ok(file.map(|p| p.to_string_lossy().to_string()))
}
