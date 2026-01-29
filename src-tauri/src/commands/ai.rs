use crate::commands::AppState;
use crate::ai::client::{stream_openai_chat, ChatMessage, create_agent_tools};
use crate::ssh_manager::ssh::SSHClient;
use tauri::{State, Window, Emitter};
use std::sync::Arc;
use uuid::Uuid;
use rusqlite::params;
use futures::StreamExt;

#[tauri::command]
pub async fn create_ai_session(
    state: State<'_, Arc<AppState>>,
    server_id: String,
    model_id: Option<String>,
) -> Result<String, String> {
    let id = Uuid::new_v4().to_string();
    let conn = state.db_manager.get_connection();
    let conn = conn.lock().unwrap();
    
    conn.execute(
        "INSERT INTO ai_sessions (id, server_id, title, model_id) VALUES (?1, ?2, ?3, ?4)",
        params![id, server_id, "New Chat", model_id],
    ).map_err(|e| e.to_string())?;

    Ok(id)
}

#[tauri::command]
pub async fn get_ai_sessions(
    state: State<'_, Arc<AppState>>,
    server_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = state.db_manager.get_connection();
    let conn = conn.lock().unwrap();
    
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at, model_id FROM ai_sessions WHERE server_id = ?1 ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;
    
    let rows = stmt.query_map(params![server_id], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "title": row.get::<_, String>(1)?,
            "createdAt": row.get::<_, String>(2)?,
            "modelId": row.get::<_, Option<String>>(3)?,
        }))
    }).map_err(|e| e.to_string())?;

    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row.map_err(|e| e.to_string())?);
    }
    
    Ok(sessions)
}

#[tauri::command]
pub async fn get_ai_messages(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<Vec<ChatMessage>, String> {
    let conn = state.db_manager.get_connection();
    let conn = conn.lock().unwrap();
    
    let mut stmt = conn.prepare(
        "SELECT role, content FROM ai_messages WHERE session_id = ?1 ORDER BY created_at ASC"
    ).map_err(|e| e.to_string())?;
    
    let rows = stmt.query_map(params![session_id], |row| {
        Ok(ChatMessage {
            role: row.get(0)?,
            content: row.get(1)?,
        })
    }).map_err(|e| e.to_string())?;

    let mut messages = Vec::new();
    for row in rows {
        messages.push(row.map_err(|e| e.to_string())?);
    }
    
    Ok(messages)
}

#[tauri::command]
pub async fn send_chat_message(
    window: Window,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    content: String,
    model_id: String,
    channel_id: String,
    mode: Option<String>, // "ask" or "agent"
    _ssh_session_id: Option<String>, // For agent mode tool access (not yet used)
) -> Result<(), String> {
    tracing::info!("[AI] send_chat_message called: session_id={}, model_id={}, channel_id={}, mode={:?}", 
        session_id, model_id, channel_id, mode);
    
    // Send immediate acknowledgment
    let _ = window.emit(&format!("ai-started-{}", session_id), "started");
    
    // 1. Get Channel Config and Model Name
    let (endpoint, api_key, model_name) = {
        let config = state.config.lock().await;
        
        // Find the model to get its name
        let model = config.ai_models.iter().find(|m| m.id == model_id)
            .ok_or_else(|| {
                tracing::error!("[AI] Model not found: {}", model_id);
                "Model not found".to_string()
            })?;
        
        // Find the channel
        let channel = config.ai_channels.iter().find(|c| c.id == channel_id)
            .ok_or_else(|| {
                tracing::error!("[AI] Channel not found: {}", channel_id);
                "Channel not found".to_string()
            })?;
        
        (
            channel.endpoint.clone().unwrap_or("https://api.openai.com/v1".to_string()),
            channel.api_key.clone().unwrap_or_default(),
            model.name.clone(),
        )
    };
    
    if api_key.is_empty() {
        tracing::error!("[AI] API key is empty for channel: {}", channel_id);
        return Err("API key is not configured".to_string());
    }
    
    tracing::info!("[AI] Using endpoint: {}, model: {}, API key length: {}", 
        endpoint, model_name, api_key.len());

    // 2. Save User Message to DB
    let user_msg_id = Uuid::new_v4().to_string();
    {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        conn.execute(
            "INSERT INTO ai_messages (id, session_id, role, content) VALUES (?1, ?2, ?3, ?4)",
            params![user_msg_id, session_id, "user", content],
        ).map_err(|e| e.to_string())?;
    }

    // 3. Load History
    let history = {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT role, content FROM ai_messages WHERE session_id = ?1 ORDER BY created_at ASC"
        ).map_err(|e| e.to_string())?;
        
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(ChatMessage {
                role: row.get(0)?,
                content: row.get(1)?,
            })
        }).map_err(|e| e.to_string())?;

        let mut history = Vec::new();
        for row in rows {
            history.push(row.map_err(|e| e.to_string())?);
        }
        history
    };

    // Determine if we should use tools (agent mode)
    let is_agent_mode = mode.as_deref() == Some("agent");
    let tools = if is_agent_mode {
        Some(create_agent_tools())
    } else {
        None
    };

    tracing::info!("[AI] Calling OpenAI API with {} messages, agent_mode={}", history.len(), is_agent_mode);
    
    // 4. Stream Response - Use model_name instead of model_id
    let mut stream = stream_openai_chat(&endpoint, &api_key, &model_name, history, tools).await?;
    
    let ai_msg_id = Uuid::new_v4().to_string();
    let mut full_response = String::new();

    while let Some(chunk_result) = stream.next().await {
        match chunk_result {
            Ok(chunk) => {
                if !chunk.is_empty() {
                    full_response.push_str(&chunk);
                    window.emit(&format!("ai-response-{}", session_id), chunk).map_err(|e| e.to_string())?;
                }
            }
            Err(e) => {
                window.emit(&format!("ai-error-{}", session_id), e.clone()).map_err(|e| e.to_string())?;
                // Don't error out, just log partial?
                // For now return err
                return Err(e);
            }
        }
    }
    
    // Send DONE signal
    window.emit(&format!("ai-done-{}", session_id), "DONE").map_err(|e| e.to_string())?;

    // 5. Save Assistant Message
    {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        conn.execute(
            "INSERT INTO ai_messages (id, session_id, role, content) VALUES (?1, ?2, ?3, ?4)",
            params![ai_msg_id, session_id, "assistant", full_response],
        ).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Get terminal text for AI to analyze
#[tauri::command]
pub async fn get_terminal_text(
    session_id: String,
) -> Result<String, String> {
    let text = SSHClient::get_terminal_text(&session_id).await?;
    
    // Strip ANSI codes for cleaner text
    let clean_text = String::from_utf8_lossy(&strip_ansi_escapes::strip(&text)).to_string();
    
    Ok(clean_text)
}

/// Run command in terminal (for AI agent mode)
#[tauri::command]
pub async fn run_in_terminal(
    session_id: String,
    command: String,
) -> Result<String, String> {
    // Add newline to execute the command
    let command_with_newline = format!("{}\n", command);
    SSHClient::send_input(&session_id, command_with_newline.as_bytes()).await?;
    
    Ok(format!("Command sent: {}", command))
}
