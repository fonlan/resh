use crate::ssh_manager::ssh::SSHClient;
use crate::config::types::Proxy;
use serde::{Deserialize, Serialize};
use tauri::{State, Window, Emitter};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::AppState;

#[derive(Debug, Deserialize)]
pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
    pub proxy: Option<Proxy>,
    pub jumphost: Option<JumphostConfig>,
}

#[derive(Debug, Deserialize)]
pub struct JumphostConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
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
        while let Some((session_id, data)) = rx.recv().await {
            // Convert bytes to string (lossy) for xterm.js
            // Note: xterm.js handles UTF-8, so we send string.
            // Ideally we'd send bytes, but Tauri event payload is JSON string usually.
            // String::from_utf8_lossy is safe.
            let text = String::from_utf8_lossy(&data).to_string();
            
            if let Err(e) = window_clone.emit(&format!("terminal-output:{}", session_id), text) {
                tracing::error!("Failed to emit terminal event: {}", e);
            }
        }
    });

    let session_id = SSHClient::connect(
        &params.host,
        params.port,
        &params.username,
        params.password,
        params.private_key,
        params.passphrase,
        params.proxy,
        params.jumphost,
        tx,
    )
    .await?;

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
