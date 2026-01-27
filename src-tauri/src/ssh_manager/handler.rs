use russh::client;
use russh_keys::key;
use async_trait::async_trait;
use tokio::sync::mpsc;

pub struct ClientHandler {
    pub session_id: Option<String>,
    pub tx: Option<mpsc::Sender<(String, Vec<u8>)>>,
}

impl ClientHandler {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            session_id: None,
            tx: None,
        }
    }

    pub fn with_channel(session_id: String, tx: mpsc::Sender<(String, Vec<u8>)>) -> Self {
        Self {
            session_id: Some(session_id),
            tx: Some(tx),
        }
    }
}

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)  // Accept all keys
    }

    async fn data(
        &mut self,
        _channel: russh::ChannelId,
        data: &[u8],
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        // Forward data from SSH server to frontend
        if let (Some(session_id), Some(tx)) = (&self.session_id, &self.tx) {
            let _ = tx.send((session_id.clone(), data.to_vec())).await;
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        _channel: russh::ChannelId,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        // Drop the transmitter to signal the frontend that the connection is closed
        self.tx = None;
        Ok(())
    }
}
