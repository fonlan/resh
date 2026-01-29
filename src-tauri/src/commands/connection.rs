use crate::ssh_manager::ssh::{SSHClient, ConnectParams};
use serde::{Deserialize, Serialize};
use tauri::{State, Window, Emitter};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::AppState;

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
            // Convert bytes to string (lossy) for xterm.js
            // Note: xterm.js handles UTF-8, so we send string.
            // Ideally we'd send bytes, but Tauri event payload is JSON string usually.
            // String::from_utf8_lossy is safe.
            let text = String::from_utf8_lossy(&data).to_string();
            
            if let Err(e) = window_clone.emit(&format!("terminal-output:{}", session_id), text) {
                tracing::error!("Failed to emit terminal event: {}", e);
            }
        }
        
        // Notify frontend that connection is closed
        if let Some(session_id) = current_session_id {
            if let Err(e) = window_clone.emit(&format!("connection-closed:{}", session_id), ()) {
                tracing::error!("Failed to emit connection-closed event: {}", e);
            }
        }
    });

    let session_id = SSHClient::connect(params, tx).await?;

    Ok(ConnectResponse { session_id })
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
