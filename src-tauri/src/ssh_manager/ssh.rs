use crate::ssh_manager::handler::ClientHandler;
use russh::client;
use russh::ChannelId;
use russh_keys;
use std::sync::Arc;
use tokio::sync::mpsc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use lazy_static::lazy_static;
use tracing::{info, error, warn};

lazy_static! {
    static ref SESSIONS: Mutex<HashMap<String, (client::Handle<ClientHandler>, ChannelId)>> = Mutex::new(HashMap::new());
}

pub struct SSHClient;

impl SSHClient {
    pub async fn connect(
        host: &str,
        port: u16,
        username: &str,
        password: Option<String>,
        private_key: Option<String>,
        passphrase: Option<String>,
        tx: mpsc::Sender<(String, Vec<u8>)>, // Event channel for forwarding SSH data
    ) -> Result<String, String> {
        let mut config = client::Config::default();
        
        // Enhance RSA compatibility for modern OpenSSH servers
        config.preferred.key = std::borrow::Cow::Owned(vec![
            russh::keys::key::Name("ssh-ed25519"),
            russh::keys::key::Name("rsa-sha2-256"), // Try 256 before 512 for better compatibility
            russh::keys::key::Name("rsa-sha2-512"),
            russh::keys::key::Name("ssh-rsa"),
        ]);

        let config = Arc::new(config);

        let session_id = uuid::Uuid::new_v4().to_string();

        // Create handler with session_id and channel
        let handler = ClientHandler::with_channel(session_id.clone(), tx);

        info!("[SSH] Connecting to {}:{} as {}", host, port, username);

        // Connect
        let mut session = client::connect(config, (host, port), handler)
            .await
            .map_err(|e| {
                error!("[SSH] Connection error: {}", e);
                e.to_string()
            })?;

        let mut authenticated = false;

        // Try publickey auth if private key is provided
        if let Some(key_content) = private_key {
            info!("[SSH] Attempting publickey auth for user: '{}'", username);
            let key = russh_keys::decode_secret_key(&key_content, passphrase.as_deref())
                .map_err(|e| {
                    let err = format!("Failed to decode private key: {}", e);
                    error!("[SSH] {}", err);
                    err
                })?;
            
            info!("[SSH] Decoded key type: {:?}", key.name());
            let key_pair = Arc::new(key);
            
            match session.authenticate_publickey(username, key_pair).await {
                Ok(true) => {
                    authenticated = true;
                    info!("[SSH] Publickey authentication successful.");
                },
                Ok(false) => {
                    warn!("[SSH] Publickey authentication rejected by server.");
                },
                Err(e) => {
                    error!("[SSH] Publickey authentication error: {}", e);
                }
            }
        }

        // Try password auth if not authenticated and password is provided
        if !authenticated {
            if let Some(pwd) = password {
                info!("[SSH] Attempting password auth...");
                if session.authenticate_password(username, &pwd).await.map_err(|e| e.to_string())? {
                    authenticated = true;
                    info!("[SSH] Password authentication successful.");
                } else {
                    warn!("[SSH] Password authentication failed.");
                }
            }
        }

        if !authenticated {
            return Err("Authentication failed".to_string());
        }

        info!("[SSH] Authentication successful. Opening channel...");

        // Open channel
        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| e.to_string())?;

        // Request PTY
        channel
            .request_pty(false, "xterm", 80, 24, 0, 0, &[])
            .await
            .map_err(|e| e.to_string())?;

        // Start shell
        channel
            .request_shell(true)
            .await
            .map_err(|e| e.to_string())?;

        let channel_id = channel.id();

        // Store session and channel ID
        {
            let mut sessions = SESSIONS.lock().await;
            sessions.insert(session_id.clone(), (session, channel_id));
        }
        
        Ok(session_id)
    }

    pub async fn send_input(session_id: &str, data: &[u8]) -> Result<(), String> {
        let mut sessions = SESSIONS.lock().await;
        if let Some((session, channel_id)) = sessions.get_mut(session_id) {
            session.data(*channel_id, data.to_vec().into()).await.map_err(|_| "Failed to send data".to_string())?;
            Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }

    pub async fn resize(_session_id: &str, _cols: u32, _rows: u32) -> Result<(), String> {
        // TODO: Implement window_change when handler is fixed
        Ok(())
    }
    
    pub async fn disconnect(session_id: &str) -> Result<(), String> {
        let mut sessions = SESSIONS.lock().await;
        if let Some((_session, _channel_id)) = sessions.remove(session_id) {
             Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }
}
