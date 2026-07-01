pub mod copilot;
mod history;
mod stream_parsing;
mod tool_registry;
mod tool_runtime;
mod turn;
mod types;

pub use tool_registry::create_tools;
pub use types::{
    AccessTokenResponse, ChatMessage, DeviceCodeResponse, FunctionCall, FunctionDefinition,
    ToolCall, ToolDefinition,
};

use history::{enrich_user_message_with_tagged_files, load_history};
use tool_runtime::{
    build_recording_input_payload, build_run_in_terminal_timeout_failure_message,
    execute_command_in_exec_channel, try_reconnect_terminal_after_timeout,
    try_recover_terminal_after_timeout, START_MARKER_EXPECT_MS, TIMEOUT_RECOVERY_GRACE_MS,
};
use turn::{execute_tools_and_save, run_ai_turn};

fn is_read_only_tool(name: &str) -> bool {
    matches!(
        name,
        "get_terminal_output" | "get_selected_terminal_output" | "read_file"
    )
}

const DUMMY_TOOL_RESPONSE: &str = "Interrupted or skipped by user";

fn latest_assistant_tool_calls_with_pending_ids(
    history: &[ChatMessage],
    requested_ids: &[String],
) -> Option<Vec<ToolCall>> {
    let requested: HashSet<&str> = requested_ids.iter().map(String::as_str).collect();
    let mut responded: HashSet<&str> = HashSet::new();

    for msg in history.iter().rev() {
        if msg.role == "tool" {
            if msg.content.as_deref() == Some(DUMMY_TOOL_RESPONSE) {
                continue;
            }
            if let Some(id) = msg.tool_call_id.as_deref() {
                responded.insert(id);
            }
            continue;
        }

        if msg.role != "assistant" {
            continue;
        }

        let Some(calls) = msg.tool_calls.as_ref() else {
            continue;
        };
        let pending: Vec<ToolCall> = calls
            .iter()
            .filter(|call| {
                requested.contains(call.id.as_str()) && !responded.contains(call.id.as_str())
            })
            .cloned()
            .collect();

        if !pending.is_empty() {
            return Some(pending);
        }
    }

    None
}

use crate::commands::AppState;
use crate::ssh_manager::ssh::SSHClient;
use futures::StreamExt;
use genai::chat::{ChatMessage as GenaiMessage, ChatStreamEvent};
use rusqlite::params;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, Manager, State, Window};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(super) const AI_STREAM_IDLE_TIMEOUT_SECS: u64 = 45;

#[cfg(test)]
mod history_window_tests {
    use super::history::truncate_dialog_messages_for_history;
    use super::ChatMessage;

    fn msg(role: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: Some(format!("{}-content", role)),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            created_at: None,
            model_id: None,
        }
    }

    #[test]
    fn truncate_moves_start_to_next_user_within_window() {
        let messages = vec![
            msg("user"),
            msg("assistant"),
            msg("user"),
            msg("assistant"),
            msg("tool"),
            msg("user"),
            msg("assistant"),
        ];

        let truncated = truncate_dialog_messages_for_history(messages, 4);
        let roles: Vec<&str> = truncated.iter().map(|m| m.role.as_str()).collect();

        assert_eq!(roles, vec!["user", "assistant"]);
    }

    #[test]
    fn truncate_falls_back_to_previous_user_when_window_tail_has_no_user() {
        let messages = vec![
            msg("user"),
            msg("assistant"),
            msg("user"),
            msg("assistant"),
            msg("tool"),
        ];

        let truncated = truncate_dialog_messages_for_history(messages, 2);
        let roles: Vec<&str> = truncated.iter().map(|m| m.role.as_str()).collect();

        assert_eq!(roles, vec!["user", "assistant", "tool"]);
    }

    #[test]
    fn truncate_returns_empty_when_no_user_exists() {
        let messages = vec![msg("assistant"), msg("tool"), msg("assistant")];

        let truncated = truncate_dialog_messages_for_history(messages, 3);

        assert!(truncated.is_empty());
    }
}

