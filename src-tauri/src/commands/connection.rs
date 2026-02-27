use crate::ssh_manager::ssh::{SSHClient, ConnectParams};
use serde::{Deserialize, Serialize};
use tauri::{State, Window, Emitter};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use lazy_static::lazy_static;
use std::collections::HashMap;
use tokio::fs::File;
use tokio::io::{BufWriter, AsyncWriteExt};
use tokio::time::{Duration, MissedTickBehavior};

use super::AppState;

lazy_static! {
    static ref RECORDING_SESSIONS: Mutex<HashMap<String, (Arc<Mutex<BufWriter<File>>>, String)>> = Mutex::new(HashMap::new());
}

async fn flush_terminal_output(window: &Window, session_id: &str, pending_output: &mut String) {
    if pending_output.is_empty() {
        return;
    }

    let text = std::mem::take(pending_output);

    if let Err(e) = SSHClient::update_terminal_buffer(session_id, &text).await {
        if e == "Session not found" {
            tracing::debug!(
                "Skipping terminal buffer update for {} before session registration",
                session_id
            );
        } else {
            tracing::error!("Failed to update terminal buffer: {}", e);
        }
    }

    if let Err(e) = window.emit(&format!("terminal-output:{}", session_id), text) {
        tracing::debug!("Failed to emit terminal event for {}: {}", session_id, e);
    }
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
    state: State<'_, Arc<AppState>>,
) -> Result<ConnectResponse, String> {
    // Create channel for receiving SSH data
    let (tx, mut rx) = mpsc::channel::<(String, Vec<u8>)>(100);

    // Spawn a task to forward SSH data to frontend events
    let window_clone = window.clone();
    let state_clone = state.inner().clone();
    tokio::spawn(async move {
        let mut current_session_id = None;
        let mut pending_output = String::new();
        let mut flush_interval = tokio::time::interval(Duration::from_millis(16));
        flush_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                recv = rx.recv() => {
                    let Some((session_id, data)) = recv else {
                        break;
                    };

                    if current_session_id.is_none() {
                        current_session_id = Some(session_id.clone());
                    }

                    // Handle recording if active
                    let recording_target = {
                        let sessions = RECORDING_SESSIONS.lock().await;
                        sessions
                            .get(&session_id)
                            .map(|(writer_mutex, mode)| (Arc::clone(writer_mutex), mode.clone()))
                    };

                    if let Some((writer_mutex, mode)) = recording_target {
                        let mut writer = writer_mutex.lock().await;

                        let data_to_write = if mode == "text" {
                            strip_ansi_escapes::strip(&data)
                        } else {
                            data.clone()
                        };

                        if let Err(e) = writer.write_all(&data_to_write).await {
                            tracing::error!("Failed to write to recording file for session {}: {}", session_id, e);
                        }
                    }

                    let text = String::from_utf8_lossy(&data);
                    pending_output.push_str(&text);

                    if pending_output.len() >= 8192 {
                        flush_terminal_output(&window_clone, &session_id, &mut pending_output).await;
                    }
                }
                _ = flush_interval.tick() => {
                    if let Some(session_id) = current_session_id.as_deref() {
                        flush_terminal_output(&window_clone, session_id, &mut pending_output).await;
                    }
                }
            }
        }

        if let Some(session_id) = current_session_id.as_deref() {
            flush_terminal_output(&window_clone, session_id, &mut pending_output).await;
        }
        
        // Notify frontend that connection is closed
        if let Some(session_id) = current_session_id {
            tracing::info!("SSH Session {} loop ended (connection closed)", session_id);
            
            // Clean up SFTP edit sessions and watchers
            state_clone.sftp_edit_manager.cleanup_session(&session_id);

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
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.sftp_edit_manager.cleanup_session(&session_id);
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
pub async fn select_save_path(default_name: String, initial_dir: Option<String>) -> Result<Option<String>, String> {
    use rfd::FileDialog;
    let path = tokio::task::spawn_blocking(move || {
        let mut dialog = FileDialog::new()
            .set_file_name(&default_name);
        
        if let Some(dir) = initial_dir {
            if !dir.is_empty() {
                dialog = dialog.set_directory(dir);
            }
        }
        
        dialog.save_file()
    }).await.map_err(|e| e.to_string())?;

    Ok(path.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn update_terminal_selection(
    session_id: String,
    selection: String,
) -> Result<(), String> {
    SSHClient::update_terminal_selection(&session_id, selection).await
}
