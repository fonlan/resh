use crate::ai::prompts::SYSTEM_PROMPT;
use crate::ai::validator::{validate_and_fix_messages, MessagePayload, ToolCallPayload, ToolCallFunction};
use crate::commands::AppState;
use crate::ssh_manager::ssh::SSHClient;
use futures::StreamExt;
use rusqlite::params;
use std::sync::Arc;
use tauri::{Emitter, State, Window};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use serde::{Serialize, Deserialize};
use genai::chat::{ChatMessage as GenaiMessage, Tool, ChatStreamEvent};
use reqwest::Client;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

fn extract_timeout(arguments: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()?
        .get("timeoutSeconds")
        .and_then(|v| v.as_u64())
}

fn extract_command(arguments: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()?
        .get("command")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub fn create_tools(is_agent_mode: bool) -> Vec<ToolDefinition> {
    let mut tools = vec![
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "get_terminal_output".to_string(),
                description: "Get the current terminal output text to analyze errors, command results, or system state.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        },
    ];

    if is_agent_mode {
        tools.push(ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "run_in_terminal".to_string(),
                description: "Execute a command in the terminal. Waits for command to complete or timeout, then returns the command output. Use this to fix issues, install packages, or perform system operations.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        },
                        "timeoutSeconds": {
                            "type": "integer",
                            "description": "Timeout in seconds (default: 30). Maximum time to wait for command completion."
                        }
                    },
                    "required": ["command"]
                }),
            },
        });

        tools.push(ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "send_interrupt".to_string(),
                description: "Send Ctrl+C (ETX, character code 3) to interrupt a running program. Use this when a TUI program (like htop, vim, less, iftop) is blocking and needs to be terminated. Returns confirmation of the interrupt being sent.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        });

        tools.push(ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "send_terminal_input".to_string(),
                description: "Send arbitrary characters or escape sequences to the terminal. Use this to send key presses like 'q' to quit a TUI program, or special keys like escape sequences. Useful for dismissing prompts or navigating TUI applications.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": {
                            "type": "string",
                            "description": "The characters or escape sequence to send (e.g., 'q' to quit, '\\x1b' for Escape, '\\n' for Enter)"
                        }
                    },
                    "required": ["input"]
                }),
            },
        });
    }

    tools
}

fn to_genai_messages(history: Vec<ChatMessage>) -> Vec<GenaiMessage> {
    history.into_iter().map(|msg| {
        match msg.role.as_str() {
            "system" => GenaiMessage::system(msg.content.unwrap_or_default()),
            "user" => GenaiMessage::user(msg.content.unwrap_or_default()),
            "assistant" => {
                let content = msg.content.unwrap_or_default();
                if let Some(tool_calls) = msg.tool_calls {
                    let genai_tool_calls: Vec<genai::chat::ToolCall> = tool_calls.into_iter().map(|tc| {
                        genai::chat::ToolCall {
                            call_id: tc.id,
                            fn_name: tc.function.name,
                            fn_arguments: serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null),
                            thought_signatures: None,
                        }
                    }).collect();
                    
                    if content.is_empty() {
                        GenaiMessage::assistant(genai_tool_calls)
                    } else {
                        GenaiMessage::assistant(genai_tool_calls)
                    }
                } else {
                    GenaiMessage::assistant(content)
                }
            },
            "tool" => {
                GenaiMessage::from(genai::chat::ToolResponse::new(
                    msg.tool_call_id.unwrap_or_default(),
                    msg.content.unwrap_or_default()
                ))
            },
            _ => GenaiMessage::user(msg.content.unwrap_or_default()),
        }
    }).collect()
}

