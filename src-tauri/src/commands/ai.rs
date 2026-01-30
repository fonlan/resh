use crate::commands::AppState;
use crate::ai::client::{stream_openai_chat, fetch_models, ChatMessage, create_tools, StreamEvent, ToolCall, FunctionCall, ToolDefinition};
use crate::ai::prompts::SYSTEM_PROMPT;
use crate::ai::copilot;
use crate::ssh_manager::ssh::SSHClient;
use tauri::{State, Window, Emitter};
use std::sync::Arc;
use uuid::Uuid;
use rusqlite::params;
use futures::StreamExt;
use std::collections::HashMap;

// --- Helper Functions ---

async fn load_history(state: &Arc<AppState>, session_id: &str, is_agent_mode: bool) -> Result<Vec<ChatMessage>, String> {
    let max_history = {
        let config = state.config.lock().await;
        config.general.ai_max_history
    };

    let conn = state.db_manager.get_connection();
    let conn = conn.lock().unwrap();
    
    // Get the last N messages
    let mut stmt = conn.prepare(
        "SELECT role, content, tool_calls, tool_call_id FROM (
            SELECT role, content, tool_calls, tool_call_id, created_at 
            FROM ai_messages 
            WHERE session_id = ?1 
            ORDER BY created_at DESC 
            LIMIT ?2
        ) ORDER BY created_at ASC"
    ).map_err(|e| e.to_string())?;
    
    let rows = stmt.query_map(params![session_id, max_history], |row| {
         let role: String = row.get(0)?;
         let content_raw: String = row.get(1)?;
         let tool_calls_json: Option<String> = row.get(2).ok();
         let tool_call_id: Option<String> = row.get(3).ok();
         
         let content = if content_raw.is_empty() { None } else { Some(content_raw) };

         let tool_calls = if let Some(json) = tool_calls_json {
             serde_json::from_str(&json).unwrap_or(None)
         } else {
             None
         };

        Ok(ChatMessage {
            role,
            content,
            tool_calls,
            tool_call_id, 
        })
    }).map_err(|e| e.to_string())?;

    let mut msgs = Vec::new();
    
    // Customize system prompt based on mode
    let mode_desc = if is_agent_mode {
        "You are currently in AGENT mode. You can read terminal output AND execute commands to solve problems."
    } else {
        "You are currently in ASK mode. You can read terminal output to analyze issues, but you CANNOT execute commands directly. Suggest commands to the user instead."
    };

    let full_system_prompt = format!("{}\n\n{}", SYSTEM_PROMPT, mode_desc);

    msgs.push(ChatMessage {
        role: "system".to_string(),
        content: Some(full_system_prompt),
        tool_calls: None,
        tool_call_id: None,
    });

    for row in rows {
        msgs.push(row.map_err(|e| e.to_string())?);
    }
    Ok(msgs)
}

struct ToolCallAccumulator {
    calls: HashMap<u64, (Option<String>, Option<String>, String, String)>, // index -> (id, type, name, args)
}

impl ToolCallAccumulator {
    fn new() -> Self {
        Self { calls: HashMap::new() }
    }

    fn update(&mut self, json: &serde_json::Value) {
        if let Some(arr) = json.as_array() {
            for call in arr {
                if let Some(index) = call["index"].as_u64() {
                    let entry = self.calls.entry(index).or_insert((None, None, String::new(), String::new()));
                    
                    if let Some(id) = call["id"].as_str() {
                        entry.0 = Some(id.to_string());
                    }
                    if let Some(t) = call["type"].as_str() {
                        entry.1 = Some(t.to_string());
                    }
                    if let Some(f) = call.get("function") {
                        if let Some(n) = f["name"].as_str() {
                            entry.2.push_str(n);
                        }
                        if let Some(a) = f["arguments"].as_str() {
                            entry.3.push_str(a);
                        }
                    }
                }
            }
        }
    }

