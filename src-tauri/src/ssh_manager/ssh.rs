use crate::ssh_manager::handler::ClientHandler;
use crate::config::types::Proxy;
use russh::client;
use russh_keys;
use std::sync::Arc;
use tokio::sync::mpsc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use lazy_static::lazy_static;
use tracing::{info, error, warn};
use serde::{Deserialize, Serialize};
use base64::prelude::*;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
    pub proxy: Option<Proxy>,
    pub jumphost: Option<JumphostConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JumphostConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub passphrase: Option<String>,
}

struct SessionData {
    channel: russh::Channel<russh::client::Msg>,
    session: russh::client::Handle<ClientHandler>,
    jumphost_session: Option<russh::client::Handle<ClientHandler>>,
    config: ConnectParams,
    tx: mpsc::Sender<(String, Vec<u8>)>,
    cols: u32,
    rows: u32,
    terminal_buffer: String,
    command_recorder: Option<String>,
    last_output_len: usize,
}

lazy_static! {
    static ref SESSIONS: Mutex<HashMap<String, SessionData>> = Mutex::new(HashMap::new());
}

pub struct SSHClient;

impl SSHClient {
    pub async fn connect(
        params: ConnectParams,
        tx: mpsc::Sender<(String, Vec<u8>)>,
    ) -> Result<String, String> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let initial_cols = 80;
        let initial_rows = 24;
        
        let (channel, session, jh_session) = Self::establish_connection(
            session_id.clone(),
            &params,
            tx.clone(),
            initial_cols,
            initial_rows
        ).await?;

        {
            let mut sessions = SESSIONS.lock().await;
            sessions.insert(session_id.clone(), SessionData {
                channel,
                session,
                jumphost_session: jh_session,
                config: params,
                tx,
                cols: initial_cols,
                rows: initial_rows,
                terminal_buffer: String::new(),
                command_recorder: None,
                last_output_len: 0,
            });
        }
        
