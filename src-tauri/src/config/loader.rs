// src-tauri/src/config/loader.rs

use crate::config::types::Config;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct ConfigManager {
    app_data_dir: PathBuf,
}

impl ConfigManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&app_data_dir);
        ConfigManager { app_data_dir }
    }

    pub fn local_config_path(&self) -> PathBuf {
        self.app_data_dir.join("local.json")
    }

    pub fn save_local_config(&self, config: &Config) -> Result<(), String> {
        let path = self.local_config_path();
        self.save_config(config, &path)
    }

    pub fn load_local_config(&self) -> Result<Config, String> {
        let path = self.local_config_path();
        if !path.exists() {
            return Ok(Config::empty());
        }

        let content =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read local config: {}", e))?;
        let mut config: Config = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse local config: {}", e))?;
        if config.normalize_legacy_defaults() {
            tracing::info!(
                transfer_profile = %config.general.sftp.transfer_profile,
                migrated_download_max_inflight = config.general.sftp.download_max_inflight,
                migrated_chunk_size_min = config.general.sftp.chunk_size_min,
                "normalized legacy SFTP throughput defaults from persisted local config"
            );
        }
        Ok(config)
    }

    pub fn save_config(&self, config: &Config, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        fs::write(path, json).map_err(|e| format!("Failed to write config: {}", e))
    }
}
