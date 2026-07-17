// src-tauri/src/commands/updater.rs

use crate::commands::AppState;
use crate::updater::{check_for_update, CheckForUpdateOptions, CheckUpdateResult};
use std::sync::Arc;
use tauri::State;

/// Check GitHub Releases for a newer stable version.
///
/// `force` skips the automatic minimum interval (used by manual checks).
/// All callers share a single in-flight request to avoid duplicate API traffic.
#[tauri::command]
pub async fn check_for_update_cmd(
    state: State<'_, Arc<AppState>>,
    force: Option<bool>,
) -> Result<CheckUpdateResult, String> {
    let (proxies, proxy_id) = {
        let config = state.config.lock().await;
        let proxies = config.proxies.clone();
        let proxy_id = config.general.update.proxy_id.clone();
        (proxies, proxy_id)
    };

    let result = check_for_update(
        &proxies,
        proxy_id.as_deref(),
        CheckForUpdateOptions {
            force: force.unwrap_or(false),
            current_version: None,
            platform: None,
        },
    )
    .await;

    Ok(result)
}

/// Return the compile-time app version used for update comparison.
#[tauri::command]
pub async fn get_app_version_cmd() -> Result<String, String> {
    Ok(crate::updater::current_app_version().to_string())
}