    fn to_vec(&self) -> Vec<ToolCall> {
        let mut result = Vec::new();
        let mut indices: Vec<_> = self.calls.keys().collect();
        indices.sort();
        
        for index in indices {
            if let Some((Some(id), Some(t), name, args)) = self.calls.get(index) {
                result.push(ToolCall {
                    id: id.clone(),
                    tool_type: t.clone(),
                    function: FunctionCall {
                        name: name.clone(),
                        arguments: args.clone(),
                    },
                });
            }
        }
        result
    }
}

async fn execute_tools_and_save(
    state: &Arc<AppState>, 
    session_id: &str, 
    ssh_session_id: Option<&str>,
    tools: Vec<ToolCall>
) -> Result<(), String> {
    for call in tools {
        tracing::debug!("[AI] Executing tool: {} args: {}", call.function.name, call.function.arguments);
        
        let result = match call.function.name.as_str() {
            "get_terminal_output" => {
                if let Some(ssh_id) = ssh_session_id {
                    match SSHClient::get_terminal_output(ssh_id).await { 
                        Ok(text) => {
                            String::from_utf8_lossy(&strip_ansi_escapes::strip(&text)).to_string()
                        },
                        Err(e) => format!("Error: {}", e)
                    }
                } else {
                    "Error: No active terminal session linked to this chat.".to_string()
                }
            },
            "run_in_terminal" => {
                if let Some(ssh_id) = ssh_session_id {
                    if let Ok(args) = serde_json::from_str::<serde_json::Value>(&call.function.arguments) {
                        if let Some(cmd) = args["command"].as_str() {
                            let cmd_nl = format!("{}\n", cmd);
                            match SSHClient::send_input(ssh_id, cmd_nl.as_bytes()).await {
                                Ok(_) => "Command sent.".to_string(),
                                Err(e) => format!("Error sending command: {}", e)
                            }
                        } else {
                            "Error: Missing 'command' argument".to_string()
                        }
                    } else {
                        "Error: Invalid arguments JSON".to_string()
                    }
                } else {
                    "Error: No active terminal session linked to this chat.".to_string()
                }
            },
            _ => format!("Error: Unknown tool {}", call.function.name)
        };

        // Save Tool Output
        let tool_msg_id = Uuid::new_v4().to_string();
        {
            let conn = state.db_manager.get_connection();
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT INTO ai_messages (id, session_id, role, content, tool_call_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![tool_msg_id, session_id, "tool", result, call.id],
            ).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

async fn run_ai_turn(
    window: &Window,
    state: &Arc<AppState>,
    session_id: String,
    model_id: String,
    channel_id: String,
    is_agent_mode: bool,
    tools: Option<Vec<ToolDefinition>>,
) -> Result<Option<Vec<ToolCall>>, String> {
    
    // 1. Get Config
    let (endpoint, api_key, model_name, provider, proxy) = {
        let config = state.config.lock().await;
        let model = config.ai_models.iter().find(|m| m.id == model_id)
            .ok_or_else(|| "Model not found".to_string())?;
        let channel = config.ai_channels.iter().find(|c| c.id == channel_id)
            .ok_or_else(|| "Channel not found".to_string())?;
        
        let proxy = if let Some(proxy_id) = &channel.proxy_id {
            if let Some(p) = config.proxies.iter().find(|p| p.id == *proxy_id) {
                Some(p.clone())
            } else {
                tracing::warn!("[AI] Configured proxy {} not found, falling back to direct connection", proxy_id);
                None
            }
        } else {
            None
        };
        
        (
            channel.endpoint.clone().unwrap_or("https://api.openai.com/v1".to_string()),
            channel.api_key.clone().unwrap_or_default(),
            model.name.clone(),
            channel.provider.clone(),
            proxy,
        )
    };
    
    if api_key.is_empty() {
        return Err("API key is not configured".to_string());
    }

    // 2. Load History with mode awareness
    let history = load_history(state, &session_id, is_agent_mode).await?;

    // 3. Prepare headers and token if Copilot
    let mut final_api_key = api_key;
    let mut extra_headers = None;
    let mut final_endpoint = endpoint;

    let copilot_token_data; // Needed to extend lifetime if copilot is used

    if provider == "copilot" {
        // Exchange OAuth token for Session Token
        let token_resp = copilot::get_copilot_token(&final_api_key).await?;
        copilot_token_data = token_resp.token; // Copilot Session Token (tid_...)
        final_api_key = copilot_token_data.clone();
        
        final_endpoint = "https://api.githubcopilot.com".to_string(); 

        let mut headers = HashMap::new();
        headers.insert("Copilot-Integration-Id".to_string(), "vscode-chat".to_string());
        headers.insert("Editor-Version".to_string(), "vscode/1.85.1".to_string());
        headers.insert("User-Agent".to_string(), "GithubCopilot/1.155.0".to_string());
        extra_headers = Some(headers);
    }

    // 4. Stream
    let mut stream = stream_openai_chat(&final_endpoint, &final_api_key, &model_name, history, tools, extra_headers, proxy).await?;
    
    let mut full_content = String::new();
    let mut tool_accumulator = ToolCallAccumulator::new();
    let mut has_tool_calls = false;

    while let Some(event_result) = stream.next().await {
        match event_result {
            Ok(StreamEvent::Content(chunk)) => {
                if !chunk.is_empty() {
                    full_content.push_str(&chunk);
                    window.emit(&format!("ai-response-{}", session_id), chunk).map_err(|e| e.to_string())?;
                }
            },
            Ok(StreamEvent::ToolCall(json)) => {
                has_tool_calls = true;
                tool_accumulator.update(&json);
            },
            Ok(StreamEvent::Done) => {},
            Err(e) => {
                window.emit(&format!("ai-error-{}", session_id), e.clone()).map_err(|e| e.to_string())?;
                return Err(e);
            }
        }
    }

    let final_tool_calls = if has_tool_calls {
        Some(tool_accumulator.to_vec())
    } else {
        None
    };

    // Save Assistant Message
    let ai_msg_id = Uuid::new_v4().to_string();
    {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        let tool_calls_json = if let Some(calls) = &final_tool_calls {
            Some(serde_json::to_string(calls).unwrap())
        } else {
            None
        };

        // Only insert if we have content or tool calls
        if !full_content.is_empty() || final_tool_calls.is_some() {
                conn.execute(
                "INSERT INTO ai_messages (id, session_id, role, content, tool_calls) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![ai_msg_id, session_id, "assistant", full_content, tool_calls_json],
            ).map_err(|e| e.to_string())?;
        }
    }

    Ok(final_tool_calls)
}

// --- Commands ---

#[tauri::command]
pub async fn fetch_ai_models(
    state: State<'_, Arc<AppState>>,
    channel_id: String,
) -> Result<Vec<String>, String> {
    let (endpoint, api_key, provider, proxy) = {
        let config = state.config.lock().await;
        let channel = config.ai_channels.iter().find(|c| c.id == channel_id)
            .ok_or_else(|| "Channel not found".to_string())?;
        
        let proxy = if let Some(proxy_id) = &channel.proxy_id {
            if let Some(p) = config.proxies.iter().find(|p| p.id == *proxy_id) {
                Some(p.clone())
            } else {
                None
            }
        } else {
            None
        };
        
        (
            channel.endpoint.clone().unwrap_or("https://api.openai.com/v1".to_string()),
            channel.api_key.clone().unwrap_or_default(),
            channel.provider.clone(),
            proxy,
        )
    };

    if api_key.is_empty() {
        return Err("API key is not configured".to_string());
    }

    let mut final_api_key = api_key;
    let mut extra_headers = None;
    let mut final_endpoint = endpoint;

    if provider == "copilot" {
        let token_resp = copilot::get_copilot_token(&final_api_key).await?;
        final_api_key = token_resp.token;
        final_endpoint = "https://api.githubcopilot.com".to_string(); 
        
        let mut headers = HashMap::new();
        headers.insert("Copilot-Integration-Id".to_string(), "vscode-chat".to_string());
        headers.insert("Editor-Version".to_string(), "vscode/1.85.1".to_string());
        headers.insert("User-Agent".to_string(), "GithubCopilot/1.155.0".to_string());
        extra_headers = Some(headers);
    }

    let models = fetch_models(&final_endpoint, &final_api_key, extra_headers, proxy).await?;

    let mut model_ids: Vec<String> = models.into_iter().filter(|m| {
        if provider == "copilot" {
             // 1. Check capabilities.type == "chat"
             let is_chat = if let Some(cap) = m.extra.get("capabilities") {
                 cap.get("type").and_then(|v| v.as_str()) == Some("chat")
             } else {
                 // Fallback to top-level type if capabilities is missing
                 m.extra.get("type").and_then(|v| v.as_str()) == Some("chat")
             };

             if !is_chat {
                 return false;
             }

             // 2. Check model_picker_enabled == true
             if let Some(enabled) = m.extra.get("model_picker_enabled").and_then(|v| v.as_bool()) {
                 return enabled;
             }
             
             // If field is missing, default to false (strict)
             false 
        } else {
            true
        }
    }).map(|m| m.id).collect();
    
    model_ids.sort();
    Ok(model_ids)
}

#[tauri::command]
pub async fn start_copilot_auth() -> Result<copilot::DeviceCodeResponse, String> {
    copilot::start_device_auth().await
}

#[tauri::command]
pub async fn poll_copilot_auth(device_code: String) -> Result<String, String> {
    copilot::poll_access_token(&device_code).await
}

#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        
        Command::new("cmd")
            .args(["/C", "start", "", &url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

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
    load_history(&state, &session_id, false).await.map(|msgs| {
        // Filter out system and tool messages for frontend to keep UI clean
        msgs.into_iter().filter(|m| m.role != "system" && m.role != "tool").collect()
    })
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
    _ssh_session_id: Option<String>,
) -> Result<(), String> {
    tracing::debug!("[AI] send_chat_message called: session_id={}, model_id={}, mode={:?}", 
        session_id, model_id, mode);
    
    let _ = window.emit(&format!("ai-started-{}", session_id), "started");
    
    // 1. Save User Message
    let user_msg_id = Uuid::new_v4().to_string();
    {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        conn.execute(
            "INSERT INTO ai_messages (id, session_id, role, content) VALUES (?1, ?2, ?3, ?4)",
            params![user_msg_id, session_id, "user", content],
        ).map_err(|e| e.to_string())?;
    }

    let is_agent_mode = mode.as_deref() == Some("agent");
    let is_ask_mode = mode.as_deref() == Some("ask");
    
    // Both agent and ask mode now support tools
    let tools = if is_agent_mode || is_ask_mode { Some(create_tools(is_agent_mode)) } else { None };

    let tool_calls = run_ai_turn(&window, &state, session_id.clone(), model_id, channel_id, is_agent_mode, tools).await?;

    if let Some(calls) = tool_calls {
        if !calls.is_empty() {
             // Emit event for confirmation instead of auto-executing
             let _ = window.emit(&format!("ai-tool-call-{}", session_id), calls);
             // We return Ok here, leaving the frontend to handle the confirmation loop
             return Ok(());
        }
    }

    window.emit(&format!("ai-done-{}", session_id), "DONE").map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn execute_agent_tools(
    window: Window,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    model_id: String,
    channel_id: String,
    mode: Option<String>,
    ssh_session_id: Option<String>,
    tool_call_ids: Vec<String>, // List of tool call IDs to confirm execution for
) -> Result<(), String> {
    tracing::debug!("[AI] execute_agent_tools called for session {} (mode: {:?})", session_id, mode);

    let _ = window.emit(&format!("ai-started-{}", session_id), "started");

    // 1. Load the last assistant message to find the tool calls
    let is_agent_mode = mode.as_deref() == Some("agent");
    let history = load_history(&state, &session_id, is_agent_mode).await?;
    let last_msg = history.last().ok_or("No history found")?;
    
    if last_msg.role != "assistant" || last_msg.tool_calls.is_none() {
        tracing::error!("[AI] Last message was not a tool call: {:?}", last_msg);
        return Err("Last message was not an assistant tool call".to_string());
    }

    let tools_to_run = last_msg.tool_calls.as_ref().unwrap().clone();
    let tools_filtered: Vec<ToolCall> = tools_to_run.into_iter().filter(|t| tool_call_ids.contains(&t.id)).collect();

    if tools_filtered.is_empty() {
        tracing::warn!("[AI] No matching tools found to execute.");
        window.emit(&format!("ai-done-{}", session_id), "DONE").map_err(|e| e.to_string())?;
        return Ok(());
    }

    // Safety check: if NOT in agent mode, do not allow run_in_terminal
    if !is_agent_mode {
        for call in &tools_filtered {
            if call.function.name == "run_in_terminal" {
                tracing::error!("[AI] Attempted to run_in_terminal in non-agent mode!");
                return Err("Execution denied: run_in_terminal is only allowed in Agent mode.".to_string());
            }
        }
    }

    tracing::debug!("[AI] Executing {} tools...", tools_filtered.len());
    execute_tools_and_save(&state, &session_id, ssh_session_id.as_deref(), tools_filtered).await?;
    tracing::debug!("[AI] Tool execution complete. Running next AI turn...");

    // Continue the loop (1 turn)
    let tools = Some(create_tools(is_agent_mode));
    let next_tool_calls = run_ai_turn(&window, &state, session_id.clone(), model_id, channel_id, is_agent_mode, tools).await?;

    if let Some(calls) = next_tool_calls {
        if !calls.is_empty() {
             tracing::debug!("[AI] Next turn generated {} more tool calls", calls.len());
             let _ = window.emit(&format!("ai-tool-call-{}", session_id), calls);
             return Ok(());
        }
    }

    tracing::debug!("[AI] Agent turn complete.");
    window.emit(&format!("ai-done-{}", session_id), "DONE").map_err(|e| e.to_string())?;
    Ok(())
}

/// Get terminal text for AI to analyze
#[tauri::command]
pub async fn get_terminal_output(
    session_id: String,
) -> Result<String, String> {
    let text = SSHClient::get_terminal_output(&session_id).await?;
    let clean_text = String::from_utf8_lossy(&strip_ansi_escapes::strip(&text)).to_string();
    Ok(clean_text)
}

/// Run command in terminal (for AI agent mode)
#[tauri::command]
pub async fn run_in_terminal(
    session_id: String,
    command: String,
) -> Result<String, String> {
    let command_with_newline = format!("{}\n", command);
    SSHClient::send_input(&session_id, command_with_newline.as_bytes()).await?;
    Ok(format!("Command sent: {}", command))
}

/// Generate a title for an AI session based on the first conversation round
#[tauri::command]
pub async fn generate_session_title(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    model_id: String,
    channel_id: String,
) -> Result<String, String> {
    tracing::debug!("[AI] generate_session_title called for session {}", session_id);

    // 1. Check if session needs title generation
    let current_title = {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT title FROM ai_sessions WHERE id = ?1")
            .map_err(|e| e.to_string())?;
        stmt.query_row(params![session_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
    };

    // Only generate title if it's still "New Chat"
    if current_title != "New Chat" {
        tracing::debug!("[AI] Session already has a custom title: {}", current_title);
        return Ok(current_title);
    }

    // 2. Get first round of conversation (user + assistant)
    let messages = {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT role, content FROM ai_messages 
             WHERE session_id = ?1 
             ORDER BY created_at ASC 
             LIMIT 2"
        ).map_err(|e| e.to_string())?;
        
        let rows = stmt.query_map(params![session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }).map_err(|e| e.to_string())?;
        
        let mut msgs = Vec::new();
        for row in rows {
            msgs.push(row.map_err(|e| e.to_string())?);
        }
        msgs
    };

    if messages.len() < 2 {
        return Err("Not enough messages to generate title".to_string());
    }

    // 3. Get model and channel config
    let (endpoint, api_key, model_name, provider, proxy) = {
        let config = state.config.lock().await;
        let model = config.ai_models.iter().find(|m| m.id == model_id)
            .ok_or_else(|| "Model not found".to_string())?;
        let channel = config.ai_channels.iter().find(|c| c.id == channel_id)
            .ok_or_else(|| "Channel not found".to_string())?;

        let proxy = if let Some(proxy_id) = &channel.proxy_id {
            if let Some(p) = config.proxies.iter().find(|p| p.id == *proxy_id) {
                Some(p.clone())
            } else {
                tracing::warn!("[AI] Configured proxy {} not found for title generation, falling back to direct connection", proxy_id);
                None
            }
        } else {
            None
        };
        
        (
            channel.endpoint.clone().unwrap_or("https://api.openai.com/v1".to_string()),
            channel.api_key.clone().unwrap_or_default(),
            model.name.clone(),
            channel.provider.clone(),
            proxy,
        )
    };
    
    if api_key.is_empty() {
        return Err("API key is not configured".to_string());
    }

    // 4. Build prompt for title generation
    let system_prompt = "You are a helpful assistant that generates concise chat titles. \
                        Generate a short title (max 6 words) that summarizes the main topic of the conversation. \
                        Only output the title text, nothing else.";
    
    let user_content = format!(
        "Based on this conversation, generate a concise title:\n\nUser: {}\n\nAssistant: {}",
        messages.get(0).map(|(_, c)| c.as_str()).unwrap_or(""),
        messages.get(1).map(|(_, c)| c.as_str()).unwrap_or("")
    );

    let title_messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: Some(system_prompt.to_string()),
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: Some(user_content),
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    // 5. Call LLM to generate title
    let mut final_api_key = api_key;
    let mut extra_headers = None;
    let mut final_endpoint = endpoint;
    let copilot_token_data;

    if provider == "copilot" {
        let token_resp = copilot::get_copilot_token(&final_api_key).await?;
        copilot_token_data = token_resp.token;
        final_api_key = copilot_token_data.clone();
        final_endpoint = "https://api.githubcopilot.com".to_string(); 
        
        let mut headers = HashMap::new();
        headers.insert("Copilot-Integration-Id".to_string(), "vscode-chat".to_string());
        headers.insert("Editor-Version".to_string(), "vscode/1.85.1".to_string());
        headers.insert("User-Agent".to_string(), "GithubCopilot/1.155.0".to_string());
        extra_headers = Some(headers);
    }

    let mut stream = stream_openai_chat(&final_endpoint, &final_api_key, &model_name, title_messages, None, extra_headers, proxy).await?;
    let mut title = String::new();
    
    while let Some(event_result) = stream.next().await {
        match event_result {
            Ok(StreamEvent::Content(chunk)) => {
                title.push_str(&chunk);
            },
            Ok(StreamEvent::Done) => break,
            Err(e) => return Err(e),
            _ => {}
        }
    }

    // Clean up title
    let title = title.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();

    let title = if title.len() > 50 {
        format!("{}...", &title[..47])
    } else {
        title
    };

    if title.is_empty() {
        return Err("Failed to generate title".to_string());
    }

    // 6. Update session title in database
    {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        conn.execute(
            "UPDATE ai_sessions SET title = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
            params![title, session_id],
        ).map_err(|e| e.to_string())?;
    }

    tracing::debug!("[AI] Generated title for session {}: {}", session_id, title);
    Ok(title)
}

#[tauri::command]
pub async fn delete_ai_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    tracing::debug!("[AI] delete_ai_session called for session {}", session_id);
    let conn = state.db_manager.get_connection();
    let conn = conn.lock().unwrap();
    
    conn.execute(
        "DELETE FROM ai_messages WHERE session_id = ?1",
        params![session_id],
    ).map_err(|e| e.to_string())?;

    conn.execute(
        "DELETE FROM ai_sessions WHERE id = ?1",
        params![session_id],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn delete_all_ai_sessions(
    state: State<'_, Arc<AppState>>,
    server_id: String,
) -> Result<(), String> {
    tracing::debug!("[AI] delete_all_ai_sessions called for server {}", server_id);
    let conn = state.db_manager.get_connection();
    let conn = conn.lock().unwrap();
    
    conn.execute(
        "DELETE FROM ai_messages WHERE session_id IN (SELECT id FROM ai_sessions WHERE server_id = ?1)",
        params![server_id],
    ).map_err(|e| e.to_string())?;

    conn.execute(
        "DELETE FROM ai_sessions WHERE server_id = ?1",
        params![server_id],
    ).map_err(|e| e.to_string())?;

    Ok(())
}
