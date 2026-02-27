use russh::client;
use tokio::sync::mpsc;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct ClientHandler {
    pub session_id: Option<String>,
    pub tx: Option<mpsc::Sender<(String, Vec<u8>)>>,
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
        tx: mpsc::Sender<(String, Vec<u8>)>,
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

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)  // Accept all keys
    }

    async fn data(
        &mut self,
        channel: russh::ChannelId,
        data: &[u8],
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        // Only forward data if it comes from the shell channel
        let should_forward = {
            let id_guard = self.shell_channel_id.lock().await;
            match *id_guard {
                Some(id) => id == channel,
                None => true, // Forward everything if shell channel is not set yet (e.g. pre-shell phase? though data usually comes after shell)
                              // Actually, if we use SFTP, we don't want to forward if id is unknown?
                              // But establishing connection might have some data? usually not.
                              // Let's assume safely: if None, we forward (maybe banner?)
                              // But once Shell is open, we set it.
            }
        };

        if should_forward {
            if let (Some(session_id), Some(tx)) = (&self.session_id, &self.tx) {
                let _ = tx.send((session_id.clone(), data.to_vec())).await;
            }
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        _channel: russh::ChannelId,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        // Only drop tx if the SHELL channel closes
        // If SFTP channel closes, we shouldn't disconnect the terminal
        let is_shell_closed = {
            let id_guard = self.shell_channel_id.lock().await;
            match *id_guard {
                Some(id) => id == _channel,
                None => true, // Default to closing if unknown
            }
        };

        if is_shell_closed {
            self.tx = None;
        }
        Ok(())
    }
}