        Ok(session_id)
    }

    async fn establish_connection(
        session_id: String,
        params: &ConnectParams,
        tx: mpsc::Sender<(String, Vec<u8>)>,
        cols: u32,
        rows: u32,
    ) -> Result<(russh::Channel<russh::client::Msg>, russh::client::Handle<ClientHandler>, Option<russh::client::Handle<ClientHandler>>), String> {
        let mut config = client::Config::default();
        
        config.preferred.key = std::borrow::Cow::Owned(vec![
            russh::keys::key::Name("ssh-ed25519"),
            russh::keys::key::Name("rsa-sha2-256"), 
            russh::keys::key::Name("rsa-sha2-512"),
            russh::keys::key::Name("ssh-rsa"),
        ]);

        let config = Arc::new(config);
        let handler = ClientHandler::with_channel(session_id.clone(), tx.clone());

        info!("[SSH] Connecting to {}:{} as {}", params.host, params.port, params.username);

        let mut jh_handle_to_store = None;

        let mut session = if let Some(p) = &params.proxy {
            info!("[SSH] Using {} proxy: {}:{}", p.proxy_type, p.host, p.port);
            let _ = tx.send((session_id.clone(), format!("Connecting via {} proxy {}:{}...\r\n", p.proxy_type, p.host, p.port).as_bytes().to_vec())).await;
            if p.proxy_type == "socks5" {
                use tokio_socks::tcp::Socks5Stream;
                let has_auth = p.username.as_ref().map(|u| !u.is_empty()).unwrap_or(false);
                let stream = if has_auth {
                    let u = p.username.as_ref().unwrap();
                    let p_auth = p.password.as_ref().map(|s| s.as_str()).unwrap_or("");
                    Socks5Stream::connect_with_password((p.host.as_str(), p.port), (&params.host[..], params.port), u, p_auth).await
                        .map_err(|e| format!("SOCKS5 proxy error: {}", e))?
                } else {
                    Socks5Stream::connect((p.host.as_str(), p.port), (&params.host[..], params.port)).await
                        .map_err(|e| format!("SOCKS5 proxy error: {}", e))?
                };
                client::connect_stream(config, stream, handler).await
            } else if p.proxy_type == "http" {
                use tokio::io::{AsyncWriteExt, AsyncReadExt};
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
                    params.host, params.port, params.host, params.port, auth_header
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
        } else if let Some(j) = &params.jumphost {
            info!("[SSH] Using jumphost: {}:{} as {}", j.host, j.port, j.username);
            let _ = tx.send((session_id.clone(), "Connecting to jump host...\r\n".as_bytes().to_vec())).await;
            
            let jh_handler = ClientHandler::new();
            let mut jh_session = client::connect(config.clone(), (j.host.as_str(), j.port), jh_handler).await
                .map_err(|e| format!("Failed to connect to jumphost: {}", e))?;
            
            let _ = tx.send((session_id.clone(), "Authenticating jump host...\r\n".as_bytes().to_vec())).await;
            Self::authenticate_session(
                &mut jh_session,
                &j.username,
                j.password.clone(),
                j.private_key.clone(),
                j.passphrase.clone(),
            ).await.map_err(|e| format!("Jumphost authentication failed: {}", e))?;
            
            let _ = tx.send((session_id.clone(), format!("Tunneling to {}:{}...\r\n", params.host, params.port).as_bytes().to_vec())).await;
            let channel = jh_session.channel_open_direct_tcpip(&params.host, params.port as u32, "127.0.0.1", 22222).await
                .map_err(|e| format!("Failed to open direct-tcpip through jumphost: {}", e))?;
            
            jh_handle_to_store = Some(jh_session);

            client::connect_stream(config, channel.into_stream(), handler).await
        } else {
            client::connect(config, (&params.host[..], params.port), handler).await
        }
        .map_err(|e| {
            error!("[SSH] Connection error: {}", e);
            e.to_string()
        })?;

        let _ = tx.send((session_id.clone(), format!("Authenticating as {}...\r\n", params.username).as_bytes().to_vec())).await;
        Self::authenticate_session(
            &mut session,
            &params.username,
            params.password.clone(),
            params.private_key.clone(),
            params.passphrase.clone(),
        ).await?;

        info!("[SSH] Authentication successful. Opening channel...");

        let channel = session.channel_open_session().await.map_err(|e| e.to_string())?;
        
        channel.request_pty(true, "xterm-256color", cols, rows, 0, 0, &[]).await.map_err(|e| format!("PTY request failed: {}", e))?;
        channel.request_shell(true).await.map_err(|e| format!("Shell request failed: {}", e))?;

        Ok((channel, session, jh_handle_to_store))
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
        let result = {
            let mut sessions = SESSIONS.lock().await;
            if let Some(session_data) = sessions.get_mut(session_id) {
                session_data.channel.data(data).await
            } else {
                return Err("Session not found".to_string());
            }
        };

        if result.is_err() {
            info!("[SSH] Send failed, attempting to reconnect session {}...", session_id);
            if let Err(e) = Self::reconnect(session_id).await {
                error!("[SSH] Reconnection failed: {}", e);
                return Err(format!("Connection lost and reconnection failed: {}", e));
            }

            // Retry sending
            let mut sessions = SESSIONS.lock().await;
            if let Some(session_data) = sessions.get_mut(session_id) {
                session_data.channel.data(data).await.map_err(|e| format!("Failed to send data after reconnect: {}", e))?;
            } else {
                return Err("Session lost during reconnect".to_string());
            }
        }
        
        Ok(())
    }

    pub async fn reconnect(session_id: &str) -> Result<(), String> {
        let (config, tx, cols, rows) = {
            let sessions = SESSIONS.lock().await;
            if let Some(data) = sessions.get(session_id) {
                (data.config.clone(), data.tx.clone(), data.cols, data.rows)
            } else {
                return Err("Session not found".to_string());
            }
        };

        // Notify user about reconnection attempt
        let _ = tx.send((session_id.to_string(), "\r\n[Resh] Connection lost. Reconnecting...\r\n".as_bytes().to_vec())).await;

        match Self::establish_connection(session_id.to_string(), &config, tx.clone(), cols, rows).await {
            Ok((channel, session, jh_session)) => {
                let mut sessions = SESSIONS.lock().await;
                if let Some(data) = sessions.get_mut(session_id) {
                    data.channel = channel;
                    data.session = session;
                    data.jumphost_session = jh_session;
                    info!("[SSH] Reconnection successful for {}", session_id);
                    Ok(())
                } else {
                    Err("Session removed during reconnection".to_string())
                }
            },
            Err(e) => {
                let _ = tx.send((session_id.to_string(), format!("\r\n[Resh] Reconnection failed: {}\r\n", e).as_bytes().to_vec())).await;
                Err(e)
            }
        }
    }

    pub async fn resize(session_id: &str, cols: u32, rows: u32) -> Result<(), String> {
        let mut sessions = SESSIONS.lock().await;
        if let Some(session_data) = sessions.get_mut(session_id) {
            session_data.cols = cols;
            session_data.rows = rows;
            session_data.channel.window_change(cols, rows, 0, 0).await.map_err(|e| format!("Failed to resize: {}", e))?;
            Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }
    
    pub async fn disconnect(session_id: &str) -> Result<(), String> {
        let mut sessions = SESSIONS.lock().await;
        if let Some(_) = sessions.remove(session_id) {
             info!("[SSH] Session {} disconnected and removed.", session_id);
             Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }

    /// Get the current terminal buffer content (last 100KB)
    pub async fn get_terminal_output(session_id: &str) -> Result<String, String> {
        let sessions = SESSIONS.lock().await;
        if let Some(session_data) = sessions.get(session_id) {
            Ok(session_data.terminal_buffer.clone())
        } else {
            Err("Session not found".to_string())
        }
    }

    /// Start recording output for a single command
    pub async fn start_command_recording(session_id: &str) -> Result<(), String> {
        let mut sessions = SESSIONS.lock().await;
        if let Some(session_data) = sessions.get_mut(session_id) {
            session_data.command_recorder = Some(String::new());
            session_data.last_output_len = session_data.terminal_buffer.len();
            Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }

    /// Stop recording and get the recorded output since start_command_recording was called
    pub async fn stop_command_recording(session_id: &str) -> Result<String, String> {
        let mut sessions = SESSIONS.lock().await;
        if let Some(session_data) = sessions.get_mut(session_id) {
            let recorded = session_data.command_recorder.take().unwrap_or_default();
            Ok(recorded)
        } else {
            Err("Session not found".to_string())
        }
    }

    /// Check if currently recording a command
    pub async fn is_recording(session_id: &str) -> Result<bool, String> {
        let sessions = SESSIONS.lock().await;
        if let Some(session_data) = sessions.get(session_id) {
            Ok(session_data.command_recorder.is_some())
        } else {
            Err("Session not found".to_string())
        }
    }

    /// Update the terminal buffer with new data
    pub async fn update_terminal_buffer(session_id: &str, data: &str) -> Result<(), String> {
        const MAX_BUFFER_SIZE: usize = 100_000;
        
        let mut sessions = SESSIONS.lock().await;
        if let Some(session_data) = sessions.get_mut(session_id) {
            session_data.terminal_buffer.push_str(data);
            
            if session_data.terminal_buffer.len() > MAX_BUFFER_SIZE {
                let excess = session_data.terminal_buffer.len() - MAX_BUFFER_SIZE;
                session_data.terminal_buffer = session_data.terminal_buffer[excess..].to_string();
            }

            if let Some(recorder) = session_data.command_recorder.as_mut() {
                recorder.push_str(data);
            }
            
            Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }
}
