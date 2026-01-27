use crate::ssh_manager::handler::ClientHandler;
use crate::config::types::Proxy;
use crate::commands::connection::JumphostConfig;
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
    static ref SESSIONS: Mutex<HashMap<String, (client::Handle<ClientHandler>, ChannelId, Option<client::Handle<ClientHandler>>)>> = Mutex::new(HashMap::new());
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
        proxy: Option<Proxy>,
        jumphost: Option<JumphostConfig>,
        tx: mpsc::Sender<(String, Vec<u8>)>, // Event channel for forwarding SSH data
    ) -> Result<String, String> {
        let mut config = client::Config::default();
        
        // Enhance RSA compatibility for modern OpenSSH servers
        config.preferred.key = std::borrow::Cow::Owned(vec![
            russh::keys::key::Name("ssh-ed25519"),
            russh::keys::key::Name("rsa-sha2-256"), 
            russh::keys::key::Name("rsa-sha2-512"),
            russh::keys::key::Name("ssh-rsa"),
        ]);

        let config = Arc::new(config);
        let session_id = uuid::Uuid::new_v4().to_string();
        let handler = ClientHandler::with_channel(session_id.clone(), tx.clone());

        info!("[SSH] Connecting to {}:{} as {}", host, port, username);

        let mut jh_handle_to_store = None;

        let mut session = if let Some(p) = &proxy {
            info!("[SSH] Using {} proxy: {}:{}", p.proxy_type, p.host, p.port);
            let _ = tx.send((session_id.clone(), format!("Connecting via {} proxy {}:{}...\r\n", p.proxy_type, p.host, p.port).as_bytes().to_vec())).await;
            if p.proxy_type == "socks5" {
                use tokio_socks::tcp::Socks5Stream;
                let has_auth = p.username.as_ref().map(|u| !u.is_empty()).unwrap_or(false);
                let stream = if has_auth {
                    let u = p.username.as_ref().unwrap();
                    let p_auth = p.password.as_ref().map(|s| s.as_str()).unwrap_or("");
                    Socks5Stream::connect_with_password((p.host.as_str(), p.port), (host, port), u, p_auth).await
                        .map_err(|e| format!("SOCKS5 proxy error: {}", e))?
                } else {
                    Socks5Stream::connect((p.host.as_str(), p.port), (host, port)).await
                        .map_err(|e| format!("SOCKS5 proxy error: {}", e))?
                };
                client::connect_stream(config, stream, handler).await
            } else if p.proxy_type == "http" {
                // Basic HTTP CONNECT implementation
                use tokio::io::{AsyncWriteExt, AsyncReadExt};
                use base64::prelude::*;
                let mut stream = tokio::net::TcpStream::connect((p.host.as_str(), p.port)).await
                    .map_err(|e| format!("Failed to connect to HTTP proxy: {}", e))?;
                
                let mut auth_header = String::new();
                if let Some(u) = &p.username {
                    if !u.is_empty() {
                        let p_auth = p.password.as_ref().map(|s| s.as_str()).unwrap_or("");
                        let auth_str = format!("{}:{}", u, p_auth);
                        let encoded = BASE64_STANDARD.encode(auth_str);
                        auth_header = format!("\r\nProxy-Authorization: Basic {}", encoded);
                    }
                }


                let connect_req = format!(
                    "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{} {}\r\n\r\n",
                    host, port, host, port, auth_header
                );
                stream.write_all(connect_req.as_bytes()).await.map_err(|e| e.to_string())?;
                
                let mut response = [0u8; 4096];
                let n = stream.read(&mut response).await.map_err(|e| e.to_string())?;
                let response_text = String::from_utf8_lossy(&response[..n]);
                if !response_text.contains("200 Connection established") && !response_text.contains("200 OK") {
                    return Err(format!("HTTP proxy returned error: {}", response_text));
                }
                client::connect_stream(config, stream, handler).await
            } else {
                return Err(format!("Unsupported proxy type: {}", p.proxy_type));
            }
        } else if let Some(j) = &jumphost {
            info!("[SSH] Using jumphost: {}:{} as {}", j.host, j.port, j.username);
            let _ = tx.send((session_id.clone(), format!("Connecting to jump host {}:{}...\r\n", j.host, j.port).as_bytes().to_vec())).await;
            
            // 1. Connect to jumphost
            let jh_handler = ClientHandler::new();
            let mut jh_session = client::connect(config.clone(), (j.host.as_str(), j.port), jh_handler).await
                .map_err(|e| format!("Failed to connect to jumphost: {}", e))?;
            
            // 2. Authenticate jumphost
            let _ = tx.send((session_id.clone(), "Authenticating jump host...\r\n".as_bytes().to_vec())).await;
            Self::authenticate_session(
                &mut jh_session,
                &j.username,
                j.password.clone(),
                j.private_key.clone(),
                j.passphrase.clone(),
            ).await.map_err(|e| format!("Jumphost authentication failed: {}", e))?;
            
            // 3. Open direct-tcpip channel to target
            let _ = tx.send((session_id.clone(), format!("Tunneling to {}:{}...\r\n", host, port).as_bytes().to_vec())).await;
            let channel = jh_session.channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 22222).await
                .map_err(|e| format!("Failed to open direct-tcpip through jumphost: {}", e))?;
            
            jh_handle_to_store = Some(jh_session);

            // 4. Connect to target through channel
            client::connect_stream(config, channel.into_stream(), handler).await
        } else {
            client::connect(config, (host, port), handler).await
        }
        .map_err(|e| {
            error!("[SSH] Connection error: {}", e);
            e.to_string()
        })?;

        let _ = tx.send((session_id.clone(), format!("Authenticating as {}...\r\n", username).as_bytes().to_vec())).await;
        Self::authenticate_session(
            &mut session,
            username,
            password,
            private_key,
            passphrase,
        ).await?;

        info!("[SSH] Authentication successful. Opening channel...");

        let channel = session.channel_open_session().await.map_err(|e| e.to_string())?;
        
        // Request PTY and wait for reply to ensure it's granted
        channel.request_pty(true, "xterm", 80, 24, 0, 0, &[]).await.map_err(|e| format!("PTY request failed: {}", e))?;
        
        // Request shell and wait for reply
        channel.request_shell(true).await.map_err(|e| format!("Shell request failed: {}", e))?;

        let channel_id = channel.id();
        {
            let mut sessions = SESSIONS.lock().await;
            sessions.insert(session_id.clone(), (session, channel_id, jh_handle_to_store));
        }
        
        Ok(session_id)
    }

    async fn authenticate_session<H: client::Handler>(
        session: &mut client::Handle<H>,
        username: &str,
        password: Option<String>,
        private_key: Option<String>,
        passphrase: Option<String>,
    ) -> Result<(), String> {
        let mut authenticated = false;

        if let Some(key_content) = private_key {
            info!("[SSH] Attempting publickey auth for user: '{}'", username);
            let key = russh_keys::decode_secret_key(&key_content, passphrase.as_deref())
                .map_err(|e| format!("Failed to decode private key: {}", e))?;
            
            let key_pair = Arc::new(key);
            match session.authenticate_publickey(username, key_pair).await {
                Ok(true) => {
                    authenticated = true;
                    info!("[SSH] Publickey authentication successful.");
                },
                Ok(false) => warn!("[SSH] Publickey authentication rejected."),
                Err(e) => error!("[SSH] Publickey authentication error: {}", e),
            }
        }

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

        Ok(())
    }

    pub async fn send_input(session_id: &str, data: &[u8]) -> Result<(), String> {
        let mut sessions = SESSIONS.lock().await;
        if let Some((session, channel_id, _)) = sessions.get_mut(session_id) {
            session.data(*channel_id, data.to_vec().into()).await.map_err(|_| "Failed to send data".to_string())?;
            Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }

    pub async fn resize(_session_id: &str, _cols: u32, _rows: u32) -> Result<(), String> {
        Ok(())
    }
    
    pub async fn disconnect(session_id: &str) -> Result<(), String> {
        let mut sessions = SESSIONS.lock().await;
        if let Some(_) = sessions.remove(session_id) {
             Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }
}