#[cfg(test)]
mod pending_tool_call_tests {
    use super::{
        latest_assistant_tool_calls_with_pending_ids, ChatMessage, FunctionCall, ToolCall,
    };

    fn call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            tool_type: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: "{}".to_string(),
            },
        }
    }

    fn assistant(calls: Vec<ToolCall>) -> ChatMessage {
        ChatMessage {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: Some("trace".to_string()),
            tool_calls: Some(calls),
            tool_call_id: None,
            created_at: None,
            model_id: None,
        }
    }

    fn tool(id: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: "tool".to_string(),
            content: Some(content.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: Some(id.to_string()),
            created_at: None,
            model_id: None,
        }
    }

    #[test]
    fn finds_pending_confirm_call_after_read_only_tool_response() {
        let history = vec![
            ChatMessage {
                role: "user".to_string(),
                content: Some("run this".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: None,
                created_at: None,
                model_id: None,
            },
            assistant(vec![
                call("read_1", "read_file"),
                call("exec_1", "run_in_terminal"),
            ]),
            tool("read_1", "file output"),
        ];

        let pending =
            latest_assistant_tool_calls_with_pending_ids(&history, &["exec_1".to_string()])
                .expect("pending tool call");

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "exec_1");
    }

    #[test]
    fn does_not_repeat_already_executed_tool() {
        let history = vec![
            assistant(vec![
                call("read_1", "read_file"),
                call("exec_1", "run_in_terminal"),
            ]),
            tool("read_1", "file output"),
            tool("exec_1", "command output"),
        ];

        let pending =
            latest_assistant_tool_calls_with_pending_ids(&history, &["read_1".to_string()]);

        assert!(pending.is_none());
    }
}

#[cfg(test)]
mod streamed_tool_call_tests {
    use super::history::to_genai_messages;
    use super::stream_parsing::{
        accumulate_streamed_tool_call_chunk, normalize_streamed_tool_calls,
    };
    use super::turn::apply_reasoning_fallback;
    use super::types::{ChatMessage, FunctionCall, ToolCall};
    use std::collections::HashMap;

