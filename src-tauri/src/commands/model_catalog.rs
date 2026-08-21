//! Tauri commands exposing the models.dev model catalog to the settings UI.
//!
//! - `model_catalog_lookup(model_id)` — best match + candidates for auto-fill.
//! - `model_catalog_status()` — cache status (entries, fetchedAt, freshness).
//! - `model_catalog_refresh()` — force a fetch now (scheduled refresh runs in
//!   the background regardless).

use std::sync::Arc;

use tauri::State;

use crate::commands::AppState;
use crate::model_catalog::{CatalogLookup, CatalogStatus, ModelCatalog};

#[tauri::command]
pub async fn model_catalog_lookup(
    state: State<'_, Arc<AppState>>,
    model_id: String,
) -> Result<CatalogLookup, String> {
    let trimmed = model_id.trim();
    if trimmed.is_empty() {
        return Err("model_id must be a non-empty string".to_string());
    }
    if trimmed.len() > 200 {
        return Err("model_id is too long".to_string());
    }
    state.model_catalog.lookup(trimmed).await
}

#[tauri::command]
pub async fn model_catalog_status(
    state: State<'_, Arc<AppState>>,
) -> Result<CatalogStatus, String> {
    Ok(state.model_catalog.status().await)
}

#[tauri::command]
pub async fn model_catalog_refresh(
    state: State<'_, Arc<AppState>>,
) -> Result<CatalogStatus, String> {
    state.model_catalog.ensure_fresh(true).await?;
    Ok(state.model_catalog.status().await)
}

/// Helper for main.rs: the background scheduled refresh loop. Checks
/// freshness every CHECK_INTERVAL and fetches when stale; failures are logged
/// and the loop continues (the last good cache stays in place).
pub fn spawn_scheduled_refresh(catalog: ModelCatalog) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(crate::model_catalog::CHECK_INTERVAL).await;
            if let Err(e) = catalog.ensure_fresh(false).await {
                tracing::warn!("model catalog: scheduled refresh failed: {e}");
            }
        }
    });
}
