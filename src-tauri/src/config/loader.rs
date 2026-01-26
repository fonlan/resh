// src-tauri/src/config/loader.rs

use crate::config::encryption::{decrypt, encrypt, EncryptedData};
use crate::config::types::{Authentication, Config, Proxy, Server};
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

    pub fn sync_config_path(&self) -> PathBuf {
        self.app_data_dir.join("sync.json")
    }

    pub fn local_config_path(&self) -> PathBuf {
        self.app_data_dir.join("local.json")
    }

    #[allow(dead_code)]
    pub fn sync_config_encrypted_path(&self) -> PathBuf {
        self.app_data_dir.join("sync.enc.json")
    }

    #[allow(dead_code)]
    pub fn local_config_encrypted_path(&self) -> PathBuf {
        self.app_data_dir.join("local.enc.json")
    }

    pub fn load_sync_config(&self) -> Result<Config, String> {
        let path = self.sync_config_path();
        if !path.exists() {
            return Ok(Config::empty());
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read sync config: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse sync config: {}", e))
    }

    pub fn load_local_config(&self) -> Result<Config, String> {
        let path = self.local_config_path();
        if !path.exists() {
            return Ok(Config::empty());
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read local config: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse local config: {}", e))
    }

    #[allow(dead_code)]
    pub fn load_encrypted_sync_config(&self, password: &str) -> Result<Config, String> {
        let path = self.sync_config_encrypted_path();
        if !path.exists() {
            return Ok(Config::empty());
        }

        let encrypted_json = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read encrypted sync config: {}", e))?;
        let encrypted: EncryptedData = serde_json::from_str(&encrypted_json)
            .map_err(|e| format!("Failed to parse encrypted data: {}", e))?;
        let json_bytes = decrypt(&encrypted, password)
            .map_err(|e| format!("Failed to decrypt sync config: {}", e))?;
        let config: Config = serde_json::from_slice(&json_bytes)
            .map_err(|e| format!("Failed to parse decrypted config: {}", e))?;

        Ok(config)
    }

    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub fn save_encrypted_sync_config(
        &self,
        config: &Config,
        password: &str,
    ) -> Result<(), String> {
        let path = self.sync_config_encrypted_path();
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

    #[allow(dead_code)]
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

    pub fn merge_configs(&self, sync: Config, local: Config) -> Config {
        let mut servers_map = std::collections::HashMap::new();
        for server in sync.servers {
            servers_map.insert(server.id.clone(), server);
        }
        for server in local.servers {
            servers_map.insert(server.id.clone(), server);
        }
        let mut servers: Vec<Server> = servers_map.into_values().collect();
        servers.sort_by(|a, b| a.id.cmp(&b.id));

        let mut auths_map = std::collections::HashMap::new();
        for auth in sync.authentications {
            auths_map.insert(auth.id.clone(), auth);
        }
        for auth in local.authentications {
            auths_map.insert(auth.id.clone(), auth);
        }
        let mut authentications: Vec<Authentication> =
            auths_map.into_values().collect();
        authentications.sort_by(|a, b| a.id.cmp(&b.id));

        let mut proxies_map = std::collections::HashMap::new();
        for proxy in sync.proxies {
            proxies_map.insert(proxy.id.clone(), proxy);
        }
        for proxy in local.proxies {
            proxies_map.insert(proxy.id.clone(), proxy);
        }
        let mut proxies: Vec<Proxy> = proxies_map.into_values().collect();
        proxies.sort_by(|a, b| a.id.cmp(&b.id));

        let general = if local.general.theme != "dark"
            || local.general.language != "en"
        {
            local.general
        } else {
            sync.general
        };

        Config {
            version: sync.version,
            servers,
            authentications,
            proxies,
            general,
        }
    }

    pub fn save_config(&self, config: &Config, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        fs::write(path, json)
            .map_err(|e| format!("Failed to write config: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_save_and_load_encrypted_sync_config() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ConfigManager::new(temp_dir.path().to_path_buf());

        let config = Config {
            version: "1.0".to_string(),
            servers: vec![],
            authentications: vec![],
            proxies: vec![],
            general: Config::empty().general,
        };

        let password = "test_password";

        manager
            .save_encrypted_sync_config(&config, password)
            .expect("Failed to save encrypted config");

        let loaded = manager
            .load_encrypted_sync_config(password)
            .expect("Failed to load encrypted config");

        assert_eq!(loaded.servers.len(), 0);
    }

    #[test]
    fn test_load_encrypted_config_wrong_password_fails() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ConfigManager::new(temp_dir.path().to_path_buf());

        let config = Config {
            version: "1.0".to_string(),
            servers: vec![],
            authentications: vec![],
            proxies: vec![],
            general: Config::empty().general,
        };

        let password = "correct_password";

        manager
            .save_encrypted_sync_config(&config, password)
            .expect("Failed to save encrypted config");

        let result = manager.load_encrypted_sync_config("wrong_password");

        assert!(result.is_err());
    }

    #[test]
    fn test_save_and_load_encrypted_local_config() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ConfigManager::new(temp_dir.path().to_path_buf());

        let config = Config {
            version: "1.0".to_string(),
            servers: vec![],
            authentications: vec![],
            proxies: vec![],
            general: Config::empty().general,
        };

        let password = "test_password";

        manager
            .save_encrypted_local_config(&config, password)
            .expect("Failed to save encrypted config");

        let loaded = manager
            .load_encrypted_local_config(password)
            .expect("Failed to load encrypted config");

        assert_eq!(loaded.authentications.len(), 0);
    }
}