    fn new_call(id: &str, name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            tool_type: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: arguments.to_string(),
            },
        }
    }

    #[test]
    fn accumulate_streamed_tool_call_chunk_merges_split_alias_fragments() {
        let mut calls: Vec<ToolCall> = Vec::new();
        let mut aliases: HashMap<String, String> = HashMap::new();

        accumulate_streamed_tool_call_chunk(
            &mut calls,
            &mut aliases,
            "tooluse_primary",
            "run_in_terminal",
            "",
        );
        accumulate_streamed_tool_call_chunk(&mut calls, &mut aliases, "call_1", "", "{\"com");
        accumulate_streamed_tool_call_chunk(
            &mut calls,
            &mut aliases,
            "call_1",
            "",
            "mand\":\"pwd\"}",
        );

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "tooluse_primary");
        assert_eq!(calls[0].function.name, "run_in_terminal");
        assert_eq!(calls[0].function.arguments, "{\"command\":\"pwd\"}");
        assert_eq!(
            aliases.get("call_1").map(String::as_str),
            Some("tooluse_primary")
        );
    }

    #[test]
    fn normalize_streamed_tool_calls_drops_orphan_nameless_call() {
        let calls = vec![
            new_call("call_1", "", "{\"command\":\"whoami\"}"),
            new_call(
                "tooluse_primary",
                "run_in_terminal",
                "{\"command\":\"pwd\"}",
            ),
        ];

        let normalized = normalize_streamed_tool_calls(calls);

        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].id, "tooluse_primary");
        assert_eq!(normalized[0].function.name, "run_in_terminal");
        assert_eq!(normalized[0].function.arguments, "{\"command\":\"pwd\"}");
    }

    #[test]
    fn normalize_streamed_tool_calls_normalizes_empty_args_to_empty_object() {
        let calls = vec![new_call("tooluse_1", "get_terminal_output", "")];

        let normalized = normalize_streamed_tool_calls(calls);

        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].function.arguments, "{}");
    }

    #[test]
    fn to_genai_messages_skips_empty_assistant_after_invalid_tool_cleanup() {
        let history = vec![ChatMessage {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: None,
            tool_calls: Some(vec![new_call("call_1", "", "{\"command\":\"pwd\"}")]),
            tool_call_id: None,
            created_at: None,
            model_id: None,
        }];

        let messages = to_genai_messages(history);

        assert!(messages.is_empty());
    }

    #[test]
    fn to_genai_messages_keeps_only_valid_tool_calls() {
        let history = vec![ChatMessage {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: None,
            tool_calls: Some(vec![
                new_call("call_1", "", "{\"command\":\"pwd\"}"),
                new_call(
                    "tooluse_ok",
                    "run_in_terminal",
                    "{\"command\":\"pwd\",\"timeoutSeconds\":30}",
                ),
            ]),
            tool_call_id: None,
            created_at: None,
            model_id: None,
        }];

        let messages = to_genai_messages(history);

        assert_eq!(messages.len(), 1);
        let tool_calls = messages[0].content.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].call_id, "tooluse_ok");
        assert_eq!(tool_calls[0].fn_name, "run_in_terminal");
    }

    #[test]
    fn to_genai_messages_keeps_empty_tool_args_as_empty_object() {
        let history = vec![ChatMessage {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: None,
            tool_calls: Some(vec![new_call("tooluse_1", "get_terminal_output", "")]),
            tool_call_id: None,
            created_at: None,
            model_id: None,
        }];

        let messages = to_genai_messages(history);

        assert_eq!(messages.len(), 1);
        let tool_calls = messages[0].content.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].call_id, "tooluse_1");
        assert_eq!(tool_calls[0].fn_name, "get_terminal_output");
        assert_eq!(tool_calls[0].fn_arguments, serde_json::json!({}));
    }

    #[test]
    fn to_genai_messages_preserves_assistant_reasoning_content() {
        let history = vec![ChatMessage {
            role: "assistant".to_string(),
            content: Some("final answer".to_string()),
            reasoning_content: Some("thinking trace".to_string()),
            tool_calls: Some(vec![new_call("tooluse_1", "get_terminal_output", "{}")]),
            tool_call_id: None,
            created_at: None,
            model_id: None,
        }];

        let messages = to_genai_messages(history);

        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0].content.first_reasoning_content(),
            Some("thinking trace")
        );
        assert_eq!(messages[0].content.first_text(), Some("final answer"));
        let tool_calls = messages[0].content.tool_calls();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].call_id, "tooluse_1");
    }

    #[test]
    fn reasoning_fallback_uses_captured_reasoning_only_when_empty() {
        let mut empty_reasoning = String::new();
        apply_reasoning_fallback(&mut empty_reasoning, Some("captured trace".to_string()));
        assert_eq!(empty_reasoning, "captured trace");

        let mut streamed_reasoning = "streamed trace".to_string();
        apply_reasoning_fallback(&mut streamed_reasoning, Some("captured trace".to_string()));
        assert_eq!(streamed_reasoning, "streamed trace");
    }

    #[test]
    fn to_genai_messages_drops_unmatched_tool_message_after_sanitization() {
        let history = vec![
            ChatMessage {
                role: "assistant".to_string(),
                content: None,
                reasoning_content: None,
                tool_calls: Some(vec![new_call("tooluse_bad", "get_terminal_output", "[]")]),
                tool_call_id: None,
                created_at: None,
                model_id: None,
            },
            ChatMessage {
                role: "tool".to_string(),
                content: Some("output".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: Some("tooluse_bad".to_string()),
                created_at: None,
                model_id: None,
            },
        ];

        let messages = to_genai_messages(history);

        assert!(messages.is_empty());
    }
}