async fn load_history(
    state: &Arc<AppState>,
    session_id: &str,
    is_agent_mode: bool,
    ssh_session_id: Option<&str>,
) -> Result<Vec<ChatMessage>, String> {
    let max_history = {
        let config = state.config.lock().await;
        config.general.ai_max_history as usize
    };

    let dialog_messages: Vec<ChatMessage> = {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();

        // 加载所有非system消息，按时间排序
        let mut stmt = conn
            .prepare(
                "SELECT role, content, reasoning_content, tool_calls, tool_call_id, created_at 
                FROM ai_messages 
                WHERE session_id = ?1 
                ORDER BY created_at ASC",
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map(params![session_id], |row| {
                let role: String = row.get(0)?;
                let content_raw: String = row.get(1)?;
                let reasoning_raw: Option<String> = row.get(2)?;
                let tool_calls_json: Option<String> = row.get(3).ok();
                let tool_call_id: Option<String> = row.get(4).ok();

                let content = if content_raw.is_empty() {
                    None
                } else {
                    Some(content_raw)
                };

                let tool_calls = if let Some(json) = tool_calls_json {
                    serde_json::from_str(&json).unwrap_or(None)
                } else {
                    None
                };

                Ok(ChatMessage {
                    role,
                    content,
                    reasoning_content: reasoning_raw,
                    tool_calls,
                    tool_call_id,
                })
            })
            .map_err(|e| e.to_string())?;

        // 收集所有消息
        let mut all_messages: Vec<ChatMessage> = Vec::new();
        for row in rows {
            let msg: ChatMessage = row.map_err(|e| e.to_string())?;
            all_messages.push(msg);
        }

        // 分离system消息和对话消息
        let dialog_messages: Vec<ChatMessage> = all_messages
            .into_iter()
            .filter(|m| m.role != "system")
            .collect();

        // 从最早的消息开始删除超出max_history的部分
        if dialog_messages.len() > max_history {
            dialog_messages[dialog_messages.len() - max_history..].to_vec()
        } else {
            dialog_messages
        }
    };

    let mut messages: Vec<ChatMessage> = Vec::new();

    let mode_desc = if is_agent_mode {
        "You are currently in AGENT mode. You can read terminal output AND execute commands to solve problems."
    } else {
        "You are currently in ASK mode. You can read terminal output to analyze issues, but you CANNOT execute commands directly. Suggest commands to the user instead."
    };

    let mut system_context = String::new();
    if let Some(ssh_id) = ssh_session_id {
        if let Some(info) = SSHClient::get_system_info(ssh_id).await {
            system_context = format!(
                "\n\nCurrent System Context:\n- OS: {}\n- Distro: {}\n- User: {}\n- Shell: {}\n- IP: {}",
                info.os, info.distro, info.username, info.shell, info.ip
            );
        }
    }

    let full_system_prompt = format!("{}\n\n{}{}", SYSTEM_PROMPT, mode_desc, system_context);

    messages.push(ChatMessage {
        role: "system".to_string(),
        content: Some(full_system_prompt),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
    });

    // 添加所有对话消息（已经在数据库中按正确顺序存储）
    messages.extend(dialog_messages);

    // 验证并修复消息序列
    let mut payload_messages: Vec<MessagePayload> = messages
        .iter()
        .map(|m| MessagePayload {
            role: m.role.clone(),
            content: m.content.clone(),
            reasoning_content: m.reasoning_content.clone(),
            tool_calls: m.tool_calls.as_ref().map(|calls| {
                calls.iter()
                    .map(|c| ToolCallPayload {
                        id: c.id.clone(),
                        function: ToolCallFunction {
                            name: c.function.name.clone(),
                            arguments: c.function.arguments.clone(),
                        },
                    })
                    .collect()
            }),
            tool_call_id: m.tool_call_id.clone(),
        })
        .collect();

    let result = validate_and_fix_messages(&mut payload_messages);
    if result.was_fixed {
        tracing::warn!("[AI] Message sequence fixed: {:?}", result.fixes);
    }

    // 转换回ChatMessage格式
    let validated_messages: Vec<ChatMessage> = payload_messages
        .into_iter()
        .map(|m| ChatMessage {
            role: m.role,
            content: m.content,
            reasoning_content: m.reasoning_content,
            tool_calls: m.tool_calls.map(|calls| {
                calls.iter()
                    .map(|c| ToolCall {
                        id: c.id.clone(),
                        tool_type: "function".to_string(),
                        function: FunctionCall {
                            name: c.function.name.clone(),
                            arguments: c.function.arguments.clone(),
                        },
                    })
                    .collect()
            }),
            tool_call_id: m.tool_call_id,
        })
        .collect();

    Ok(validated_messages)
}

async fn execute_tools_and_save(
    state: &Arc<AppState>,
    session_id: &str,
    ssh_session_id: Option<&str>,
    tools: Vec<ToolCall>,
    cancellation_token: CancellationToken,
) -> Result<(), String> {
    for call in tools {
        if cancellation_token.is_cancelled() {
            return Err("CANCELLED".to_string());
        }

        let result = match call.function.name.as_str() {
            "get_terminal_output" => {
                if let Some(ssh_id) = ssh_session_id {
                    match SSHClient::get_terminal_output(ssh_id).await {
                        Ok(text) => {
                            String::from_utf8_lossy(&strip_ansi_escapes::strip(&text)).to_string()
                        }
                        Err(e) => format!("Error: {}", e),
                    }
                } else {
                    "Error: No active terminal session linked to this chat.".to_string()
                }
            }
            "run_in_terminal" => {
                if let Some(ssh_id) = ssh_session_id {
                    if let Ok(args) =
                        serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                    {
                        if let Some(cmd) = args["command"].as_str() {
                            let timeout = args
                                .get("timeoutSeconds")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(30);

                            SSHClient::start_command_recording(ssh_id).await?;
                            tracing::debug!("[execute_tools] Command '{}' sent, timeout={}s", cmd, timeout);

                            // Chain command with marker output using semicolon
                            // This ensures marker is output AFTER command completes
                            let cmd_with_marker = format!("{}; printf '\x1b\x1b\n'\n", cmd);
                            if let Err(_e) = SSHClient::send_input(ssh_id, cmd_with_marker.as_bytes()).await {
                                let _ = SSHClient::stop_command_recording(ssh_id).await;
                                break;
                            }

                            let mut interval =
                                tokio::time::interval(tokio::time::Duration::from_millis(100));
                            let mut elapsed = 0u64;
                            let timeout_ms = timeout * 1000;

                            loop {
                                if cancellation_token.is_cancelled() {
                                    SSHClient::stop_command_recording(ssh_id).await.ok();
                                    return Err("CANCELLED".to_string());
                                }
                                interval.tick().await;
                                elapsed += 100;

                                if elapsed >= timeout_ms {
                                    tracing::warn!("[execute_tools] Timeout reached at {}ms for command '{}'", elapsed, cmd);
                                    break;
                                }

                                let is_completed =
                                    SSHClient::check_command_completed(ssh_id).await?;
                                if is_completed {
                                    tracing::debug!("[execute_tools] Command '{}' completed at {}ms", cmd, elapsed);
                                    break;
                                }
                            }

                            match SSHClient::stop_command_recording(ssh_id).await {
                                Ok(output) => {
                                    let clean_output = String::from_utf8_lossy(
                                        &strip_ansi_escapes::strip(output.as_bytes()),
                                    )
                                    .to_string();
                                    if clean_output.is_empty() {
                                        "Command timed out or produced no output".to_string()
                                    } else {
                                        clean_output
                                    }
                                }
                                Err(e) => format!("Error getting output: {}", e),
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
            }
            "send_interrupt" => {
                if let Some(ssh_id) = ssh_session_id {
                    match SSHClient::send_interrupt(ssh_id).await {
                        Ok(_) => "Interrupt signal (Ctrl+C) sent successfully".to_string(),
                        Err(e) => format!("Error sending interrupt: {}", e),
                    }
                } else {
                    "Error: No active terminal session linked to this chat.".to_string()
                }
            }
            "send_terminal_input" => {
                if let Some(ssh_id) = ssh_session_id {
                    if let Ok(args) =
                        serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                    {
                        if let Some(input) = args["input"].as_str() {
                            match SSHClient::send_terminal_input(ssh_id, input).await {
                                Ok(_) => {
                                    format!("Input '{}' sent successfully", input.escape_debug())
                                }
                                Err(e) => format!("Error sending input: {}", e),
                            }
                        } else {
                            "Error: Missing 'input' argument".to_string()
                        }
                    } else {
                        "Error: Invalid arguments JSON".to_string()
                    }
                } else {
                    "Error: No active terminal session linked to this chat.".to_string()
                }
            }
            _ => format!("Error: Unknown tool {}", call.function.name),
        };

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

pub fn run_ai_turn(
    window: Window,
    state: Arc<AppState>,
    session_id: String,
    model_id: String,
    channel_id: String,
    is_agent_mode: bool,
    tools: Option<Vec<ToolDefinition>>,
    cancellation_token: CancellationToken,
    ssh_session_id: Option<String>,
) -> futures::future::BoxFuture<'static, Result<Option<Vec<ToolCall>>, String>> {
    Box::pin(async move {
        let (channel, model, proxy) = {
            let config = state.config.lock().await;
            let model = config.ai_models.iter().find(|m| m.id == model_id).cloned().ok_or("Model not found")?;
            let channel = config.ai_channels.iter().find(|c| c.id == channel_id).cloned().ok_or("Channel not found")?;
            let proxy = channel.proxy_id.as_ref().and_then(|id| config.proxies.iter().find(|p| &p.id == id).cloned());
            (channel, model, proxy)
        };

        let history: Vec<ChatMessage> = load_history(&state, &session_id, is_agent_mode, ssh_session_id.as_deref()).await?;
        let genai_history = to_genai_messages(history);

        let genai_tools: Option<Vec<Tool>> = tools.as_ref().map(|ts| {
            ts.iter().map(|t| {
                Tool::new(t.function.name.clone())
                    .with_description(t.function.description.clone())
                    .with_schema(t.function.parameters.clone())
            }).collect()
        });

        let mut stream = state.ai_manager.stream_chat(
            &channel,
            &model,
            genai_history,
            genai_tools,
            proxy
        ).await?;

        let mut full_content = String::new();
        let mut full_reasoning = String::new();
        let mut final_tool_calls: Option<Vec<ToolCall>> = None;
        let mut accumulated_tool_calls: Vec<ToolCall> = Vec::new();
        let mut has_pending_reasoning = false;

        while let Some(event_result) = stream.next().await {
            if cancellation_token.is_cancelled() {
                return Err("CANCELLED".to_string());
            }
            match event_result {
                Ok(event) => {
                    match event {
                        ChatStreamEvent::Chunk(chunk) => {
                            // 如果之前有思考内容，先发送结束标记
                            if has_pending_reasoning {
                                window.emit(&format!("ai-reasoning-end-{}", session_id), "end").map_err(|e| e.to_string())?;
                                has_pending_reasoning = false;
                            }
                            full_content.push_str(&chunk.content);
                            window.emit(&format!("ai-response-{}", session_id), chunk.content).map_err(|e| e.to_string())?;
                        }
                        ChatStreamEvent::ReasoningChunk(chunk) => {
                            has_pending_reasoning = true;
                            full_reasoning.push_str(&chunk.content);
                            window.emit(&format!("ai-reasoning-{}", session_id), chunk.content).map_err(|e| e.to_string())?;
                        }
                        ChatStreamEvent::ToolCallChunk(chunk) => {
                            // 累积工具调用 chunk（用于某些提供商，End 事件可能没有 captured_content）
                            tracing::debug!("[AI] ToolCallChunk raw: call_id={}, fn_name={}, fn_arguments={:?}",
                                chunk.tool_call.call_id, chunk.tool_call.fn_name, chunk.tool_call.fn_arguments);

                            let args = chunk.tool_call.fn_arguments.as_str()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| chunk.tool_call.fn_arguments.to_string());

                            tracing::debug!("[AI] ToolCallChunk extracted args: \"{}\"", args);

                            // 查找是否已有相同 call_id 的工具调用
                            if let Some(existing) = accumulated_tool_calls.iter_mut().find(|tc| tc.id == chunk.tool_call.call_id) {
                                // 累积 arguments（某些流式响应会分块发送参数）
                                existing.function.arguments.push_str(&args);

                                // 如果之前没有 fn_name，现在有了，就更新
                                if existing.function.name.is_empty() && !chunk.tool_call.fn_name.is_empty() {
                                    existing.function.name = chunk.tool_call.fn_name.clone();
                                }

                                tracing::debug!("[AI] ToolCallChunk (append by id): id={}, name={}, args_len={}, total_len={}",
                                    chunk.tool_call.call_id, existing.function.name, args.len(), existing.function.arguments.len());
                            } else if chunk.tool_call.call_id == "call_0" && accumulated_tool_calls.len() == 1 {
                                // 处理 call_id 变化的情况（如 DeepSeek）：
                                // 如果 call_id 是 "call_0" 且只有一个工具调用在进行中，则累积到那个工具调用
                                let existing = &mut accumulated_tool_calls[0];
                                existing.function.arguments.push_str(&args);

                                if existing.function.name.is_empty() && !chunk.tool_call.fn_name.is_empty() {
                                    existing.function.name = chunk.tool_call.fn_name.clone();
                                }

                                tracing::debug!("[AI] ToolCallChunk (append by fallback): original_id={}, fallback_id={}, name={}, args_len={}, total_len={}",
                                    existing.id, chunk.tool_call.call_id, existing.function.name, args.len(), existing.function.arguments.len());
                            } else if !chunk.tool_call.fn_name.is_empty() || !args.is_empty() {
                                // 新工具调用（只要有 fn_name 或 arguments 就创建）
                                accumulated_tool_calls.push(ToolCall {
                                    id: chunk.tool_call.call_id.clone(),
                                    tool_type: "function".to_string(),
                                    function: FunctionCall {
                                        name: chunk.tool_call.fn_name.clone(),
                                        arguments: args.clone(),
                                    }
                                });
                                tracing::debug!("[AI] ToolCallChunk (new): id={}, name={}, args=\"{}\"",
                                    chunk.tool_call.call_id, chunk.tool_call.fn_name, args);
                            } else {
                                tracing::debug!("[AI] ToolCallChunk (skipped): empty fn_name and args");
                            }
                        }
                        ChatStreamEvent::End(end) => {
                            // 发送思考结束标记
                            if has_pending_reasoning {
                                window.emit(&format!("ai-reasoning-end-{}", session_id), "end").map_err(|e| e.to_string())?;
                                has_pending_reasoning = false;
                            }

                            // 优先使用 End 事件中的完整 tool_calls，但如果为空则回退到累积的 tool_calls
                            let tool_calls = if let Some(content) = end.captured_content {
                                let raw_tool_calls = content.tool_calls();
                                tracing::debug!("[AI] End event captured_content tool_calls count: {}", raw_tool_calls.len());

                                let calls_from_captured: Vec<ToolCall> = raw_tool_calls
                                    .into_iter()
                                    .filter(|tc| !tc.fn_name.is_empty())
                                    .map(|tc| {
                                        let args = tc.fn_arguments.as_str()
                                            .map(|s| s.to_string())
                                            .unwrap_or_else(|| tc.fn_arguments.to_string());
                                        ToolCall {
                                            id: tc.call_id.clone(),
                                            tool_type: "function".to_string(),
                                            function: FunctionCall {
                                                name: tc.fn_name.clone(),
                                                arguments: args,
                                            }
                                        }
                                    })
                                    .collect();

                                // 如果 captured_content 的 tool_calls 为空，回退到累积的 tool_calls
                                if calls_from_captured.is_empty() && !accumulated_tool_calls.is_empty() {
                                    tracing::debug!("[AI] captured_content tool_calls is empty, fallback to accumulated tool_calls: {}", accumulated_tool_calls.len());
                                    for acc_call in &accumulated_tool_calls {
                                        tracing::debug!("[AI] Accumulated tool call: id={}, name={}, args=\"{}\"",
                                            acc_call.id, acc_call.function.name, acc_call.function.arguments);
                                    }
                                    accumulated_tool_calls.clone()
                                } else {
                                    calls_from_captured
                                }
                            } else {
                                // 如果 End 事件没有 captured_content，则使用累积的 tool_calls
                                tracing::debug!("[AI] End event has no captured_content, using accumulated tool_calls: {}", accumulated_tool_calls.len());
                                for acc_call in &accumulated_tool_calls {
                                    tracing::debug!("[AI] Accumulated tool call: id={}, name={}, args=\"{}\"",
                                        acc_call.id, acc_call.function.name, acc_call.function.arguments);
                                }
                                accumulated_tool_calls.clone()
                            };

                            // 记录所有工具调用的详细信息
                            for call in &tool_calls {
                                let timeout = extract_timeout(&call.function.arguments);
                                let command = extract_command(&call.function.arguments);
                                tracing::info!(
                                    "[AI] Tool call received: id={}, name={}, command=\"{}\", timeout={}s, raw_args=\"{}\"",
                                    call.id,
                                    call.function.name,
                                    command.as_deref().unwrap_or("N/A"),
                                    timeout.unwrap_or(30),
                                    call.function.arguments
                                );
                            }

                            if !tool_calls.is_empty() {
                                *final_tool_calls.get_or_insert_with(Vec::new) = tool_calls;
                            }
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    // 发送思考结束标记（如果有的话）
                    if has_pending_reasoning {
                        window.emit(&format!("ai-reasoning-end-{}", session_id), "end").ok();
                    }
                    let err_msg = e.to_string();
                    window.emit(&format!("ai-error-{}", session_id), err_msg.clone()).map_err(|e| e.to_string())?;
                    return Err(err_msg);
                }
            }
        }

        // 流结束后也发送思考结束标记（以防万一）
        if has_pending_reasoning {
            window.emit(&format!("ai-reasoning-end-{}", session_id), "end").ok();
        }

        let ai_msg_id = Uuid::new_v4().to_string();
        {
            let conn = state.db_manager.get_connection();
            let conn = conn.lock().unwrap();
            let tool_calls_json = if let Some(calls) = &final_tool_calls {
                Some(serde_json::to_string(calls).map_err(|e| format!("序列化失败: {}", e))?)
            } else {
                None
            };

            if !full_content.is_empty() || final_tool_calls.is_some() || !full_reasoning.is_empty() {
                conn.execute(
                    "INSERT INTO ai_messages (id, session_id, role, content, reasoning_content, tool_calls) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![ai_msg_id, session_id, "assistant", full_content, full_reasoning, tool_calls_json],
                ).map_err(|e| e.to_string())?;
            }
        }

        if let Some(calls) = &final_tool_calls {
            if !calls.is_empty() {
                // 分类工具：get_terminal_output 自动执行，其他工具需要确认
                let auto_exec_calls: Vec<ToolCall> = calls.iter()
                    .filter(|c| c.function.name == "get_terminal_output")
                    .cloned()
                    .collect();

                let confirm_calls: Vec<ToolCall> = calls.iter()
                    .filter(|c| c.function.name != "get_terminal_output")
                    .cloned()
                    .collect();

                // 自动执行 get_terminal_output 工具（Ask 和 Agent 模式都支持）
                if !auto_exec_calls.is_empty() {
                    tracing::info!("[AI] Auto-executing {} get_terminal_output tools", auto_exec_calls.len());
                    execute_tools_and_save(
                        &state,
                        &session_id,
                        ssh_session_id.as_deref(),
                        auto_exec_calls,
                        cancellation_token.clone(),
                    ).await?;
                }

                // 需要确认的工具（仅在 Agent 模式下允许）
                if !confirm_calls.is_empty() {
                    if is_agent_mode {
                        tracing::info!("[AI] {} tools need confirmation, emitting for frontend countdown", confirm_calls.len());
                        // 发送完整的tool_calls到前端（完整接收后才发送）
                        window.emit(&format!("ai-tool-call-{}", session_id), confirm_calls.clone())
                            .map_err(|e| e.to_string())?;
                        return Ok(Some(confirm_calls));
                    } else {
                        // Ask 模式下不允许执行除 get_terminal_output 以外的工具
                        tracing::warn!("[AI] Ask mode cannot execute tools other than get_terminal_output, ignoring {} tool calls", confirm_calls.len());
                    }
                }

                // 继续 AI 轮次
                return run_ai_turn(
                    window,
                    state,
                    session_id,
                    model_id,
                    channel_id,
                    is_agent_mode,
                    tools,
                    cancellation_token,
                    ssh_session_id,
                ).await;
            }
        }

        Ok(final_tool_calls)
    })
}

#[tauri::command]
pub async fn cancel_ai_chat(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    if let Some(token) = state.ai_cancellation_tokens.get(&session_id) {
        token.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn fetch_ai_models(
    state: State<'_, Arc<AppState>>,
    channel_id: String,
) -> Result<Vec<String>, String> {
    let (channel, proxy) = {
        let config = state.config.lock().await;
        let channel = config
            .ai_channels
            .iter()
            .find(|c| c.id == channel_id)
            .ok_or_else(|| "Channel not found".to_string())?
            .clone();

        let proxy = if let Some(proxy_id) = &channel.proxy_id {
            config.proxies.iter().find(|p| p.id == *proxy_id).cloned()
        } else {
            None
        };

        (channel, proxy)
    };

    state.ai_manager.fetch_models(&channel, proxy).await
}

const CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccessTokenResponse {
    pub access_token: Option<String>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[tauri::command]
pub async fn start_copilot_auth() -> Result<DeviceCodeResponse, String> {
    let client = Client::new();
    let res = client.post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": CLIENT_ID,
            "scope": "read:user"
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err(format!("Failed to request device code: {}", res.status()));
    }

    let body = res.json::<DeviceCodeResponse>().await.map_err(|e| e.to_string())?;
    Ok(body)
}

#[tauri::command]
pub async fn poll_copilot_auth(device_code: String) -> Result<String, String> {
    let client = Client::new();
    let res = client.post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": CLIENT_ID,
            "device_code": device_code,
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !res.status().is_success() {
        return Err(format!("Poll failed: {}", res.status()));
    }

    let body = res.json::<AccessTokenResponse>().await.map_err(|e| e.to_string())?;

    if let Some(error) = body.error {
        if error == "authorization_pending" {
            return Err("pending".to_string());
        } else if error == "slow_down" {
            return Err("slow_down".to_string());
        } else {
            return Err(format!("Auth error: {} - {}", error, body.error_description.unwrap_or_default()));
        }
    }

    if let Some(token) = body.access_token {
        Ok(token)
    } else {
        Err("No access token in response".to_string())
    }
}

#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
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
    ssh_session_id: Option<String>,
) -> Result<String, String> {
    let id = Uuid::new_v4().to_string();
    let conn = state.db_manager.get_connection();
    let conn = conn.lock().unwrap();

    conn.execute(
        "INSERT INTO ai_sessions (id, server_id, title, model_id, ssh_session_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, server_id, "New Chat", model_id, ssh_session_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(id)
}

#[tauri::command]
pub async fn get_ai_sessions(
    state: State<'_, Arc<AppState>>,
    server_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = state.db_manager.get_connection();
    let conn = conn.lock().unwrap();

    let mut stmt = conn
        .prepare(
            "SELECT id, title, created_at, model_id, ssh_session_id FROM ai_sessions 
         WHERE server_id = ?1 
         AND EXISTS (SELECT 1 FROM ai_messages WHERE session_id = ai_sessions.id)
         ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![server_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "title": row.get::<_, String>(1)?,
                "createdAt": row.get::<_, String>(2)?,
                "modelId": row.get::<_, Option<String>>(3)?,
                "sshSessionId": row.get::<_, Option<String>>(4)?,
            }))
        })
        .map_err(|e| e.to_string())?;

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
    load_history(&state, &session_id, false, None).await.map(|msgs: Vec<ChatMessage>| {
        msgs.into_iter()
            .filter(|m| m.role != "system" && m.role != "tool")
            .collect()
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
    mode: Option<String>,
    ssh_session_id: Option<String>,
) -> Result<(), String> {
    let _ = window.emit(&format!("ai-started-{}", session_id), "started");

    // 绑定 SSH 会话 ID 到 AI 会话（如果尚未绑定）
    let bound_ssh_session_id = {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        
        let existing_ssh_id: Option<String> = conn.query_row(
            "SELECT ssh_session_id FROM ai_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        ).ok();

        if existing_ssh_id.is_none() || existing_ssh_id.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
            if let Some(ref new_id) = ssh_session_id {
                if !new_id.is_empty() {
                    conn.execute(
                        "UPDATE ai_sessions SET ssh_session_id = ?1 WHERE id = ?2",
                        params![new_id, session_id],
                    ).ok();
                    ssh_session_id.clone()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            existing_ssh_id
        }
    };

    let user_msg_id = Uuid::new_v4().to_string();
    {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        conn.execute(
            "INSERT INTO ai_messages (id, session_id, role, content) VALUES (?1, ?2, ?3, ?4)",
            params![user_msg_id, session_id, "user", content],
        )
        .map_err(|e| e.to_string())?;
    }

    let is_agent_mode = mode.as_deref() == Some("agent");
    let is_ask_mode = mode.as_deref() == Some("ask");

    let tools = if is_agent_mode || is_ask_mode {
        Some(create_tools(is_agent_mode))
    } else {
        None
    };

    let token = CancellationToken::new();
    state
        .ai_cancellation_tokens
        .insert(session_id.clone(), token.clone());

    let result: Result<Option<Vec<ToolCall>>, String> = run_ai_turn(
        window.clone(),
        state.inner().clone(),
        session_id.clone(),
        model_id,
        channel_id,
        is_agent_mode,
        tools,
        token,
        bound_ssh_session_id,
    )
    .await;

    state.ai_cancellation_tokens.remove(&session_id);

    match result {
        Ok(tool_calls) => {
            if let Some(calls) = tool_calls {
                if !calls.is_empty() {
                    let _ = window.emit(&format!("ai-tool-call-{}", session_id), calls);
                    return Ok(());
                }
            }
            window
                .emit(&format!("ai-done-{}", session_id), "DONE")
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        Err(e) => {
            tracing::error!("[AI] send_chat_message error: {}", e);
            window.emit(&format!("ai-error-{}", session_id), e.clone()).ok();
            Err(e)
        }
    }
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
    tool_call_ids: Vec<String>,
) -> Result<(), String> {
    let _ = window.emit(&format!("ai-started-{}", session_id), "started");

    // 获取绑定的 SSH 会话 ID
    let bound_ssh_session_id = {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        
        let existing_ssh_id: Option<String> = conn.query_row(
            "SELECT ssh_session_id FROM ai_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        ).ok();

        if existing_ssh_id.is_none() || existing_ssh_id.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
            ssh_session_id.clone()
        } else {
            existing_ssh_id
        }
    };

    let is_agent_mode = mode.as_deref() == Some("agent");

    let _provider = {
        let config = state.config.lock().await;
        config
            .ai_channels
            .iter()
            .find(|c| c.id == channel_id)
            .map(|c| c.provider.clone())
            .unwrap_or_else(|| "openai".to_string())
    };

    let history: Vec<ChatMessage> = load_history(&state, &session_id, is_agent_mode, bound_ssh_session_id.as_deref()).await?;
    let last_msg = history.last().ok_or("No history found")?;

    if last_msg.role != "assistant" || last_msg.tool_calls.is_none() {
        return Err("Last message was not an assistant tool call".to_string());
    }

    let tools_to_run = last_msg.tool_calls.as_ref().unwrap().clone();
    let tools_filtered: Vec<ToolCall> = tools_to_run
        .into_iter()
        .filter(|t| tool_call_ids.contains(&t.id))
        .collect();

    if tools_filtered.is_empty() {
        window
            .emit(&format!("ai-done-{}", session_id), "DONE")
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    if !is_agent_mode {
        for call in &tools_filtered {
            if call.function.name == "run_in_terminal" {
                return Err(
                    "Execution denied: run_in_terminal is only allowed in Agent mode.".to_string(),
                );
            }
        }
    }

    let token = CancellationToken::new();
    state
        .ai_cancellation_tokens
        .insert(session_id.clone(), token.clone());

    let exec_res = execute_tools_and_save(
        &state,
        &session_id,
        bound_ssh_session_id.as_deref(),
        tools_filtered,
        token.clone(),
    )
    .await;

    if let Err(e) = exec_res {
        state.ai_cancellation_tokens.remove(&session_id);
        return Err(e);
    }

    let tools = Some(create_tools(is_agent_mode));

    let result: Result<Option<Vec<ToolCall>>, String> = run_ai_turn(
        window.clone(),
        state.inner().clone(),
        session_id.clone(),
        model_id,
        channel_id,
        is_agent_mode,
        tools,
        token,
        bound_ssh_session_id,
    )
    .await;

    state.ai_cancellation_tokens.remove(&session_id);

    match result {
        Ok(next_tool_calls) => {
            if let Some(calls) = next_tool_calls {
                if !calls.is_empty() {
                    let _ = window.emit(&format!("ai-tool-call-{}", session_id), calls);
                    return Ok(());
                }
            }
            window
                .emit(&format!("ai-done-{}", session_id), "DONE")
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        Err(e) => {
            tracing::error!("[AI] execute_agent_tools error: {}", e);
            window.emit(&format!("ai-error-{}", session_id), e.clone()).ok();
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn get_terminal_output(session_id: String) -> Result<String, String> {
    let text = SSHClient::get_terminal_output(&session_id).await?;
    let clean_text = String::from_utf8_lossy(&strip_ansi_escapes::strip(&text)).to_string();
    Ok(clean_text)
}

#[tauri::command]
pub async fn run_in_terminal(
    session_id: String,
    command: String,
    timeout_seconds: Option<u64>,
) -> Result<String, String> {
    let timeout = timeout_seconds.unwrap_or(30);
    let timeout_ms = timeout * 1000;

    SSHClient::start_command_recording(&session_id).await?;

    // Chain command with marker output using semicolon
    let cmd_with_marker = format!("{}; printf '\x1b\x1b\n'\n", command);
    if let Err(e) = SSHClient::send_input(&session_id, cmd_with_marker.as_bytes()).await {
        let _ = SSHClient::stop_command_recording(&session_id).await;
        return Err(format!("Failed to send command: {}", e));
    }

    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));
    let mut elapsed = 0u64;

    tracing::debug!("[run_in_terminal] Starting countdown, timeout={}ms", timeout_ms);

    loop {
        interval.tick().await;
        elapsed += 100;

        tracing::debug!("[run_in_terminal] Elapsed={}ms, checking...", elapsed);

        if elapsed >= timeout_ms {
            tracing::warn!("[run_in_terminal] Timeout reached ({}ms), sending interrupt", elapsed);
            let _ = SSHClient::send_interrupt(&session_id).await;
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            break;
        }

        match SSHClient::check_command_completed(&session_id).await {
            Ok(true) => {
                tracing::debug!("[run_in_terminal] Command completed at {}ms", elapsed);
                break;
            }
            Ok(false) => {
                tracing::debug!("[run_in_terminal] Command not completed yet, continuing...");
                continue;
            }
            Err(e) => {
                tracing::warn!("[run_in_terminal] check_command_completed error: {}, continuing...", e);
                continue;
            }
        }
    }

    let output = SSHClient::stop_command_recording(&session_id).await?;
    let clean_output = String::from_utf8_lossy(&strip_ansi_escapes::strip(&output.as_bytes())).to_string();

    if clean_output.trim().is_empty() || clean_output.contains("Command timed out") {
        return Err("Command timed out or produced no output".to_string());
    }

    Ok(clean_output)
}

#[tauri::command]
pub async fn send_interrupt(session_id: String) -> Result<String, String> {
    match SSHClient::send_interrupt(&session_id).await {
        Ok(_) => Ok("Interrupt signal (Ctrl+C) sent successfully".to_string()),
        Err(e) => Err(format!("Error sending interrupt: {}", e)),
    }
}

#[tauri::command]
pub async fn send_terminal_input(session_id: String, input: String) -> Result<String, String> {
    match SSHClient::send_terminal_input(&session_id, &input).await {
        Ok(_) => Ok(format!("Input '{}' sent successfully", input.escape_debug())),
        Err(e) => Err(format!("Error sending input: {}", e)),
    }
}

#[tauri::command]
pub async fn generate_session_title(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    model_id: String,
    channel_id: String,
) -> Result<String, String> {
    let current_title = {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT title FROM ai_sessions WHERE id = ?1").map_err(|e| e.to_string())?;
        stmt.query_row(params![session_id], |row| row.get::<_, String>(0)).map_err(|e| e.to_string())?
    };

    if current_title != "New Chat" { return Ok(current_title); }

    let messages = {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT role, content FROM ai_messages WHERE session_id = ?1 ORDER BY created_at ASC LIMIT 2").map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![session_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).map_err(|e| e.to_string())?;
        let mut msgs = Vec::new();
        for row in rows { msgs.push(row.map_err(|e| e.to_string())?); }
        msgs
    };

    if messages.len() < 2 { return Err("Not enough messages to generate title".to_string()); }

    let (channel, model, proxy) = {
        let config = state.config.lock().await;
        let model = config.ai_models.iter().find(|m| m.id == model_id).cloned().ok_or("Model not found")?;
        let channel = config.ai_channels.iter().find(|c| c.id == channel_id).cloned().ok_or("Channel not found")?;
        let proxy = channel.proxy_id.as_ref().and_then(|id| config.proxies.iter().find(|p| &p.id == id).cloned());
        (channel, model, proxy)
    };

    let system_prompt = "You are a helpful assistant that generates concise chat titles. Generate a short title (max 6 words) that summarizes the main topic of the conversation. Only output the title text, nothing else.";
    let user_content = format!("Based on this conversation, generate a concise title:\n\nUser: {}\n\nAssistant: {}", messages.get(0).map(|(_, c)| c.as_str()).unwrap_or(""), messages.get(1).map(|(_, c)| c.as_str()).unwrap_or(""));

    let title_messages = vec![
        GenaiMessage::system(system_prompt.to_string()),
        GenaiMessage::user(user_content),
    ];

    let mut stream = state.ai_manager.stream_chat(&channel, &model, title_messages, None, proxy).await?;
    let mut title = String::new();
    while let Some(event_result) = stream.next().await {
        match event_result {
            Ok(ChatStreamEvent::Chunk(chunk)) => { title.push_str(&chunk.content); }
            _ => {}
        }
    }

    let title = title.trim().trim_matches('"').trim_matches('\'').to_string();
    let title = if title.len() > 50 { format!("{}...", &title[..47]) } else { title };
    if title.is_empty() { return Err("Failed to generate title".to_string()); }

    {
        let conn = state.db_manager.get_connection();
        let conn = conn.lock().unwrap();
        conn.execute("UPDATE ai_sessions SET title = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2", params![title, session_id]).map_err(|e| e.to_string())?;
    }

    Ok(title)
}

#[tauri::command]
pub async fn delete_ai_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    let conn = state.db_manager.get_connection();
    let conn = conn.lock().unwrap();
    conn.execute("DELETE FROM ai_messages WHERE session_id = ?1", params![session_id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM ai_sessions WHERE id = ?1", params![session_id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_all_ai_sessions(
    state: State<'_, Arc<AppState>>,
    server_id: String,
) -> Result<(), String> {
    let conn = state.db_manager.get_connection();
    let conn = conn.lock().unwrap();
    conn.execute("DELETE FROM ai_messages WHERE session_id IN (SELECT id FROM ai_sessions WHERE server_id = ?1)", params![server_id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM ai_sessions WHERE server_id = ?1", params![server_id]).map_err(|e| e.to_string())?;
    Ok(())
}
