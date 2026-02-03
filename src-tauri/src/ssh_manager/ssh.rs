use crate::config::types::Proxy;
use crate::ssh_manager::handler::ClientHandler;
use base64::prelude::*;
use lazy_static::lazy_static;
use russh::client;
use russh_keys;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::lookup_host;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub distro: String,
    pub username: String,
    pub ip: String,
    pub shell: String,
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
    recording_prompt: Option<String>,
    command_finished: bool,
    last_exit_code: Option<i32>,
    pub system_info: Option<SystemInfo>,
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
            initial_rows,
        )
        .await?;

        {
            let mut sessions = SESSIONS.lock().await;
            sessions.insert(
                session_id.clone(),
                SessionData {
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
                    recording_prompt: None,
                    command_finished: false,
                    last_exit_code: None,
                    system_info: None,
                },
            );
        }

        Ok(session_id)
    }

    async fn establish_connection(
        session_id: String,
        params: &ConnectParams,
        tx: mpsc::Sender<(String, Vec<u8>)>,
        cols: u32,
        rows: u32,
    ) -> Result<
        (
            russh::Channel<russh::client::Msg>,
            russh::client::Handle<ClientHandler>,
            Option<russh::client::Handle<ClientHandler>>,
        ),
        String,
    > {
        let mut config = client::Config::default();

        config.preferred.key = std::borrow::Cow::Owned(vec![
            russh::keys::key::Name("ssh-ed25519"),
            russh::keys::key::Name("rsa-sha2-256"),
            russh::keys::key::Name("rsa-sha2-512"),
            russh::keys::key::Name("ssh-rsa"),
        ]);

        let config = Arc::new(config);
        let handler = ClientHandler::with_channel(session_id.clone(), tx.clone());

        info!(
            "[SSH] Connecting to {}:{} as {}",
            params.host, params.port, params.username
        );

        // Debug: Log proxy configuration
        if let Some(ref p) = params.proxy {
            info!(
                "[SSH] Proxy configured: type={}, host={}, port={}",
                p.proxy_type, p.host, p.port
            );
        } else {
            info!("[SSH] No proxy configured (proxy=None)");
        }

        // Debug: Log jumphost configuration
        if let Some(ref j) = params.jumphost {
            info!(
                "[SSH] Jumphost configured: {}:{} as {}",
                j.host, j.port, j.username
            );
        } else {
            info!("[SSH] No jumphost configured (jumphost=None)");
        }

        let mut jh_handle_to_store = None;

        let mut session = if let Some(p) = &params.proxy {
            info!("[SSH] Using {} proxy: {}:{}", p.proxy_type, p.host, p.port);
            let _ = tx
                .send((
                    session_id.clone(),
                    format!(
                        "Connecting via {} proxy {}:{}...\r\n",
                        p.proxy_type, p.host, p.port
                    )
                    .as_bytes()
                    .to_vec(),
                ))
                .await;
            if p.proxy_type == "socks5" {
                use tokio_socks::tcp::Socks5Stream;
                let has_auth = p.username.as_ref().map(|u| !u.is_empty()).unwrap_or(false);
                let stream = if has_auth {
                    let u = p.username.as_ref().unwrap();
                    let p_auth = p.password.as_ref().map(|s| s.as_str()).unwrap_or("");
                    Socks5Stream::connect_with_password(
                        (p.host.as_str(), p.port),
                        (&params.host[..], params.port),
                        u,
                        p_auth,
                    )
                    .await
                    .map_err(|e| format!("SOCKS5 proxy error: {}", e))?
                } else {
                    Socks5Stream::connect(
                        (p.host.as_str(), p.port),
                        (&params.host[..], params.port),
                    )
                    .await
                    .map_err(|e| format!("SOCKS5 proxy error: {}", e))?
                };
                client::connect_stream(config, stream, handler).await
            } else if p.proxy_type == "http" {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut stream = tokio::net::TcpStream::connect((p.host.as_str(), p.port))
                    .await
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
                stream
                    .write_all(connect_req.as_bytes())
                    .await
                    .map_err(|e| e.to_string())?;

                let mut response = [0u8; 4096];
                let n = stream
                    .read(&mut response)
                    .await
                    .map_err(|e| e.to_string())?;
                let response_text = String::from_utf8_lossy(&response[..n]);
                if !response_text.contains("200 Connection established")
                    && !response_text.contains("200 OK")
                {
                    return Err(format!("HTTP proxy returned error: {}", response_text));
                }
                client::connect_stream(config, stream, handler).await
            } else {
                return Err(format!("Unsupported proxy type: {}", p.proxy_type));
            }
        } else if let Some(j) = &params.jumphost {
            info!(
                "[SSH] Using jumphost: {}:{} as {}",
                j.host, j.port, j.username
            );
            let _ = tx
                .send((
                    session_id.clone(),
                    "Connecting to jump host...\r\n".as_bytes().to_vec(),
                ))
                .await;

            let jh_handler = ClientHandler::new();

            // Connect to jumphost - use proxy if configured, otherwise direct
            let mut jh_session = if let Some(p) = &params.proxy {
                info!(
                    "[SSH] Connecting to jumphost via {} proxy: {}:{} -> {}:{}",
                    p.proxy_type, p.host, p.port, j.host, j.port
                );
                info!("[SSH] Target server: {}:{}", params.host, params.port);
                let _ = tx
                    .send((
                        session_id.clone(),
                        format!(
                            "Connecting via {} proxy {}:{} to jumphost {}:{}...\r\n",
                            p.proxy_type, p.host, p.port, j.host, j.port
                        )
                        .as_bytes()
                        .to_vec(),
                    ))
                    .await;

                if p.proxy_type == "socks5" {
                    info!("[SSH] Attempting SOCKS5 connection to jumphost via proxy...");
                    use tokio_socks::tcp::Socks5Stream;
                    let has_auth = p.username.as_ref().map(|u| !u.is_empty()).unwrap_or(false);
                    info!(
                        "[SSH] SOCKS5 proxy auth: {}",
                        if has_auth { "yes" } else { "no" }
                    );

                    let stream = if has_auth {
                        let u = p.username.as_ref().unwrap();
                        let p_auth = p.password.as_ref().map(|s| s.as_str()).unwrap_or("");
                        info!(
                            "[SSH] SOCKS5: Connecting to proxy {}:{} with username {}",
                            p.host, p.port, u
                        );
                        Socks5Stream::connect_with_password(
                            (p.host.as_str(), p.port),
                            (&j.host[..], j.port),
                            u,
                            p_auth,
                        )
                        .await
                        .map_err(|e| {
                            error!("[SSH] SOCKS5 connect_with_password failed: {}", e);
                            format!("SOCKS5 proxy error when connecting to jumphost: {}", e)
                        })?
                    } else {
                        info!(
                            "[SSH] SOCKS5: Connecting to proxy {}:{} (no auth)",
                            p.host, p.port
                        );
                        Socks5Stream::connect((p.host.as_str(), p.port), (&j.host[..], j.port))
                            .await
                            .map_err(|e| {
                                error!("[SSH] SOCKS5 connect failed: {}", e);
                                format!("SOCKS5 proxy error when connecting to jumphost: {}", e)
                            })?
                    };
                    info!("[SSH] SOCKS5 stream established, now connecting SSH...");
                    client::connect_stream(config.clone(), stream, jh_handler)
                        .await
                        .map_err(|e| {
                            error!("[SSH] SSH connection via SOCKS5 failed: {}", e);
                            format!("Failed to connect to jumphost via SOCKS5: {}", e)
                        })?
                } else if p.proxy_type == "http" {
                    info!("[SSH] Attempting HTTP proxy connection to jumphost...");
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    info!("[SSH] HTTP: Connecting to proxy {}:{}", p.host, p.port);
                    let mut stream = tokio::net::TcpStream::connect((p.host.as_str(), p.port))
                        .await
                        .map_err(|e| {
                            error!("[SSH] HTTP proxy TCP connect failed: {}", e);
                            format!("Failed to connect to HTTP proxy: {}", e)
                        })?;

                    let mut auth_header = String::new();
                    if let Some(u) = &p.username {
                        if !u.is_empty() {
                            let p_auth = p.password.as_ref().map(|s| s.as_str()).unwrap_or("");
                            let auth_str = format!("{}:{}", u, p_auth);
                            let encoded = BASE64_STANDARD.encode(auth_str);
                            auth_header = format!("\r\nProxy-Authorization: Basic {}", encoded);
                            info!("[SSH] HTTP: Using proxy auth for user {}", u);
                        }
                    }

                    let connect_req = format!(
                        "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{} {}\r\n\r\n",
                        j.host, j.port, j.host, j.port, auth_header
                    );
                    info!(
                        "[SSH] HTTP: Sending CONNECT request for {}:{}",
                        j.host, j.port
                    );
                    stream
                        .write_all(connect_req.as_bytes())
                        .await
                        .map_err(|e| {
                            error!("[SSH] HTTP CONNECT write failed: {}", e);
                            e.to_string()
                        })?;

                    let mut response = [0u8; 4096];
                    let n = stream.read(&mut response).await.map_err(|e| {
                        error!("[SSH] HTTP CONNECT response read failed: {}", e);
                        e.to_string()
                    })?;
                    let response_text = String::from_utf8_lossy(&response[..n]);
                    info!("[SSH] HTTP proxy response: {}", response_text);

                    if !response_text.contains("200 Connection established")
                        && !response_text.contains("200 OK")
                    {
                        error!("[SSH] HTTP proxy rejected connection: {}", response_text);
                        return Err(format!(
                            "HTTP proxy returned error when connecting to jumphost: {}",
                            response_text
                        ));
                    }
                    info!("[SSH] HTTP tunnel established, now connecting SSH...");
                    client::connect_stream(config.clone(), stream, jh_handler)
                        .await
                        .map_err(|e| {
                            error!("[SSH] SSH connection via HTTP proxy failed: {}", e);
                            format!("Failed to connect to jumphost via HTTP proxy: {}", e)
                        })?
                } else {
                    return Err(format!(
                        "Unsupported proxy type for jumphost connection: {}",
                        p.proxy_type
                    ));
                }
            } else {
                // Direct connection to jumphost
                info!(
                    "[SSH] Direct connection to jumphost {}:{} (no proxy)",
                    j.host, j.port
                );
                client::connect(config.clone(), (j.host.as_str(), j.port), jh_handler)
                    .await
                    .map_err(|e| {
                        error!("[SSH] Direct jumphost connection failed: {}", e);
                        format!("Failed to connect to jumphost: {}", e)
                    })?
            };

            info!("[SSH] Connected to jumphost successfully, now authenticating...");
            let _ = tx
                .send((
                    session_id.clone(),
                    "Authenticating jump host...\r\n".as_bytes().to_vec(),
                ))
                .await;
            info!(
                "[SSH] Jumphost auth: username={}, password={}, private_key={}",
                j.username,
                if j.password.is_some() { "yes" } else { "no" },
                if j.private_key.is_some() { "yes" } else { "no" }
            );
            Self::authenticate_session(
                &mut jh_session,
                &j.username,
                j.password.clone(),
                j.private_key.clone(),
                j.passphrase.clone(),
            )
            .await
            .map_err(|e| {
                error!("[SSH] Jumphost authentication failed: {}", e);
                format!("Jumphost authentication failed: {}", e)
            })?;
            info!("[SSH] Jumphost authentication successful");

            info!(
                "[SSH] Opening tunnel to target {}:{}",
                params.host, params.port
            );
            let _ = tx
                .send((
                    session_id.clone(),
                    format!("Tunneling to {}:{}...\r\n", params.host, params.port)
                        .as_bytes()
                        .to_vec(),
                ))
                .await;
            let channel = jh_session
                .channel_open_direct_tcpip(&params.host, params.port as u32, "127.0.0.1", 22222)
                .await
                .map_err(|e| {
                    error!("[SSH] Failed to open direct-tcpip tunnel: {}", e);
                    format!("Failed to open direct-tcpip through jumphost: {}", e)
                })?;
            info!("[SSH] Tunnel opened successfully, connecting to target via tunnel...");

            jh_handle_to_store = Some(jh_session);

            client::connect_stream(config, channel.into_stream(), handler).await
        } else {
            client::connect(config, (&params.host[..], params.port), handler).await
        }
        .map_err(|e| {
            error!("[SSH] Connection error: {}", e);
            e.to_string()
        })?;

        let _ = tx
            .send((
                session_id.clone(),
                format!("Authenticating as {}...\r\n", params.username)
                    .as_bytes()
                    .to_vec(),
            ))
            .await;
        Self::authenticate_session(
            &mut session,
            &params.username,
            params.password.clone(),
            params.private_key.clone(),
            params.passphrase.clone(),
        )
        .await?;

        info!("[SSH] Authentication successful. Opening channel...");

        let channel = session
            .channel_open_session()
            .await
            .map_err(|e| e.to_string())?;

        channel
            .request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
            .await
            .map_err(|e| format!("PTY request failed: {}", e))?;
        channel
            .request_shell(true)
            .await
            .map_err(|e| format!("Shell request failed: {}", e))?;

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
            info!(
                "[SSH] Attempting publickey auth for user: '{}' (has passphrase: {})",
                username,
                passphrase.is_some()
            );
            let key = russh_keys::decode_secret_key(&key_content, passphrase.as_deref()).map_err(
                |e| {
                    error!("[SSH] Failed to decode private key: {}", e);
                    format!("Failed to decode private key: {}", e)
                },
            )?;

            let key_pair = Arc::new(key);
            match session.authenticate_publickey(username, key_pair).await {
                Ok(true) => {
                    authenticated = true;
                    info!("[SSH] Publickey authentication successful.");
                }
                Ok(false) => warn!("[SSH] Publickey authentication rejected."),
                Err(e) => error!("[SSH] Publickey authentication error: {}", e),
            }
        }

        if !authenticated {
            if let Some(pwd) = password {
                info!("[SSH] Attempting password auth for user: '{}'...", username);
                match session.authenticate_password(username, &pwd).await {
                    Ok(true) => {
                        authenticated = true;
                        info!("[SSH] Password authentication successful.");
                    }
                    Ok(false) => warn!("[SSH] Password authentication failed."),
                    Err(e) => error!("[SSH] Password authentication error: {}", e),
                }
            }
        }

        if !authenticated {
            error!(
                "[SSH] All authentication methods failed for user: {}",
                username
            );
            return Err(
                "AUTH_PASSWORD_REQUIRED: Authentication failed. Please enter your password."
                    .to_string(),
            );
        }

        info!("[SSH] Authentication complete for user: {}", username);
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
            info!(
                "[SSH] Send failed, attempting to reconnect session {}...",
                session_id
            );
            if let Err(e) = Self::reconnect(session_id).await {
                error!("[SSH] Reconnection failed: {}", e);
                return Err(format!("Connection lost and reconnection failed: {}", e));
            }

            // Retry sending
            let mut sessions = SESSIONS.lock().await;
            if let Some(session_data) = sessions.get_mut(session_id) {
                session_data
                    .channel
                    .data(data)
                    .await
                    .map_err(|e| format!("Failed to send data after reconnect: {}", e))?;
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
        let _ = tx
            .send((
                session_id.to_string(),
                "\r\n[Resh] Connection lost. Reconnecting...\r\n"
                    .as_bytes()
                    .to_vec(),
            ))
            .await;

        match Self::establish_connection(session_id.to_string(), &config, tx.clone(), cols, rows)
            .await
        {
            Ok((channel, session, jh_session)) => {
                let mut sessions = SESSIONS.lock().await;
                if let Some(data) = sessions.get_mut(session_id) {
                    data.channel = channel;
                    data.session = session;
                    data.jumphost_session = jh_session;
                    // Reset terminal state for new connection
                    data.terminal_buffer.clear();
                    data.command_recorder = None;
                    data.last_output_len = 0;
                    data.recording_prompt = None;
                    data.command_finished = false;
                    info!("[SSH] Reconnection successful for {}", session_id);
                    Ok(())
                } else {
                    Err("Session removed during reconnection".to_string())
                }
            }
            Err(e) => {
                let _ = tx
                    .send((
                        session_id.to_string(),
                        format!("\r\n[Resh] Reconnection failed: {}\r\n", e)
                            .as_bytes()
                            .to_vec(),
                    ))
                    .await;
                Err(e)
            }
        }
    }

    pub async fn resize(session_id: &str, cols: u32, rows: u32) -> Result<(), String> {
        let mut sessions = SESSIONS.lock().await;
        if let Some(session_data) = sessions.get_mut(session_id) {
            session_data.cols = cols;
            session_data.rows = rows;
            session_data
                .channel
                .window_change(cols, rows, 0, 0)
                .await
                .map_err(|e| format!("Failed to resize: {}", e))?;
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
            session_data.recording_prompt =
                Self::extract_last_line_raw(&session_data.terminal_buffer);
            session_data.command_finished = false;
            session_data.last_exit_code = None;

            tracing::info!(
                "[start_command_recording] {} - buffer_len={}, last_output_len={}, recorded_prompt={:?}",
                session_id,
                session_data.terminal_buffer.len(),
                session_data.last_output_len,
                session_data.recording_prompt
            );

            Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }

    /// Extract the last line from buffer, cleaned of ANSI escapes
    /// Extract the RAW last line from buffer (without ANSI cleaning)
    /// This preserves Nerd Font icon bytes which are different between icons
    fn extract_last_line_raw(buffer: &str) -> Option<String> {
        let trimmed = buffer.trim_end();
        if trimmed.is_empty() {
            return None;
        }
        let lines: Vec<&str> = trimmed.lines().collect();
        if let Some(last_line) = lines.last() {
            let trimmed_line = last_line.trim_end();
            if !trimmed_line.is_empty() {
                return Some(trimmed_line.to_string());
            }
        }
        None
    }

    /// Check if a line looks like a shell prompt
    fn is_prompt_like(line: &str) -> bool {
        // Common prompt characters (including Nerd Font / Powerline symbols)
        let prompt_chars = ['$', '#', '>', '%', '❯', '➜', '→', '»', 'λ', '♦', '', '▶'];
        let trimmed = line.trim();

        // Check if line ends with common prompt characters (after trimming whitespace)
        for ch in &prompt_chars {
            if trimmed.ends_with(*ch) {
                return true;
            }
        }

        // Also check if the line is very short (like just a prompt symbol)
        // This handles cases where the prompt is just an icon like ""
        // Note: use chars().count() for Unicode character count, not .len() which returns bytes
        if trimmed.chars().count() <= 3 {
            // Single character line is likely a prompt
            return true;
        }

        // Check for patterns that look like paths (e.g., "/home/user" or "~/project")
        // followed by a space or special character
        if trimmed.contains('/') || trimmed.contains('~') || trimmed.contains('\\') {
            // If line contains path-like patterns and is relatively short, likely a prompt
            if trimmed.len() < 200 {
                return true;
            }
        }

        // Check for common prompt patterns (e.g., "user@host:path$" format)
        if trimmed.contains('@') && (trimmed.contains(':') || trimmed.contains(' ')) {
            return true;
        }

        false
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

    /// Check if command execution appears completed (prompt detected or completion marker)
    pub async fn check_command_completed(session_id: &str) -> Result<bool, String> {
        let sessions = SESSIONS.lock().await;
        if let Some(session_data) = sessions.get(session_id) {
            if session_data.command_recorder.is_none() {
                tracing::debug!(
                    "[check_command_completed] {} - no recorder, returning true",
                    session_id
                );
                return Ok(true);
            }

            let current_buffer = &session_data.terminal_buffer;
            let recording_start = session_data.last_output_len;
            let new_content = if current_buffer.len() > recording_start {
                &current_buffer[recording_start..]
            } else {
                ""
            };

            tracing::debug!("[check_command_completed] {} - buffer_len={}, start={}, content_len={}, content={:?}",
                session_id, current_buffer.len(), recording_start, new_content.len(), new_content.chars().take(50).collect::<String>());

            // Priority 1: Completion marker detection (pure control chars)
            // The marker is: \x1b\x1b (double ESC, completely invisible)
            if new_content.contains("\x1b\x1b") {
                tracing::debug!(
                    "[check_command_completed] {} - Completion marker detected, returning true",
                    session_id
                );
                return Ok(true);
            }

            // Priority 2: New prompt detection - check if a new prompt line appeared
            let lines: Vec<&str> = new_content.lines().collect();
            tracing::info!("[check_command_completed] {} - new_content has {} lines", session_id, lines.len());
            
            if let Some(last_line) = lines.last() {
                let last_line_trimmed = last_line.trim_end();

                // Compare RAW content (not cleaned) - Nerd Font icons have different UTF-8 bytes
                // This avoids the issue where strip_ansi_escapes converts different icons to same garbled text
                tracing::info!("[check_command_completed] {} - raw last_line={:?}, recorded={:?}",
                    session_id, last_line_trimmed, session_data.recording_prompt);

                if let Some(ref recorded) = session_data.recording_prompt {
                    // Check if the last line changed (indicates new prompt appeared)
                    if last_line_trimmed != recorded {
                        // New line appeared, check if it looks like a prompt
                        if Self::is_prompt_like(last_line_trimmed) {
                            tracing::debug!("[check_command_completed] {} - new prompt detected: {:?} (was: {:?}), returning true",
                                session_id, last_line_trimmed, recorded);
                            return Ok(true);
                        } else {
                            tracing::info!("[check_command_completed] {} - line differs but not prompt-like: {:?}",
                                session_id, last_line_trimmed);
                        }
                    } else {
                        tracing::info!("[check_command_completed] {} - line same as recorded: {:?}",
                            session_id, last_line_trimmed);
                    }
                } else {
                    // No previous prompt recorded, just check if it looks like a prompt
                    if Self::is_prompt_like(last_line_trimmed) {
                        tracing::debug!("[check_command_completed] {} - prompt detected: {:?}, returning true",
                            session_id, last_line_trimmed);
                        return Ok(true);
                    } else {
                        tracing::info!("[check_command_completed] {} - no recorded prompt and line not prompt-like: {:?}",
                            session_id, last_line_trimmed);
                    }
                }
            }

            tracing::debug!("[check_command_completed] {} - returning false", session_id);
            Ok(false)
        } else {
            Err("Session not found".to_string())
        }
    }

    /// Send interrupt signal (Ctrl+C) to the terminal
    pub async fn send_interrupt(session_id: &str) -> Result<(), String> {
        let result = {
            let mut sessions = SESSIONS.lock().await;
            if let Some(session_data) = sessions.get_mut(session_id) {
                session_data.channel.data(&[3u8][..]).await // 3 = ETX (Ctrl+C)
            } else {
                return Err("Session not found".to_string());
            }
        };

        if result.is_err() {
            info!(
                "[SSH] Send interrupt failed, attempting to reconnect session {}...",
                session_id
            );
            if let Err(e) = Self::reconnect(session_id).await {
                error!("[SSH] Reconnection failed: {}", e);
                return Err(format!("Connection lost and reconnection failed: {}", e));
            }

            // Retry sending
            let mut sessions = SESSIONS.lock().await;
            if let Some(session_data) = sessions.get_mut(session_id) {
                session_data
                    .channel
                    .data(&[3u8][..])
                    .await
                    .map_err(|e| format!("Failed to send interrupt after reconnect: {}", e))?;
            } else {
                return Err("Session lost during reconnect".to_string());
            }
        }

        Ok(())
    }

    /// Send arbitrary input (characters, escape sequences) to the terminal
    pub async fn send_terminal_input(session_id: &str, input: &str) -> Result<(), String> {
        let result = {
            let mut sessions = SESSIONS.lock().await;
            if let Some(session_data) = sessions.get_mut(session_id) {
                session_data.channel.data(input.as_bytes()).await
            } else {
                return Err("Session not found".to_string());
            }
        };

        if result.is_err() {
            info!(
                "[SSH] Send input failed, attempting to reconnect session {}...",
                session_id
            );
            if let Err(e) = Self::reconnect(session_id).await {
                error!("[SSH] Reconnection failed: {}", e);
                return Err(format!("Connection lost and reconnection failed: {}", e));
            }

            // Retry sending
            let mut sessions = SESSIONS.lock().await;
            if let Some(session_data) = sessions.get_mut(session_id) {
                session_data
                    .channel
                    .data(input.as_bytes())
                    .await
                    .map_err(|e| format!("Failed to send input after reconnect: {}", e))?;
            } else {
                return Err("Session lost during reconnect".to_string());
            }
        }

        Ok(())
    }

    /// Update the terminal buffer with new data
    pub async fn update_terminal_buffer(session_id: &str, data: &str) -> Result<(), String> {
        const MAX_BUFFER_SIZE: usize = 100_000;

        let mut sessions = SESSIONS.lock().await;
        if let Some(session_data) = sessions.get_mut(session_id) {
            session_data.terminal_buffer.push_str(data);

            if session_data.terminal_buffer.len() > MAX_BUFFER_SIZE {
                let mut cut_off = session_data.terminal_buffer.len() - MAX_BUFFER_SIZE;
                while cut_off < session_data.terminal_buffer.len()
                    && !session_data.terminal_buffer.is_char_boundary(cut_off)
                {
                    cut_off += 1;
                }
                session_data.terminal_buffer = session_data.terminal_buffer[cut_off..].to_string();
            }

            if let Some(recorder) = session_data.command_recorder.as_mut() {
                recorder.push_str(data);
            }

            Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }

    pub async fn update_system_info(session_id: &str, info: SystemInfo) -> Result<(), String> {
        let mut sessions = SESSIONS.lock().await;
        if let Some(session_data) = sessions.get_mut(session_id) {
            session_data.system_info = Some(info);
            Ok(())
        } else {
            Err("Session not found".to_string())
        }
    }

    pub async fn get_system_info(session_id: &str) -> Option<SystemInfo> {
        let sessions = SESSIONS.lock().await;
        sessions.get(session_id).and_then(|s| s.system_info.clone())
    }

    pub async fn gather_system_info(params: ConnectParams) -> Result<SystemInfo, String> {
        let mut config = client::Config::default();
        config.preferred.key = std::borrow::Cow::Owned(vec![
            russh::keys::key::Name("ssh-ed25519"),
            russh::keys::key::Name("rsa-sha2-256"),
            russh::keys::key::Name("rsa-sha2-512"),
            russh::keys::key::Name("ssh-rsa"),
        ]);
        let config = Arc::new(config);
        let handler = ClientHandler::new();

        let mut session = if let Some(p) = &params.proxy {
            if p.proxy_type == "socks5" {
                use tokio_socks::tcp::Socks5Stream;
                let has_auth = p.username.as_ref().map(|u| !u.is_empty()).unwrap_or(false);
                let stream = if has_auth {
                    let u = p.username.as_ref().unwrap();
                    let p_auth = p.password.as_ref().map(|s| s.as_str()).unwrap_or("");
                    Socks5Stream::connect_with_password(
                        (p.host.as_str(), p.port),
                        (&params.host[..], params.port),
                        u,
                        p_auth,
                    )
                    .await
                    .map_err(|e| format!("SOCKS5 proxy error: {}", e))?
                } else {
                    Socks5Stream::connect(
                        (p.host.as_str(), p.port),
                        (&params.host[..], params.port),
                    )
                    .await
                    .map_err(|e| format!("SOCKS5 proxy error: {}", e))?
                };
                client::connect_stream(config, stream, handler).await
            } else if p.proxy_type == "http" {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut stream = tokio::net::TcpStream::connect((p.host.as_str(), p.port))
                    .await
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
                stream
                    .write_all(connect_req.as_bytes())
                    .await
                    .map_err(|e| e.to_string())?;

                let mut response = [0u8; 4096];
                let n = stream
                    .read(&mut response)
                    .await
                    .map_err(|e| e.to_string())?;
                let response_text = String::from_utf8_lossy(&response[..n]);
                if !response_text.contains("200 Connection established")
                    && !response_text.contains("200 OK")
                {
                    return Err(format!("HTTP proxy returned error: {}", response_text));
                }
                client::connect_stream(config, stream, handler).await
            } else {
                return Err(format!("Unsupported proxy type: {}", p.proxy_type));
            }
        } else if let Some(j) = &params.jumphost {
            let jh_handler = ClientHandler::new();
            let mut jh_session = if let Some(p) = &params.proxy {
                if p.proxy_type == "socks5" {
                    use tokio_socks::tcp::Socks5Stream;
                    let has_auth = p.username.as_ref().map(|u| !u.is_empty()).unwrap_or(false);
                    let stream = if has_auth {
                        let u = p.username.as_ref().unwrap();
                        let p_auth = p.password.as_ref().map(|s| s.as_str()).unwrap_or("");
                        Socks5Stream::connect_with_password(
                            (p.host.as_str(), p.port),
                            (&j.host[..], j.port),
                            u,
                            p_auth,
                        )
                        .await
                        .map_err(|e| {
                            format!("SOCKS5 proxy error when connecting to jumphost: {}", e)
                        })?
                    } else {
                        Socks5Stream::connect((p.host.as_str(), p.port), (&j.host[..], j.port))
                            .await
                            .map_err(|e| {
                                format!("SOCKS5 proxy error when connecting to jumphost: {}", e)
                            })?
                    };
                    client::connect_stream(config.clone(), stream, jh_handler)
                        .await
                        .map_err(|e| format!("Failed to connect to jumphost via SOCKS5: {}", e))?
                } else if p.proxy_type == "http" {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut stream = tokio::net::TcpStream::connect((p.host.as_str(), p.port))
                        .await
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
                        j.host, j.port, j.host, j.port, auth_header
                    );
                    stream
                        .write_all(connect_req.as_bytes())
                        .await
                        .map_err(|e| e.to_string())?;

                    let mut response = [0u8; 4096];
                    let n = stream
                        .read(&mut response)
                        .await
                        .map_err(|e| e.to_string())?;
                    let response_text = String::from_utf8_lossy(&response[..n]);
                    if !response_text.contains("200 Connection established")
                        && !response_text.contains("200 OK")
                    {
                        return Err(format!(
                            "HTTP proxy returned error when connecting to jumphost: {}",
                            response_text
                        ));
                    }
                    client::connect_stream(config.clone(), stream, jh_handler)
                        .await
                        .map_err(|e| {
                            format!("Failed to connect to jumphost via HTTP proxy: {}", e)
                        })?
                } else {
                    return Err(format!("Unsupported proxy type: {}", p.proxy_type));
                }
            } else {
                client::connect(config.clone(), (j.host.as_str(), j.port), jh_handler)
                    .await
                    .map_err(|e| format!("Failed to connect to jumphost: {}", e))?
            };

            Self::authenticate_session(
                &mut jh_session,
                &j.username,
                j.password.clone(),
                j.private_key.clone(),
                j.passphrase.clone(),
            )
            .await
            .map_err(|e| format!("Jumphost authentication failed: {}", e))?;

            let channel = jh_session
                .channel_open_direct_tcpip(&params.host, params.port as u32, "127.0.0.1", 22222)
                .await
                .map_err(|e| format!("Failed to open direct-tcpip through jumphost: {}", e))?;

            client::connect_stream(config, channel.into_stream(), handler).await
        } else {
            client::connect(config, (&params.host[..], params.port), handler).await
        }
        .map_err(|e| e.to_string())?;

        Self::authenticate_session(
            &mut session,
            &params.username,
            params.password.clone(),
            params.private_key.clone(),
            params.passphrase.clone(),
        )
        .await?;

        let mut channel = session
            .channel_open_session()
            .await
            .map_err(|e| e.to_string())?;

        let cmd = "sh -c '\
            OS=$(uname -s 2>/dev/null || echo Unknown); \
            UNAME_V=$(uname -v 2>/dev/null || echo Unknown); \
            echo \"OS:$OS\"; \
            echo \"UNAME_V:$UNAME_V\"; \
            [ -f /etc/os-release ] && cat /etc/os-release; \
            echo \"SHELL:${1:-$2}\"' -- \"$SHELL\" \"$0\"";
        channel.exec(true, cmd).await.map_err(|e| e.to_string())?;

        let mut output = String::new();
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));
        let mut elapsed = 0;

        loop {
            tokio::select! {
                Some(msg) = channel.wait() => {
                    match msg {
                        russh::ChannelMsg::Data { ref data } => {
                            output.push_str(&String::from_utf8_lossy(data));
                        }
                        russh::ChannelMsg::Eof => break,
                        russh::ChannelMsg::Close => break,
                        _ => {}
                    }
                }
                _ = interval.tick() => {
                    elapsed += 100;
                    if elapsed > 5000 { break; }
                }
            }
        }

        let mut info = SystemInfo {
            os: "Unknown".to_string(),
            distro: "Unknown".to_string(),
            username: params.username.clone(),
            ip: params.host.clone(),
            shell: "Unknown".to_string(),
        };

        if let Ok(mut addrs) = lookup_host(format!("{}:{}", params.host, params.port)).await {
            if let Some(addr) = addrs.next() {
                info.ip = addr.ip().to_string();
            }
        }

        let mut uname_v = String::new();

        for line in output.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("OS:") {
                info.os = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("UNAME_V:") {
                uname_v = val.trim().to_string();
            } else if line.starts_with("PRETTY_NAME=") {
                info.distro = line
                    .split('=')
                    .nth(1)
                    .unwrap_or("")
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string();
            } else if let Some(val) = line.strip_prefix("SHELL:") {
                info.shell = val.trim().to_string();
            }
        }

        if info.distro == "Unknown" && !uname_v.is_empty() {
            info.distro = uname_v;
        }

        Ok(info)
    }
}
