use rusqlite::{Connection, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct DatabaseManager {
    conn: Arc<Mutex<Connection>>,
}

impl DatabaseManager {
    pub fn new(app_data_dir: PathBuf) -> Result<Self> {
        let db_path = app_data_dir.join("config.db");
        tracing::debug!("Opening database at {:?}", db_path);
        let conn = Connection::open(db_path)?;

        // Enable foreign keys
        conn.execute("PRAGMA foreign_keys = ON;", [])?;

        let manager = DatabaseManager {
            conn: Arc::new(Mutex::new(conn)),
        };

        manager.init_tables()?;

        Ok(manager)
    }

    fn init_tables(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            "CREATE TABLE IF NOT EXISTS ai_sessions (
                id TEXT PRIMARY KEY,
                server_id TEXT NOT NULL,
                title TEXT NOT NULL,
                model_id TEXT,
                ssh_session_id TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS ai_messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                reasoning_content TEXT,
                tool_calls TEXT,
                tool_call_id TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(session_id) REFERENCES ai_sessions(id) ON DELETE CASCADE
            )",
            [],
        )?;

        // Migration: Ensure tool_call_id column exists for existing tables
        let _ = conn.execute("ALTER TABLE ai_messages ADD COLUMN tool_call_id TEXT", []);
        // Migration: Ensure reasoning_content column exists
        let _ = conn.execute(
            "ALTER TABLE ai_messages ADD COLUMN reasoning_content TEXT",
            [],
        );

        // Migration: Ensure ssh_session_id column exists for ai_sessions
        let _ = conn.execute("ALTER TABLE ai_sessions ADD COLUMN ssh_session_id TEXT", []);

        let _ = conn.execute(
            "DELETE FROM ai_sessions 
             WHERE NOT EXISTS (SELECT 1 FROM ai_messages WHERE session_id = ai_sessions.id)",
            [],
        );

        Ok(())
    }

    pub fn get_connection(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }
}
