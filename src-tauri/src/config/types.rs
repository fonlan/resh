use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: String,
    pub profiles: HashMap<String, Profile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMethod {
    #[serde(rename = "type")]
    pub type_: String,
    pub data: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub timeout: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedConfig {
    pub salt: String,
    pub nonce: String,
    pub ciphertext: String,
}

impl Config {
    pub fn empty() -> Self {
        Config {
            version: "1.0.0".to_string(),
            profiles: HashMap::new(),
        }
    }
}
