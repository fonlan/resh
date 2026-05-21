use rusqlite::{Connection, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
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

        // Migration: Ensure model_id column exists for ai_messages
        let _ = conn.execute("ALTER TABLE ai_messages ADD COLUMN model_id TEXT", []);

        let _ = conn.execute(
            "DELETE FROM ai_sessions 
             WHERE NOT EXISTS (SELECT 1 FROM ai_messages WHERE session_id = ai_sessions.id)",
            [],
        );

        Ok(())
    }

    /// 在 spawn_blocking 中执行同步 SQLite 操作，避免在 tokio worker 上阻塞 runtime。
    ///
    /// 使用方法：
    /// ```ignore
    /// let row: Option<String> = state
    ///     .db_manager
    ///     .run_blocking(move |conn| {
    ///         conn.query_row("SELECT v FROM t WHERE id = ?1", params![id], |r| r.get(0))
    ///             .optional()
    ///             .map_err(|e| e.to_string())
    ///     })
    ///     .await?;
    /// ```
    pub async fn run_blocking<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> Result<R, String> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|e| format!("DB mutex poisoned: {}", e))?;
            f(&conn)
        })
        .await
        .map_err(|e| format!("DB blocking task join error: {}", e))?
    }
}
