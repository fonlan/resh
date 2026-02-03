use russh_sftp::client::SftpSession;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use lazy_static::lazy_static;
use crate::ssh_manager::ssh::SSHClient;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::io::AsyncReadExt;

lazy_static! {
    static ref SFTP_SESSIONS: Mutex<HashMap<String, Arc<SftpSession>>> = Mutex::new(HashMap::new());
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
}

pub struct SftpManager;

impl SftpManager {
    pub async fn get_session(session_id: &str) -> Result<Arc<SftpSession>, String> {
        let mut sessions = SFTP_SESSIONS.lock().await;
        if let Some(s) = sessions.get(session_id) {
            return Ok(s.clone());
        }

        // Create new session
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
             let modified = metadata.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
             
             // Normalize path
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
                 // Try to determine if target is a directory by stating the link path (which follows link)
                 // Note: This might fail if the link is broken
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
             });
        }
        
        // Sort: Directories first, then files
        files.sort_by(|a, b| {
            if a.is_dir == b.is_dir {
                a.name.cmp(&b.name)
            } else {
                b.is_dir.cmp(&a.is_dir)
            }
        });

        Ok(files)
    }

    pub async fn download_file(session_id: &str, remote_path: &str, local_path: &str) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        let mut remote_file = sftp.open(remote_path).await.map_err(|e| e.to_string())?;
        let mut local_file = tokio::fs::File::create(local_path).await.map_err(|e| e.to_string())?;
        
        let mut buffer = [0u8; 32 * 1024];
        loop {
            let n = remote_file.read(&mut buffer).await.map_err(|e| e.to_string())?;
            if n == 0 { break; }
            local_file.write_all(&buffer[..n]).await.map_err(|e| e.to_string())?;
        }
        
        local_file.flush().await.map_err(|e| e.to_string())?;
        
        Ok(())
    }

    pub async fn upload_file(session_id: &str, local_path: &str, remote_path: &str) -> Result<(), String> {
        let sftp = Self::get_session(session_id).await?;
        let mut local_file = tokio::fs::File::open(local_path).await.map_err(|e| e.to_string())?;
        let mut remote_file = sftp.create(remote_path).await.map_err(|e| e.to_string())?;
        
        let mut buffer = [0u8; 32 * 1024];
        loop {
            let n = local_file.read(&mut buffer).await.map_err(|e| e.to_string())?;
            if n == 0 { break; }
            remote_file.write_all(&buffer[..n]).await.map_err(|e| e.to_string())?;
        }
        
        Ok(())
    }
}
