// src-tauri/src/config/types.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_id: Option<String>,
    pub proxy_id: Option<String>,
    pub jumphost_id: Option<String>,
    #[serde(default)]
    pub port_forwards: Vec<PortForward>,
    #[serde(default)]
    pub keep_alive: u32,
    #[serde(default)]
    pub auto_exec_commands: Vec<String>,
    #[serde(default)]
    pub env_vars: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub synced: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortForward {
    pub local: u16,
    pub remote: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Authentication {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub auth_type: String, // "key" or "password"
    pub key_content: Option<String>,
    pub passphrase: Option<String>,
    pub password: Option<String>,
    #[serde(default = "default_true")]
    pub synced: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proxy {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub proxy_type: String, // "http" or "socks5"
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default = "default_true")]
    pub synced: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: String,
    pub name: String,
    pub content: String,
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub synced: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSettings {
    pub font_family: String,
    pub font_size: u32,
    pub cursor_style: String,
    pub scrollback: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDAVSettings {
    pub url: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub width: f64,
    pub height: f64,
    pub is_maximized: bool,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettings {
    pub theme: String,
    pub language: String,
    pub terminal: TerminalSettings,
    pub webdav: WebDAVSettings,
    pub confirm_close_tab: bool,
    pub confirm_exit_app: bool,
    #[serde(default)]
    pub debug_enabled: bool,
    #[serde(default = "default_max_recent_servers")]
    pub max_recent_servers: u32,
    #[serde(default)]
    pub recent_server_ids: Vec<String>,
    #[serde(default = "default_window_state")]
    pub window_state: WindowState,
}

fn default_max_recent_servers() -> u32 {
    3
}

fn default_window_state() -> WindowState {
    WindowState {
        width: 1200.0,
        height: 800.0,
        is_maximized: false,
        x: 100.0,
        y: 100.0,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub version: String,
    pub servers: Vec<Server>,
    pub authentications: Vec<Authentication>,
    pub proxies: Vec<Proxy>,
    #[serde(default)]
    pub snippets: Vec<Snippet>,
    pub general: GeneralSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConfig {
    pub version: String,
    #[serde(default)]
    pub servers: Vec<Server>,
    #[serde(default)]
    pub authentications: Vec<Authentication>,
    #[serde(default)]
    pub proxies: Vec<Proxy>,
    #[serde(default)]
    pub snippets: Vec<Snippet>,
    #[serde(default)]
    pub removed_ids: Vec<String>,
}

impl Config {
    pub fn empty() -> Self {
        Self {
            version: "1.0".to_string(),
            servers: vec![],
            authentications: vec![],
            proxies: vec![],
            snippets: vec![],
            general: GeneralSettings {
                theme: "dark".to_string(),
                language: "en".to_string(),
                terminal: TerminalSettings {
                    font_family: "Consolas".to_string(),
                    font_size: 14,
                    cursor_style: "block".to_string(),
                    scrollback: 5000,
                },
                webdav: WebDAVSettings {
                    url: String::new(),
                    username: String::new(),
                    password: String::new(),
                    enabled: false,
                },
                confirm_close_tab: true,
                confirm_exit_app: true,
                debug_enabled: false,
                max_recent_servers: 3,
                recent_server_ids: vec![],
                window_state: default_window_state(),
            },
        }
    }
}
