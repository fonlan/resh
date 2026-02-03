use crate::ssh_manager::ssh::{SSHClient, ConnectParams};
use serde::{Deserialize, Serialize};
use tauri::{State, Window, Emitter};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use lazy_static::lazy_static;
use std::collections::HashMap;
use tokio::fs::File;
use tokio::io::{BufWriter, AsyncWriteExt};

use super::AppState;

lazy_static! {
    static ref RECORDING_SESSIONS: Mutex<HashMap<String, (Arc<Mutex<BufWriter<File>>>, String)>> = Mutex::new(HashMap::new());
}

#[derive(Debug, Serialize)]
pub struct ConnectResponse {
    pub session_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CommandParams {
    pub session_id: String,
    pub command: String, // Kept 'command' name for compatibility, but it's raw input
}

#[derive(Debug, Serialize)]
pub struct CommandResponse {
    pub output: String,
}

#[derive(Debug, Deserialize)]
pub struct ResizeParams {
    pub session_id: String,
    pub cols: u32,
    pub rows: u32,
}

#[tauri::command]
pub async fn connect_to_server(
    window: Window,
    params: ConnectParams,
    _state: State<'_, Arc<AppState>>,
) -> Result<ConnectResponse, String> {
    // Create channel for receiving SSH data
    let (tx, mut rx) = mpsc::channel::<(String, Vec<u8>)>(100);

    // Spawn a task to forward SSH data to frontend events
    let window_clone = window.clone();
    tokio::spawn(async move {
        let mut current_session_id = None;
        while let Some((session_id, data)) = rx.recv().await {
            if current_session_id.is_none() {
                current_session_id = Some(session_id.clone());
            }

            // Handle recording if active
            {
                let sessions = RECORDING_SESSIONS.lock().await;
                if let Some((writer_mutex, mode)) = sessions.get(&session_id) {
                    let mut writer = writer_mutex.lock().await;
                    
                    let data_to_write = if mode == "text" {
                        strip_ansi_escapes::strip(&data)
                    } else {
                        data.clone()
                    };

                    if let Err(e) = writer.write_all(&data_to_write).await {
                        tracing::error!("Failed to write to recording file for session {}: {}", session_id, e);
                    } else if let Err(e) = writer.flush().await {
                         tracing::error!("Failed to flush recording file for session {}: {}", session_id, e);
                    }
                }
            }

            // Convert bytes to string (lossy) for xterm.js
            // Note: xterm.js handles UTF-8, so we send string.
            // Ideally we'd send bytes, but Tauri event payload is JSON string usually.
            // String::from_utf8_lossy is safe.
            let text = String::from_utf8_lossy(&data).to_string();
            
            // Update terminal buffer for AI access
            use crate::ssh_manager::ssh::SSHClient;
            if let Err(e) = SSHClient::update_terminal_buffer(&session_id, &text).await {
                tracing::error!("Failed to update terminal buffer: {}", e);
            }
            
            if let Err(e) = window_clone.emit(&format!("terminal-output:{}", session_id), text) {
                // If the window is closed, we expect emit to fail eventually.
                // Log at debug level to avoid spamming info/error logs.
                tracing::debug!("Failed to emit terminal event for {}: {}", session_id, e);
            }
        }
        
        // Notify frontend that connection is closed
        if let Some(session_id) = current_session_id {
            tracing::info!("SSH Session {} loop ended (connection closed)", session_id);
            // Ensure recording is stopped
            {
                let mut sessions = RECORDING_SESSIONS.lock().await;
                sessions.remove(&session_id);
            }
            
            if let Err(e) = window_clone.emit(&format!("connection-closed:{}", session_id), ()) {
                tracing::debug!("Failed to emit connection-closed event: {}", e);
            }
        }
    });

    let session_id = SSHClient::connect(params.clone(), tx).await?;
    tracing::info!("SSH Session {} established", session_id);

    let session_id_clone = session_id.clone();
    tokio::spawn(async move {
        match SSHClient::gather_system_info(params).await {
            Ok(info) => {
                if let Err(e) = SSHClient::update_system_info(&session_id_clone, info).await {
                    tracing::error!("Failed to update system info for {}: {}", session_id_clone, e);
                }
            }
            Err(e) => {
                tracing::debug!("Failed to gather system info for {} (this is expected for some systems): {}", session_id_clone, e);
            }
        }
    });

    Ok(ConnectResponse { session_id })
}

#[tauri::command]
pub async fn start_recording(
    session_id: String,
    file_path: String,
    mode: String,
) -> Result<(), String> {
    let file = File::create(&file_path).await.map_err(|e| format!("Failed to create file: {}", e))?;
    let writer = BufWriter::new(file);
    
    let mut sessions = RECORDING_SESSIONS.lock().await;
    sessions.insert(session_id, (Arc::new(Mutex::new(writer)), mode));
    
    Ok(())
}

#[tauri::command]
pub async fn stop_recording(
    session_id: String,
) -> Result<(), String> {
    let mut sessions = RECORDING_SESSIONS.lock().await;
    if let Some((writer_mutex, _)) = sessions.remove(&session_id) {
        let mut writer = writer_mutex.lock().await;
        writer.flush().await.map_err(|e| format!("Failed to flush file: {}", e))?;
        // File closes when dropped
    }
    Ok(())
}

#[tauri::command]
pub async fn send_command(
    params: CommandParams,
    _state: State<'_, Arc<AppState>>,
) -> Result<CommandResponse, String> {
    // Convert string input to bytes
    let data = params.command.as_bytes();
    
    SSHClient::send_input(&params.session_id, data).await?;

    // No echo here - the SSH server will echo back if appropriate (default for PTY)
    Ok(CommandResponse { output: String::new() })
}

#[tauri::command]
pub async fn resize_terminal(
    params: ResizeParams,
    _state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    SSHClient::resize(&params.session_id, params.cols, params.rows).await?;
    Ok(())
}

#[tauri::command]
pub async fn close_session(
    session_id: String,
    _state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    SSHClient::disconnect(&session_id).await?;
    Ok(())
}

#[tauri::command]
pub async fn reconnect_session(
    session_id: String,
    _state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    SSHClient::reconnect(&session_id).await?;
    Ok(())
}

#[tauri::command]
pub async fn export_terminal_log(
    content: String,
    default_path: String,
) -> Result<(), String> {
    use rfd::FileDialog;
    use std::fs;

    let path = FileDialog::new()
        .set_file_name(&default_path)
        .add_filter("Text", &["txt"])
        .add_filter("All Files", &["*"])
        .save_file();

    if let Some(path) = path {
        fs::write(path, content).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Save cancelled".to_string())
    }
}

#[tauri::command]
pub async fn select_save_path(default_name: String) -> Result<Option<String>, String> {
    use rfd::FileDialog;
    
    // Run in blocking task to avoid blocking the async runtime
    let path = tokio::task::spawn_blocking(move || {
        FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("Text", &["txt", "log"])
            .add_filter("All Files", &["*"])
            .save_file()
    }).await.map_err(|e| e.to_string())?;

    if let Some(path) = path {
        Ok(Some(path.to_string_lossy().to_string()))
    } else {
        Ok(None)
    }
}
