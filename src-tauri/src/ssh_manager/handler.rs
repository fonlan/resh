use crate::ssh_manager::ssh::SSHClient;
use russh::client;
use russh::keys;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::debug;

pub struct ClientHandler {
    pub session_id: Option<String>,
    pub tx: Option<mpsc::UnboundedSender<(String, Vec<u8>)>>,
    pub shell_channel_id: Arc<Mutex<Option<russh::ChannelId>>>,
}

impl ClientHandler {
    pub fn new() -> Self {
        Self {
            session_id: None,
            tx: None,
            shell_channel_id: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_channel(
        session_id: String,
        tx: mpsc::UnboundedSender<(String, Vec<u8>)>,
        shell_channel_id: Arc<Mutex<Option<russh::ChannelId>>>,
    ) -> Self {
        Self {
            session_id: Some(session_id),
            tx: Some(tx),
            shell_channel_id,
        }
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        _server_public_key: &keys::PublicKey,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        async { Ok(true) } // Accept all keys
    }

    fn data(
        &mut self,
        channel: russh::ChannelId,
        data: &[u8],
        _session: &mut russh::client::Session,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let shell_channel_id = self.shell_channel_id.clone();
        let session_id = self.session_id.clone();
        let tx = self.tx.clone();
        let owned_data = data.to_vec();

        async move {
            let should_forward = {
                let id_guard = shell_channel_id.lock().await;
                match *id_guard {
                    Some(id) => id == channel,
                    None => true,
                }
            };

            if should_forward {
                if let (Some(session_id), Some(tx)) = (session_id.as_ref(), tx.as_ref()) {
                    let text = String::from_utf8_lossy(&owned_data);
                    if let Err(e) =
                        SSHClient::append_command_recorder_chunk(session_id, text.as_ref()).await
                    {
                        if e != "Session not found" {
                            debug!(
                                "[SSH] Failed to append recorder chunk for {}: {}",
                                session_id, e
                            );
                        }
                    }
                    let _ = tx.send((session_id.clone(), owned_data));
                }
            }

            Ok(())
        }
    }

    fn channel_close(
        &mut self,
        channel: russh::ChannelId,
        _session: &mut russh::client::Session,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let should_clear_tx = self
            .shell_channel_id
            .try_lock()
            .map(|guard| match *guard {
                Some(id) => id == channel,
                None => true,
            })
            .unwrap_or(true);
        if should_clear_tx {
            self.tx = None;
        }

        async { Ok(()) }
    }
}
