// src-tauri/src/commands/master_password.rs
// Tauri commands for master password management

use serde::{Deserialize, Serialize};
use tauri::State;
use std::sync::Arc;

use super::AppState;

/// Response for master password status
#[derive(Debug, Serialize)]
pub struct MasterPasswordStatusResponse {
    pub is_set: bool,
}

/// Parameters for setting the master password
#[derive(Debug, Deserialize)]
pub struct SetMasterPasswordParams {
    pub password: String,
}

/// Parameters for verifying the master password
#[derive(Debug, Deserialize)]
pub struct VerifyMasterPasswordParams {
    pub password: String,
}

/// Response for password verification
#[derive(Debug, Serialize)]
pub struct VerifyMasterPasswordResponse {
    pub is_valid: bool,
}

/// Check if a master password has been set
/// 
/// # Arguments
/// * `state` - Application state containing the password manager
/// 
/// # Returns
/// A response indicating whether a master password is set
#[tauri::command]
pub async fn get_master_password_status(
    state: State<'_, Arc<AppState>>,
) -> Result<MasterPasswordStatusResponse, String> {
    let is_set = state.password_manager.is_set()?;
    Ok(MasterPasswordStatusResponse { is_set })
}

/// Set the master password
/// 
/// # Arguments
/// * `params` - The password to set
/// * `state` - Application state containing the password manager
/// 
/// # Returns
/// Result indicating success or failure
#[tauri::command]
pub async fn set_master_password(
    params: SetMasterPasswordParams,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.password_manager.set_password(&params.password)?;
    Ok(())
}

/// Verify the master password
/// 
/// # Arguments
/// * `params` - The password to verify
/// * `state` - Application state containing the password manager
/// 
/// # Returns
/// A response indicating whether the password is valid
#[tauri::command]
pub async fn verify_master_password(
    params: VerifyMasterPasswordParams,
    state: State<'_, Arc<AppState>>,
) -> Result<VerifyMasterPasswordResponse, String> {
    let is_valid = state.password_manager.verify_password(&params.password)?;
    Ok(VerifyMasterPasswordResponse { is_valid })
}
