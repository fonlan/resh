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
                run_id TEXT,
                turn_index INTEGER,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(session_id) REFERENCES ai_sessions(id) ON DELETE CASCADE,
                FOREIGN KEY(run_id) REFERENCES ai_runs(id) ON DELETE SET NULL
            )",
            [],
        )?;

        // Migration: Ensure columns added after the original history-only schema exist.
        let _ = conn.execute("ALTER TABLE ai_messages ADD COLUMN tool_call_id TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE ai_messages ADD COLUMN reasoning_content TEXT",
            [],
        );
        let _ = conn.execute("ALTER TABLE ai_sessions ADD COLUMN ssh_session_id TEXT", []);
        let _ = conn.execute("ALTER TABLE ai_messages ADD COLUMN model_id TEXT", []);
        let _ = conn.execute("ALTER TABLE ai_messages ADD COLUMN run_id TEXT", []);
        let _ = conn.execute("ALTER TABLE ai_messages ADD COLUMN turn_index INTEGER", []);

        conn.execute(
            "CREATE TABLE IF NOT EXISTS ai_runs (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                request_id TEXT NOT NULL UNIQUE,
                active_request_id TEXT NOT NULL,
                status TEXT NOT NULL,
                stop_reason TEXT,
                model_turn_count INTEGER NOT NULL DEFAULT 0,
                total_tool_call_count INTEGER NOT NULL DEFAULT 0,
                started_at_ms INTEGER NOT NULL,
                completed_at_ms INTEGER,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(session_id) REFERENCES ai_sessions(id) ON DELETE CASCADE
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS ai_tool_invocations (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                tool_call_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                arguments_json TEXT NOT NULL,
                status TEXT NOT NULL,
                turn_index INTEGER NOT NULL,
                approval_id TEXT,
                background_task_id TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(run_id, tool_call_id),
                FOREIGN KEY(run_id) REFERENCES ai_runs(id) ON DELETE CASCADE
            )",
            [],
        )?;
        let _ = conn.execute(
            "ALTER TABLE ai_tool_invocations ADD COLUMN approval_id TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE ai_tool_invocations ADD COLUMN background_task_id TEXT",
            [],
        );
        conn.execute(
            "CREATE TABLE IF NOT EXISTS ai_tool_approval_grants (
                session_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY(session_id, tool_name),
                FOREIGN KEY(session_id) REFERENCES ai_sessions(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_ai_runs_session_status ON ai_runs(session_id, status)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_ai_tool_invocations_run_status ON ai_tool_invocations(run_id, status)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_ai_tool_invocations_approval ON ai_tool_invocations(run_id, turn_index, approval_id)",
            [],
        )?;
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_ai_tool_invocations_background_task
             ON ai_tool_invocations(background_task_id) WHERE background_task_id IS NOT NULL",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS ai_background_task_results (
                task_id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                completed_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_ai_background_task_results_status
             ON ai_background_task_results(status)",
            [],
        )?;

        // A process restart never replays a side-effecting invocation. Approval waits are
        // durable; active work is explicitly marked interrupted for recovery/UI inspection.
        conn.execute(
            "UPDATE ai_tool_invocations SET status = 'interrupted', updated_at = CURRENT_TIMESTAMP
             WHERE status IN ('executing', 'queued', 'running')",
            [],
        )?;
        conn.execute(
            "UPDATE ai_runs SET status = 'interrupted', stop_reason = 'app_restarted',
                 completed_at_ms = CAST(strftime('%s', 'now') AS INTEGER) * 1000,
                 updated_at = CURRENT_TIMESTAMP
             WHERE status = 'running'",
            [],
        )?;

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
