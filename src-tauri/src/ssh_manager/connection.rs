use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;

/// Represents the status of an SSH connection
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
    Error(String),
}

/// Represents an SSH connection
#[allow(dead_code)]
pub struct SSHConnection {
    session_id: String,
    server_name: String,
    status: Arc<Mutex<ConnectionStatus>>,
}

#[allow(dead_code)]
impl SSHConnection {
    /// Create a new SSH connection
    pub fn new(session_id: String, server_name: String) -> Self {
        Self {
            session_id,
            server_name,
            status: Arc::new(Mutex::new(ConnectionStatus::Disconnected)),
        }
    }

    /// Get the current status of the connection
    pub async fn get_status(&self) -> ConnectionStatus {
        self.status.lock().await.clone()
    }

    /// Set the status of the connection
    pub async fn set_status(&self, new_status: ConnectionStatus) {
        *self.status.lock().await = new_status;
    }

    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the server name
    pub fn server_name(&self) -> &str {
        &self.server_name
    }
}

/// Manages multiple SSH connections
#[allow(dead_code)]
pub struct SSHConnectionManager {
    connections: Arc<Mutex<HashMap<String, SSHConnection>>>,
}

#[allow(dead_code)]
impl SSHConnectionManager {
    /// Create a new SSH connection manager
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new connection and store it in the manager
    pub async fn create_connection(&self, session_id: String, server_name: String) -> SSHConnection {
        let connection = SSHConnection::new(session_id.clone(), server_name);
        let mut connections = self.connections.lock().await;
        connections.insert(session_id, connection.clone());
        connection
    }

    /// Get a connection by session ID
    pub async fn get_connection(&self, session_id: &str) -> Option<SSHConnection> {
        let connections = self.connections.lock().await;
        connections.get(session_id).cloned()
    }

    /// Close a connection and remove it from the manager
    pub async fn close_connection(&self, session_id: &str) -> bool {
        let mut connections = self.connections.lock().await;
        connections.remove(session_id).is_some()
    }
}

#[allow(dead_code)]
impl Clone for SSHConnection {
    fn clone(&self) -> Self {
        Self {
            session_id: self.session_id.clone(),
            server_name: self.server_name.clone(),
            status: Arc::clone(&self.status),
        }
    }
}

#[allow(dead_code)]
impl Default for SSHConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_connection() {
        let manager = SSHConnectionManager::new();
        let session_id = "session_123".to_string();
        let server_name = "example.com".to_string();

        let connection = manager
            .create_connection(session_id.clone(), server_name.clone())
            .await;

        assert_eq!(connection.session_id(), &session_id);
        assert_eq!(connection.server_name(), &server_name);
        assert_eq!(connection.get_status().await, ConnectionStatus::Disconnected);

        let retrieved = manager.get_connection(&session_id).await;
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.unwrap().get_status().await,
            ConnectionStatus::Disconnected
        );
    }

    #[tokio::test]
    async fn test_close_connection() {
        let manager = SSHConnectionManager::new();
        let session_id = "session_456".to_string();
        let server_name = "example.com".to_string();

        manager
            .create_connection(session_id.clone(), server_name)
            .await;

        let closed = manager.close_connection(&session_id).await;
        assert!(closed);

        let retrieved = manager.get_connection(&session_id).await;
        assert!(retrieved.is_none());
    }
}
