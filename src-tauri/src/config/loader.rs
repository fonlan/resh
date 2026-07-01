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
            tracing::info!("normalized persisted local config defaults");
        }
        Ok(config)
    }

    pub fn save_config(&self, config: &Config, path: &Path) -> Result<(), String> {
        let mut normalized = config.clone();
        normalized.normalize_legacy_defaults();
        let json = serde_json::to_string_pretty(&normalized)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        fs::write(path, json).map_err(|e| format!("Failed to write config: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::Server;

    fn sample_server() -> Server {
        Server {
            id: "srv-unicode".to_string(),
            name: "生产-emoji-😀".to_string(),
            group: "默认/ops".to_string(),
            host: "例子.test".to_string(),
            port: 22,
            username: "alice".to_string(),
            auth_id: None,
            proxy_id: None,
            jumphost_id: None,
            port_forwards: vec![],
            keep_alive: 30,
            auto_exec_commands: vec!["printf 'ok'".to_string()],
            snippets: vec![],
            ai_models: vec![],
            sftp_custom_commands: vec![],
            sftp_favorite_paths: vec!["/var/log/服务".to_string()],
            additional_prompt: None,
            synced: true,
            created_at: None,
            updated_at: "2026-06-30T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn local_config_path_uses_platform_join() {
        let temp_dir = tempfile::tempdir().unwrap();
        let app_data_dir = temp_dir.path().join("Application Support").join("Resh");
        let manager = ConfigManager::new(app_data_dir.clone());

        assert_eq!(manager.local_config_path(), app_data_dir.join("local.json"));
    }

    #[test]
    fn saves_and_loads_config_under_unicode_app_data_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let app_data_dir = temp_dir.path().join("Resh 配置 😀");
        let manager = ConfigManager::new(app_data_dir);

        let mut config = Config::empty();
        config.servers.push(sample_server());
        config.general.sftp.default_download_path = "/Users/alice/Downloads/远程 文件".to_string();

        manager.save_local_config(&config).unwrap();
        let loaded = manager.load_local_config().unwrap();

        assert_eq!(loaded.servers[0].name, "生产-emoji-😀");
        assert_eq!(loaded.servers[0].sftp_favorite_paths[0], "/var/log/服务");
        assert_eq!(
            loaded.general.sftp.default_download_path,
            "/Users/alice/Downloads/远程 文件"
        );
    }

    #[test]
    fn loads_config_with_windows_style_paths_without_rewriting_them() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::new(temp_dir.path().join("Resh"));

        let mut config = Config::empty();
        config.general.sftp.default_download_path = r"C:\Users\alice\Downloads".to_string();
        config
            .general
            .sftp
            .editors
            .push(crate::config::types::EditorRule {
                id: "code".to_string(),
                pattern: "*.rs".to_string(),
                editor: r"C:\Program Files\Microsoft VS Code\Code.exe".to_string(),
            });

        manager.save_local_config(&config).unwrap();
        let loaded = manager.load_local_config().unwrap();

        assert_eq!(
            loaded.general.sftp.default_download_path,
            r"C:\Users\alice\Downloads"
        );
        assert_eq!(
            loaded.general.sftp.editors[0].editor,
            r"C:\Program Files\Microsoft VS Code\Code.exe"
        );
    }
    #[test]
    fn ai_tool_confirmation_countdown_defaults_and_clamps() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::new(temp_dir.path().join("Resh"));

        let mut missing_field = serde_json::to_value(Config::empty()).unwrap();
        missing_field["general"]
            .as_object_mut()
            .unwrap()
            .remove("aiToolConfirmationCountdown");
        std::fs::write(
            manager.local_config_path(),
            serde_json::to_vec_pretty(&missing_field).unwrap(),
        )
        .unwrap();

        let loaded = manager.load_local_config().unwrap();
        assert_eq!(loaded.general.ai_tool_confirmation_countdown, 5);

        let mut config = Config::empty();
        config.general.ai_tool_confirmation_countdown = 99;
        manager.save_local_config(&config).unwrap();

        let loaded = manager.load_local_config().unwrap();
        assert_eq!(loaded.general.ai_tool_confirmation_countdown, 30);

        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(manager.local_config_path()).unwrap()).unwrap();
        assert_eq!(
            raw["general"]["aiToolConfirmationCountdown"],
            serde_json::json!(30)
        );
    }
}