#[cfg(test)]
mod tool_timeout_requirement_tests {
    use super::stream_parsing::extract_required_timeout_seconds;
    use super::tool_registry::create_tools;

    fn required_fields(tool_name: &str) -> Vec<String> {
        create_tools(true)
            .into_iter()
            .find(|tool| tool.function.name == tool_name)
            .and_then(|tool| {
                tool.function
                    .parameters
                    .get("required")
                    .and_then(|value| value.as_array().cloned())
            })
            .map(|values| {
                values
                    .into_iter()
                    .filter_map(|value| value.as_str().map(|text| text.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn execution_tools_require_timeout_seconds_in_schema() {
        let terminal_required = required_fields("run_in_terminal");
        assert!(terminal_required.iter().any(|field| field == "command"));
        assert!(terminal_required
            .iter()
            .any(|field| field == "timeoutSeconds"));

        let background_required = required_fields("run_in_background");
        assert!(background_required.iter().any(|field| field == "command"));
        assert!(background_required
            .iter()
            .any(|field| field == "timeoutSeconds"));
    }

    #[test]
    fn required_timeout_parser_rejects_missing_or_invalid_values() {
        let missing_args = serde_json::json!({ "command": "ls" });
        let zero_args = serde_json::json!({ "command": "ls", "timeoutSeconds": 0 });
        let valid_args = serde_json::json!({ "command": "ls", "timeoutSeconds": 45 });

        let missing_error = extract_required_timeout_seconds(&missing_args, "run_in_terminal")
            .err()
            .unwrap_or_default();
        let invalid_error = extract_required_timeout_seconds(&zero_args, "run_in_terminal")
            .err()
            .unwrap_or_default();

        assert!(missing_error.contains("Missing required 'timeoutSeconds'"));
        assert!(invalid_error.contains("Invalid 'timeoutSeconds'"));
        assert_eq!(
            extract_required_timeout_seconds(&valid_args, "run_in_terminal"),
            Ok(45)
        );
    }
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

#[tauri::command]
pub async fn create_ai_session(
    state: State<'_, Arc<AppState>>,
    server_id: String,
    model_id: Option<String>,
    ssh_session_id: Option<String>,
) -> Result<String, String> {
    let id = Uuid::new_v4().to_string();
    let id_clone = id.clone();
    state
        .db_manager
        .run_blocking(move |conn| {
            conn.execute(
                "INSERT INTO ai_sessions (id, server_id, title, model_id, ssh_session_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id_clone, server_id, "New Chat", model_id, ssh_session_id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await?;

    Ok(id)
}

#[tauri::command]
pub async fn get_ai_sessions(
    state: State<'_, Arc<AppState>>,
    server_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    state
        .db_manager
        .run_blocking(move |conn| {
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
        })
        .await
}

#[tauri::command]
pub async fn get_ai_messages(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<Vec<ChatMessage>, String> {
    state
        .db_manager
        .run_blocking(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT role, content, reasoning_content, tool_calls, tool_call_id, created_at, model_id
                     FROM ai_messages
                     WHERE session_id = ?1
                     ORDER BY created_at ASC, rowid ASC",
                )
                .map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map(params![session_id], |row| {
                    let role: String = row.get(0)?;
                    let content_raw: String = row.get(1)?;
                    let reasoning_raw: Option<String> = row.get(2)?;
                    let tool_calls_json: Option<String> = row.get(3).ok();
                    let tool_call_id: Option<String> = row.get(4).ok();
                    let created_at: String = row.get(5)?;
                    let model_id: Option<String> = row.get(6).ok();

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
                        created_at: Some(created_at),
                        model_id,
                    })
                })
                .map_err(|e| e.to_string())?;

            let mut messages: Vec<ChatMessage> = Vec::new();
            for row in rows {
                let msg: ChatMessage = row.map_err(|e| e.to_string())?;
                if msg.role != "system" {
                    messages.push(msg);
                }
            }

            Ok(messages)
        })
        .await
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
    thinking_level: Option<String>,
) -> Result<(), String> {
    let _ = window.emit(&format!("ai-started-{}", session_id), "started");

    let bound_ssh_session_id: Option<String> = {
        let session_id_owned = session_id.clone();
        let ssh_session_id_clone = ssh_session_id.clone();
        state
            .db_manager
            .run_blocking(move |conn| {
                let existing_ssh_id: Option<String> = conn
                    .query_row(
                        "SELECT ssh_session_id FROM ai_sessions WHERE id = ?1",
                        params![session_id_owned],
                        |row| row.get(0),
                    )
                    .ok();

                let result = if existing_ssh_id.is_none()
                    || existing_ssh_id
                        .as_ref()
                        .map(|s| s.is_empty())
                        .unwrap_or(true)
                {
                    if let Some(ref new_id) = ssh_session_id_clone {
                        if !new_id.is_empty() {
                            conn.execute(
                                "UPDATE ai_sessions SET ssh_session_id = ?1 WHERE id = ?2",
                                params![new_id, session_id_owned],
                            )
                            .ok();
                            ssh_session_id_clone.clone()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    existing_ssh_id
                };
                Ok(result)
            })
            .await?
    };

    let content =
        enrich_user_message_with_tagged_files(&content, bound_ssh_session_id.as_deref()).await;

    let user_msg_id = Uuid::new_v4().to_string();
    {
        let user_msg_id_owned = user_msg_id.clone();
        let session_id_owned = session_id.clone();
        let content_owned = content.clone();
        state
            .db_manager
            .run_blocking(move |conn| {
                conn.execute(
                    "INSERT INTO ai_messages (id, session_id, role, content) VALUES (?1, ?2, ?3, ?4)",
                    params![user_msg_id_owned, session_id_owned, "user", content_owned],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            })
            .await?;
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
        thinking_level,
    )
    .await;

    state.ai_cancellation_tokens.remove(&session_id);

    match result {
        Ok(tool_calls) => {
            if let Some(calls) = tool_calls {
                if !calls.is_empty() {
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
            window
                .emit(&format!("ai-error-{}", session_id), e.clone())
                .ok();
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn regenerate_ai_response(
    window: Window,
    state: State<'_, Arc<AppState>>,
    session_id: String,
    model_id: String,
    channel_id: String,
    mode: Option<String>,
    ssh_session_id: Option<String>,
    thinking_level: Option<String>,
) -> Result<(), String> {
    let _ = window.emit(&format!("ai-started-{}", session_id), "started");

    let bound_ssh_session_id: Option<String> = {
        let session_id_owned = session_id.clone();
        let ssh_session_id_clone = ssh_session_id.clone();
        state
            .db_manager
            .run_blocking(move |conn| {
                let existing_ssh_id: Option<String> = conn
                    .query_row(
                        "SELECT ssh_session_id FROM ai_sessions WHERE id = ?1",
                        params![session_id_owned],
                        |row| row.get(0),
                    )
                    .ok();

                let result = if existing_ssh_id
                    .as_ref()
                    .map(|id| id.is_empty())
                    .unwrap_or(true)
                {
                    ssh_session_id_clone.clone()
                } else {
                    existing_ssh_id
                };
                Ok(result)
            })
            .await?
    };

    {
        let session_id_owned = session_id.clone();
        state
            .db_manager
            .run_blocking(move |conn| {
                let (latest_msg_id, latest_role): (String, String) = conn
                    .query_row(
                        "SELECT id, role FROM ai_messages WHERE session_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 1",
                        params![session_id_owned],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .map_err(|_| "No history found".to_string())?;

                if latest_role != "assistant" {
                    return Err("Latest message is not an assistant response".to_string());
                }

                conn.execute(
                    "DELETE FROM ai_messages WHERE id = ?1",
                    params![latest_msg_id],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            })
            .await?;
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
        thinking_level,
    )
    .await;

    state.ai_cancellation_tokens.remove(&session_id);

    match result {
        Ok(tool_calls) => {
            if let Some(calls) = tool_calls {
                if !calls.is_empty() {
                    return Ok(());
                }
            }
            window
                .emit(&format!("ai-done-{}", session_id), "DONE")
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        Err(e) => {
            tracing::error!("[AI] regenerate_ai_response error: {}", e);
            window
                .emit(&format!("ai-error-{}", session_id), e.clone())
                .ok();
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
    thinking_level: Option<String>,
) -> Result<(), String> {
    let _ = window.emit(&format!("ai-started-{}", session_id), "started");

    let bound_ssh_session_id: Option<String> = {
        let session_id_owned = session_id.clone();
        let ssh_session_id_clone = ssh_session_id.clone();
        state
            .db_manager
            .run_blocking(move |conn| {
                let existing_ssh_id: Option<String> = conn
                    .query_row(
                        "SELECT ssh_session_id FROM ai_sessions WHERE id = ?1",
                        params![session_id_owned],
                        |row| row.get(0),
                    )
                    .ok();

                let result = if existing_ssh_id.is_none()
                    || existing_ssh_id
                        .as_ref()
                        .map(|s| s.is_empty())
                        .unwrap_or(true)
                {
                    ssh_session_id_clone.clone()
                } else {
                    existing_ssh_id
                };
                Ok(result)
            })
            .await?
    };

    let is_agent_mode = mode.as_deref() == Some("agent");

    let history: Vec<ChatMessage> = load_history(
        &state,
        &session_id,
        is_agent_mode,
        bound_ssh_session_id.as_deref(),
    )
    .await?;

    let Some(tools_filtered) =
        latest_assistant_tool_calls_with_pending_ids(&history, &tool_call_ids)
    else {
        window
            .emit(&format!("ai-done-{}", session_id), "DONE")
            .map_err(|e| e.to_string())?;
        return Ok(());
    };

    if tools_filtered.is_empty() {
        window
            .emit(&format!("ai-done-{}", session_id), "DONE")
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    if !is_agent_mode {
        for call in &tools_filtered {
            if call.function.name == "run_in_terminal" || call.function.name == "run_in_background"
            {
                return Err(
                    "Execution denied: run_in_terminal/run_in_background are only allowed in Agent mode.".to_string(),
                );
            }
        }
    }

    let token = CancellationToken::new();
    state
        .ai_cancellation_tokens
        .insert(session_id.clone(), token.clone());

    let exec_res = execute_tools_and_save(
        window.app_handle().clone(),
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
        thinking_level,
    )
    .await;

    state.ai_cancellation_tokens.remove(&session_id);

    match result {
        Ok(next_tool_calls) => {
            if let Some(calls) = next_tool_calls {
                if !calls.is_empty() {
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
            window
                .emit(&format!("ai-error-{}", session_id), e.clone())
                .ok();
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
    window: Window,
    session_id: String,
    command: String,
    timeout_seconds: Option<u64>,
    wait_finish: Option<bool>,
) -> Result<String, String> {
    let should_wait_finish = wait_finish.unwrap_or(true);
    let command_block_event = format!("terminal-command-block:{}", session_id);
    if !should_wait_finish {
        let cmd_nl = format!("{}\n", command);
        let _ = window.emit(&command_block_event, "start");
        if let Err(e) = SSHClient::send_input(&session_id, cmd_nl.as_bytes()).await {
            let _ = window.emit(&command_block_event, "end");
            return Err(format!("Failed to send command: {}", e));
        }
        return Ok(
            "Command sent to terminal without waiting for completion (wait_finish=false)."
                .to_string(),
        );
    }

    let timeout = timeout_seconds.unwrap_or(30);
    let timeout_ms = timeout * 1000;

    SSHClient::start_command_recording(&session_id).await?;

    let input_payload =
        build_recording_input_payload(&session_id, &command, "run_in_terminal").await;

    let _ = window.emit(&command_block_event, "start");

    if let Err(e) = SSHClient::send_input(&session_id, input_payload.as_bytes()).await {
        let _ = window.emit(&command_block_event, "end");
        let _ = SSHClient::stop_command_recording(&session_id).await;
        return Err(format!("Failed to send command: {}", e));
    }

    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));
    let mut elapsed = 0u64;
    let mut completion_detected = false;
    let mut timed_out = false;
    let mut start_marker_seen = false;

    loop {
        interval.tick().await;
        elapsed += 100;

        if !start_marker_seen {
            start_marker_seen = SSHClient::is_recording_start_marker_seen(&session_id)
                .await
                .unwrap_or(false);
        }

        match SSHClient::check_command_completed(&session_id).await {
            Ok(true) => {
                completion_detected = true;
                break;
            }
            Ok(false) => {}
            Err(_) => {}
        }

        if elapsed >= START_MARKER_EXPECT_MS && !start_marker_seen {
            tracing::warn!(
                "[run_in_terminal] Start marker not observed within {}ms for '{}'; treating foreground channel as stalled",
                START_MARKER_EXPECT_MS,
                command
            );
            timed_out = true;
            break;
        }

        if elapsed >= timeout_ms {
            timed_out = true;
            break;
        }
    }

    if timed_out && !completion_detected {
        let recovered =
            try_recover_terminal_after_timeout(&session_id, &command, "run_in_terminal").await;
        if !recovered {
            tracing::warn!(
                "[run_in_terminal] Terminal not recovered within {}ms grace after timeout for '{}'",
                TIMEOUT_RECOVERY_GRACE_MS,
                command
            );
            let _ = try_reconnect_terminal_after_timeout(&session_id, &command, "run_in_terminal")
                .await;
        }
    }

    let _ = window.emit(&command_block_event, "end");

    let output = SSHClient::stop_command_recording(&session_id).await?;
    let clean_output =
        String::from_utf8_lossy(&strip_ansi_escapes::strip(&output.as_bytes())).to_string();

    if timed_out && !completion_detected {
        return Err(build_run_in_terminal_timeout_failure_message(
            timeout,
            &clean_output,
        ));
    }

    if clean_output.trim().is_empty() {
        return Err("Command produced no output".to_string());
    }

    Ok(clean_output)
}

#[tauri::command]
pub async fn run_in_background(
    session_id: String,
    command: String,
    timeout_seconds: Option<u64>,
) -> Result<String, String> {
    let timeout = timeout_seconds.unwrap_or(30);
    let exec_result = execute_command_in_exec_channel(&session_id, &command, timeout, None).await?;
    let clean_output =
        String::from_utf8_lossy(&strip_ansi_escapes::strip(exec_result.output.as_bytes()))
            .to_string();

    if exec_result.timed_out {
        if clean_output.trim().is_empty() {
            return Err(format!("run_in_background timed out after {}s.", timeout));
        }
        return Err(format!(
            "run_in_background timed out after {}s.\n\n[Partial output]\n{}",
            timeout,
            clean_output.trim()
        ));
    }

    if let Some(status) = exec_result.exit_status {
        if status != 0 {
            if clean_output.trim().is_empty() {
                return Err(format!(
                    "run_in_background command exited with status {}.",
                    status
                ));
            }
            return Err(format!(
                "run_in_background command exited with status {}.\n\n{}",
                status, clean_output
            ));
        }
    }

    if clean_output.trim().is_empty() {
        return Ok("Command completed with no output".to_string());
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
        Ok(_) => Ok(format!(
            "Input '{}' sent successfully",
            input.escape_debug()
        )),
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
    let current_title: String = {
        let session_id_owned = session_id.clone();
        state
            .db_manager
            .run_blocking(move |conn| {
                let mut stmt = conn
                    .prepare("SELECT title FROM ai_sessions WHERE id = ?1")
                    .map_err(|e| e.to_string())?;
                stmt.query_row(params![session_id_owned], |row| row.get::<_, String>(0))
                    .map_err(|e| e.to_string())
            })
            .await?
    };

    if current_title != "New Chat" {
        return Ok(current_title);
    }

    let messages: Vec<(String, String)> = {
        let session_id_owned = session_id.clone();
        state
            .db_manager
            .run_blocking(move |conn| {
                let mut stmt = conn
                    .prepare("SELECT role, content FROM ai_messages WHERE session_id = ?1 ORDER BY created_at ASC LIMIT 2")
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map(params![session_id_owned], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(|e| e.to_string())?;
                let mut msgs = Vec::new();
                for row in rows {
                    msgs.push(row.map_err(|e| e.to_string())?);
                }
                Ok(msgs)
            })
            .await?
    };

    if messages.len() < 2 {
        return Err("Not enough messages to generate title".to_string());
    }

    let (channel, model, proxy) = {
        let config = state.config.lock().await;
        let model = config
            .ai_models
            .iter()
            .find(|m| m.id == model_id)
            .cloned()
            .ok_or("Model not found")?;
        let channel = config
            .ai_channels
            .iter()
            .find(|c| c.id == channel_id)
            .cloned()
            .ok_or("Channel not found")?;
        let proxy = channel
            .proxy_id
            .as_ref()
            .and_then(|id| config.proxies.iter().find(|p| &p.id == id).cloned());
        (channel, model, proxy)
    };

    let system_prompt = "You are a helpful assistant that generates concise chat titles. Generate a short title (max 6 words) that summarizes the main topic of the conversation. Only output the title text, nothing else.";
    let user_content = format!(
        "Based on this conversation, generate a concise title:\n\nUser: {}\n\nAssistant: {}",
        messages.get(0).map(|(_, c)| c.as_str()).unwrap_or(""),
        messages.get(1).map(|(_, c)| c.as_str()).unwrap_or("")
    );

    let title_messages = vec![
        GenaiMessage::system(system_prompt.to_string()),
        GenaiMessage::user(user_content),
    ];

    let mut stream = state
        .ai_manager
        .stream_chat(&channel, &model, title_messages, None, proxy, None)
        .await?;
    let mut title = String::new();
    loop {
        let next_event = tokio::time::timeout(
            Duration::from_secs(AI_STREAM_IDLE_TIMEOUT_SECS),
            stream.next(),
        )
        .await
        .map_err(|_| {
            format!(
                "AI title stream stalled for {} seconds.",
                AI_STREAM_IDLE_TIMEOUT_SECS
            )
        })?;
        let Some(event_result) = next_event else {
            break;
        };
        match event_result {
            Ok(ChatStreamEvent::Chunk(chunk)) => {
                title.push_str(&chunk.content);
            }
            _ => {}
        }
    }

    let title = title
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    let title = if title.chars().count() > 50 {
        format!("{}...", title.chars().take(47).collect::<String>())
    } else {
        title
    };
    if title.is_empty() {
        return Err("Failed to generate title".to_string());
    }

    {
        let title_owned = title.clone();
        let session_id_owned = session_id.clone();
        state
            .db_manager
            .run_blocking(move |conn| {
                conn.execute(
                    "UPDATE ai_sessions SET title = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
                    params![title_owned, session_id_owned],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            })
            .await?;
    }

    Ok(title)
}

#[tauri::command]
pub async fn delete_ai_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    state
        .db_manager
        .run_blocking(move |conn| {
            conn.execute(
                "DELETE FROM ai_messages WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM ai_sessions WHERE id = ?1", params![session_id])
                .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
}

#[tauri::command]
pub async fn delete_all_ai_sessions(
    state: State<'_, Arc<AppState>>,
    server_id: String,
) -> Result<(), String> {
    state
        .db_manager
        .run_blocking(move |conn| {
            conn.execute(
                "DELETE FROM ai_messages WHERE session_id IN (SELECT id FROM ai_sessions WHERE server_id = ?1)",
                params![server_id],
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "DELETE FROM ai_sessions WHERE server_id = ?1",
                params![server_id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
}
