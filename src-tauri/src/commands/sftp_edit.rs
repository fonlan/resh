use tauri::{State, AppHandle, Emitter};
use std::sync::Arc;
use crate::commands::AppState;
use crate::sftp_manager::{SftpManager, TransferProgress};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use std::path::Path;
use uuid::Uuid;

#[tauri::command]
pub async fn sftp_edit_file(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    remote_path: String,
) -> Result<String, String> {
    // 1. Create temp directory
    let temp_dir = std::env::temp_dir().join("resh_sftp").join(&session_id);
    fs::create_dir_all(&temp_dir).await.map_err(|e| e.to_string())?;

    // 2. Determine local filename
    let file_name_str = Path::new(&remote_path)
        .file_name()
        .ok_or("Invalid remote path")?
        .to_string_lossy();
    let local_path = temp_dir.join(file_name_str.as_ref());

    let task_id = Uuid::new_v4().to_string();

    // 3. Download file
    let sftp = SftpManager::get_session(&session_id).await?;
    
    // Check if file exists and is a file
    let metadata = sftp.metadata(&remote_path).await.map_err(|e| e.to_string())?;
    if metadata.is_dir() {
        return Err("Not a file (is a directory)".to_string());
    }
    let total_bytes = metadata.len();

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

    let result = async {
        let mut remote_file = sftp.open(&remote_path).await.map_err(|e| e.to_string())?;
        let mut local_file = fs::File::create(&local_path).await.map_err(|e| e.to_string())?;
        
        // Copy content
        tokio::io::copy(&mut remote_file, &mut local_file).await.map_err(|e| e.to_string())?;
        local_file.flush().await.map_err(|e| e.to_string())?;
        Ok(())
    }.await;

    // Emit Final Progress
    let final_status = if result.is_ok() { "completed" } else { "failed" };
    let final_error = result.err();

    let _ = app.emit("transfer-progress", TransferProgress {
        task_id,
        type_: "download".to_string(),
        session_id: session_id.clone(),
        file_name: file_name_str.to_string(),
        source: remote_path.clone(),
        destination: local_path.to_string_lossy().to_string(),
        total_bytes,
        transferred_bytes: total_bytes,
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
    if let Some(editor_cmd) = editor {
        if !editor_cmd.is_empty() {
             // Open with specific editor
             // We allow arguments in the editor string?
             // Simple implementation: split by space (naive)
             // Better: Use shlex or just assume first part is cmd, rest args?
             // Requirement says: "C:\Windows\system32\notepad.exe"
             // Usually just the path.
             
             std::process::Command::new(editor_cmd)
                .arg(path)
                .spawn()
                .map_err(|e| e.to_string())?;
             return Ok(());
        }
    }
    
    // Default open
    open::that(path).map_err(|e| e.to_string())?;
    
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
