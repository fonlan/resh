use crate::sftp_manager::{SftpManager, FileEntry};
use tauri::AppHandle;

#[tauri::command]
pub async fn sftp_list_dir(session_id: String, path: String) -> Result<Vec<FileEntry>, String> {
    SftpManager::list_dir(&session_id, &path).await
}

#[tauri::command]
pub async fn sftp_download(app: AppHandle, session_id: String, remote_path: String, local_path: String) -> Result<String, String> {
    SftpManager::download_file(app, session_id, remote_path, local_path).await
}

#[tauri::command]
pub async fn sftp_upload(app: AppHandle, session_id: String, local_path: String, remote_path: String) -> Result<String, String> {
    SftpManager::upload_file(app, session_id, local_path, remote_path).await
}

#[tauri::command]
pub async fn sftp_cancel_transfer(task_id: String) -> Result<(), String> {
    SftpManager::cancel_transfer(&task_id).await
}

#[tauri::command]
pub async fn pick_files() -> Result<Option<Vec<String>>, String> {
    use rfd::FileDialog;
    let files = tokio::task::spawn_blocking(move || {
        FileDialog::new()
            .pick_files()
    }).await.map_err(|e| e.to_string())?;

    if let Some(files) = files {
        Ok(Some(files.into_iter().map(|p| p.to_string_lossy().to_string()).collect()))
    } else {
        Ok(None)
    }
}
