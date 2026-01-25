// src-tauri/src/commands/connection.rs
// Tauri commands for SSH connection management

use crate::ssh_manager::ssh::SSHClient;
use serde::{Deserialize, Serialize};
use tauri::State;
use std::sync::Arc;

use super::AppState;

/// Parameters for connecting to an SSH server
#[derive(Debug, Deserialize)]
pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

/// Response from a successful connection
#[derive(Debug, Serialize)]
pub struct ConnectResponse {
    pub session_id: String,
}

/// Parameters for sending a command
#[derive(Debug, Deserialize)]
pub struct CommandParams {
    pub session_id: String,
    pub command: String,
}

/// Response from sending a command
#[derive(Debug, Serialize)]
pub struct CommandResponse {
    pub output: String,
}

/// Connect to an SSH server
/// 
/// # Arguments
/// * `params` - Connection parameters (host, port, username, password)
/// * `state` - Application state
/// 
/// # Returns
/// A session ID for the established connection
#[tauri::command]
pub async fn connect_to_server(
    params: ConnectParams,
    _state: State<'_, Arc<AppState>>,
) -> Result<ConnectResponse, String> {
    let session_id = SSHClient::connect(
        &params.host,
        params.port,
        &params.username,
        &params.password,
    )?;

    Ok(ConnectResponse { session_id })
}

/// Send a command to an SSH session
/// 
/// # Arguments
/// * `params` - Command parameters (session_id, command)
/// * `state` - Application state
/// 
/// # Returns
/// The command output
#[tauri::command]
pub async fn send_command(
    params: CommandParams,
    _state: State<'_, Arc<AppState>>,
) -> Result<CommandResponse, String> {
    let output = SSHClient::send_command(&params.session_id, &params.command)?;

    Ok(CommandResponse { output })
}

/// Close an SSH session
/// 
/// # Arguments
/// * `session_id` - The session ID to close
/// * `state` - Application state
/// 
/// # Returns
/// Result indicating success or failure
#[tauri::command]
pub async fn close_session(
    session_id: String,
    _state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    SSHClient::close_session(&session_id)?;
    Ok(())
}
