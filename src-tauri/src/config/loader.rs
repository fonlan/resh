// src-tauri/src/config/loader.rs

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
    fn test_merge_configs() {
        let sync = Config {
            version: "1.0".to_string(),
            servers: vec![
                Server {
                    id: "server1".to_string(),
                    name: "Server 1".to_string(),
                    host: "example.com".to_string(),
                    port: 22,
                    username: "user".to_string(),
                    auth_id: None,
                    proxy_id: None,
                    jumphost_id: None,
                    port_forwards: vec![],
                    keep_alive: 0,
                    auto_exec_commands: vec![],
                    env_vars: std::collections::HashMap::new(),
                },
                Server {
                    id: "server2".to_string(),
                    name: "Server 2".to_string(),
                    host: "example2.com".to_string(),
                    port: 22,
                    username: "user".to_string(),
                    auth_id: None,
                    proxy_id: None,
                    jumphost_id: None,
                    port_forwards: vec![],
                    keep_alive: 0,
                    auto_exec_commands: vec![],
                    env_vars: std::collections::HashMap::new(),
                },
            ],
            authentications: vec![
                Authentication {
                    id: "auth1".to_string(),
                    name: "Auth 1".to_string(),
                    auth_type: "key".to_string(),
                    key_content: Some("key_content".to_string()),
                    passphrase: None,
                    username: None,
                    password: None,
                },
            ],
            proxies: vec![],
            general: Config::empty().general,
        };

        let local = Config {
            version: "1.0".to_string(),
            servers: vec![
                Server {
                    id: "server1".to_string(),
                    name: "Server 1 Updated".to_string(),
                    host: "updated.com".to_string(),
                    port: 2222,
                    username: "updated_user".to_string(),
                    auth_id: None,
                    proxy_id: None,
                    jumphost_id: None,
                    port_forwards: vec![],
                    keep_alive: 0,
                    auto_exec_commands: vec![],
                    env_vars: std::collections::HashMap::new(),
                },
                Server {
                    id: "server3".to_string(),
                    name: "Server 3".to_string(),
                    host: "example3.com".to_string(),
                    port: 22,
                    username: "user".to_string(),
                    auth_id: None,
                    proxy_id: None,
                    jumphost_id: None,
                    port_forwards: vec![],
                    keep_alive: 0,
                    auto_exec_commands: vec![],
                    env_vars: std::collections::HashMap::new(),
                },
            ],
            authentications: vec![],
            proxies: vec![],
            general: Config::empty().general,
        };

        let temp_dir = TempDir::new().unwrap();
        let manager = ConfigManager::new(temp_dir.path().to_path_buf());

        let merged = manager.merge_configs(sync, local);

        assert_eq!(merged.servers.len(), 3);

        let server1 = merged.servers.iter().find(|s| s.id == "server1").unwrap();
        assert_eq!(server1.name, "Server 1 Updated");
        assert_eq!(server1.host, "updated.com");
        assert_eq!(server1.port, 2222);

        let server2 = merged.servers.iter().find(|s| s.id == "server2").unwrap();
        assert_eq!(server2.name, "Server 2");
        assert_eq!(server2.host, "example2.com");

        let server3 = merged.servers.iter().find(|s| s.id == "server3").unwrap();
        assert_eq!(server3.name, "Server 3");

        assert_eq!(merged.authentications.len(), 1);
        assert_eq!(merged.authentications[0].id, "auth1");
    }

    #[test]
    fn test_config_manager_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let app_data_dir = temp_dir.path().join("nonexistent");

        assert!(!app_data_dir.exists());

        let _manager = ConfigManager::new(app_data_dir.clone());

        assert!(app_data_dir.exists());
    }

    #[test]
    fn test_load_empty_configs() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ConfigManager::new(temp_dir.path().to_path_buf());

        let sync = manager.load_sync_config().unwrap();
        let local = manager.load_local_config().unwrap();

        assert!(sync.servers.is_empty());
        assert!(local.authentications.is_empty());
    }
}
