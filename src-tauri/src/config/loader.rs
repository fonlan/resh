// src-tauri/src/config/loader.rs

use crate::config::encryption::{decrypt, encrypt, EncryptedData};
use crate::config::types::Config;
use std::fs;
use std::path::{Path, PathBuf};

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

    pub fn local_config_encrypted_path(&self) -> PathBuf {
        self.app_data_dir.join("local.enc.json")
    }

    pub fn load_local_config(&self) -> Result<Config, String> {
        let path = self.local_config_path();
        if !path.exists() {
            return Ok(Config::empty());
        }

        let content =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read local config: {}", e))?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse local config: {}", e))
    }

    pub fn load_encrypted_local_config(&self, password: &str) -> Result<Config, String> {
        let path = self.local_config_encrypted_path();
        if !path.exists() {
            return Ok(Config::empty());
        }

        let encrypted_json = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read encrypted local config: {}", e))?;
        let encrypted: EncryptedData = serde_json::from_str(&encrypted_json)
            .map_err(|e| format!("Failed to parse encrypted data: {}", e))?;
        let json_bytes = decrypt(&encrypted, password)
            .map_err(|e| format!("Failed to decrypt local config: {}", e))?;
        let config: Config = serde_json::from_slice(&json_bytes)
            .map_err(|e| format!("Failed to parse decrypted config: {}", e))?;

        Ok(config)
    }

    pub fn save_encrypted_local_config(
        &self,
        config: &Config,
        password: &str,
    ) -> Result<(), String> {
        let path = self.local_config_encrypted_path();
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        let encrypted = encrypt(json.as_bytes(), password)
            .map_err(|e| format!("Failed to encrypt config: {}", e))?;

        let encrypted_json = serde_json::to_string(&encrypted)
            .map_err(|e| format!("Failed to serialize encrypted data: {}", e))?;
        fs::write(&path, encrypted_json)
            .map_err(|e| format!("Failed to write encrypted config: {}", e))?;

        Ok(())
    }

    pub fn save_config(&self, config: &Config, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        fs::write(path, json).map_err(|e| format!("Failed to write config: {}", e))
    }
}
