use crate::commands::AppState;
use crate::webdav::WebDAVClient;
use crate::webdav::conflict::{detect_conflict, SyncMetadata};
use tauri::State;
use std::sync::Arc;
use std::fs;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SyncResult {
    pub status: String, // "success", "conflict", "error"
    pub message: String,
    pub remote_content: Option<String>, // For conflict resolution (encrypted string)
}

#[tauri::command]
pub async fn sync_webdav(state: State<'_, Arc<AppState>>) -> Result<SyncResult, String> {
    // 1. Load local WebDAV settings (from local.json)
    // Note: load_local_config might fail if file doesn't exist, handle gracefully
    let local_config = match state.config_manager.load_local_config() {
        Ok(config) => config,
        Err(_) => return Err("Failed to load local configuration".to_string()),
    };
    
    let settings = local_config.general.webdav;
    
    if settings.url.is_empty() {
        return Err("WebDAV URL not configured".to_string());
    }

    // 2. Initialize WebDAV client
    let client = WebDAVClient::new(settings.url, settings.username, settings.password);
    
    // 3. Check connection
    client.test_connection().await.map_err(|e| format!("Connection failed: {}", e))?;

    // 4. Download remote sync.json
    let remote_bytes = match client.download("sync.json").await {
        Ok(bytes) => Some(bytes),
        Err(_) => None, // File might not exist yet
    };

    let sync_path = state.config_manager.sync_config_path();
    let meta_path = sync_path.with_extension("meta");
    let mut metadata = SyncMetadata::load(&meta_path).unwrap_or(SyncMetadata::new());

    // 5. Conflict detection logic
    if let Some(remote_content) = &remote_bytes {
        // If local file exists, check for conflict
        if sync_path.exists() {
            let has_conflict = detect_conflict(&sync_path, remote_content).map_err(|e| e.to_string())?;
            
            if has_conflict {
                // Check if we already synced this exact version
                let remote_hash = crate::webdav::conflict::calculate_hash(remote_content);
                if remote_hash != metadata.last_sync_hash {
                    return Ok(SyncResult {
                        status: "conflict".to_string(),
                        message: "Remote changes detected".to_string(),
                        remote_content: Some(String::from_utf8_lossy(remote_content).to_string()),
                    });
                }
            }
        }
        
        // No conflict or first sync -> Write remote content to local file
        fs::write(&sync_path, remote_content).map_err(|e| e.to_string())?;
    }

    // 6. Upload local sync.json (push)
    if sync_path.exists() {
        let content = fs::read(&sync_path).map_err(|e| e.to_string())?;
        client.upload("sync.json", &content).await.map_err(|e| e.to_string())?;
        
        // Update metadata
        metadata.last_sync_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        metadata.last_sync_hash = crate::webdav::conflict::calculate_hash(&content);
        metadata.save(&meta_path).map_err(|e| e.to_string())?;
    }

    Ok(SyncResult {
        status: "success".to_string(),
        message: "Sync completed successfully".to_string(),
        remote_content: None,
    })
}
