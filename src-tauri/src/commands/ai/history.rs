use std::collections::HashSet;
use std::sync::Arc;

use genai::chat::{ChatMessage as GenaiMessage, ContentPart, MessageContent};
use rusqlite::params;
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};

use crate::ai::prompts::SYSTEM_PROMPT;
use crate::ai::validator::{
    validate_and_fix_messages, MessagePayload, ToolCallFunction, ToolCallPayload,
};
use crate::commands::AppState;
use crate::ssh_manager::ssh::SSHClient;

use super::stream_parsing::parse_tool_call_arguments;
use super::types::{ChatMessage, FunctionCall, ToolCall};

pub(super) const READ_FILE_MAX_BYTES: usize = 128 * 1024;
const MAX_TAGGED_FILES_PER_MESSAGE: usize = 8;

fn is_path_trailing_punctuation(character: char) -> bool {
    matches!(
        character,
        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '>' | '"' | '\''
    )
}

fn extract_tagged_file_paths(content: &str) -> Vec<String> {
    let mut paths = Vec::new();

    for token in content.split_whitespace() {
        if let Some(raw_path) = token.strip_prefix("#/") {
            let cleaned_path = raw_path.trim_end_matches(is_path_trailing_punctuation);
            if cleaned_path.is_empty() {
                continue;
            }

            let normalized_path = format!("/{}", cleaned_path);
            if !paths.contains(&normalized_path) {
                paths.push(normalized_path);
            }
        }
    }

    paths
}

pub(super) async fn read_remote_file_via_sftp(
    ssh_session_id: &str,
    remote_path: &str,
    max_bytes: usize,
) -> Result<(String, bool), String> {
    let metadata = crate::sftp_manager::SftpManager::metadata(ssh_session_id, remote_path).await?;
    if metadata.attrs.is_dir() {
        return Err(format!("'{}' is a directory, not a file.", remote_path));
    }

    let sftp = crate::sftp_manager::SftpManager::get_session(ssh_session_id).await?;
    let handle = sftp
        .open(remote_path, OpenFlags::READ, FileAttributes::default())
        .await
        .map_err(|e| e.to_string())?
        .handle;

    let mut offset = 0u64;
    let mut content = Vec::new();
    let mut truncated = false;

    loop {
        let remaining = max_bytes.saturating_sub(content.len());
        if remaining == 0 {
            truncated = true;
            break;
        }

        let chunk_size = remaining.min(255 * 1024) as u32;
        match sftp.read(&handle, offset, chunk_size).await {
            Ok(data) => {
                if data.data.is_empty() {
                    break;
                }
                offset += data.data.len() as u64;
                content.extend_from_slice(&data.data);
            }
            Err(russh_sftp::client::error::Error::Status(status))
                if status.status_code == StatusCode::Eof =>
            {
                break;
            }
            Err(e) => {
                let _ = sftp.close(handle).await;
                return Err(e.to_string());
            }
        }
    }

    let _ = sftp.close(handle).await;
    Ok((String::from_utf8_lossy(&content).to_string(), truncated))
}

pub(super) async fn enrich_user_message_with_tagged_files(
    content: &str,
    ssh_session_id: Option<&str>,
) -> String {
    let tagged_paths = extract_tagged_file_paths(content);
    if tagged_paths.is_empty() {
        return content.to_string();
    }

    let tagged_path_count = tagged_paths.len();

    let mut enriched_content = content.to_string();
    enriched_content.push_str("\n\n[Remote file context loaded via read_file]\n");

    let paths_to_load: Vec<String> = tagged_paths
        .into_iter()
        .take(MAX_TAGGED_FILES_PER_MESSAGE)
        .collect();

    for path in &paths_to_load {
        if let Some(ssh_id) = ssh_session_id {
            match read_remote_file_via_sftp(ssh_id, path, READ_FILE_MAX_BYTES).await {
                Ok((file_content, truncated)) => {
                    enriched_content.push_str(&format!("\n[File: {}]\n", path));
                    enriched_content.push_str(&file_content);
                    if truncated {
                        enriched_content.push_str(&format!(
                            "\n[Truncated to first {} bytes]",
                            READ_FILE_MAX_BYTES
                        ));
                    }
                    enriched_content.push_str("\n[/File]\n");
                }
                Err(error) => {
                    enriched_content.push_str(&format!(
                        "\n[File: {}]\n[Read error: {}]\n[/File]\n",
                        path, error
                    ));
                }
            }
        } else {
            enriched_content.push_str(&format!(
                "\n[File: {}]\n[Read error: No active terminal session linked to this chat.]\n[/File]\n",
                path
            ));
        }
    }

    if paths_to_load.len() < tagged_path_count {
        enriched_content.push_str(&format!(
            "\n[Note: Only the first {} tagged files were loaded.]\n",
            MAX_TAGGED_FILES_PER_MESSAGE
        ));
    }

    enriched_content
}

fn attach_reasoning_content(
    message: GenaiMessage,
    reasoning_content: Option<String>,
) -> GenaiMessage {
    if let Some(reasoning_content) = reasoning_content.filter(|content| !content.is_empty()) {
        message.with_reasoning_content(Some(reasoning_content))
    } else {
        message
    }
}

pub(super) fn to_genai_messages(history: Vec<ChatMessage>) -> Vec<GenaiMessage> {
    let mut messages = Vec::with_capacity(history.len());
    let mut pending_tool_call_ids: HashSet<String> = HashSet::new();

    for msg in history {
        match msg.role.as_str() {
            "system" => {
                pending_tool_call_ids.clear();
                messages.push(GenaiMessage::system(msg.content.unwrap_or_default()));
            }
            "user" => {
                pending_tool_call_ids.clear();
                messages.push(GenaiMessage::user(msg.content.unwrap_or_default()));
            }
            "assistant" => {
                let reasoning_content = msg.reasoning_content;
                let content = msg.content.unwrap_or_default();
                if let Some(tool_calls) = msg.tool_calls {
                    let genai_tool_calls: Vec<genai::chat::ToolCall> = tool_calls
                        .into_iter()
                        .filter_map(|tc| {
                            if tc.function.name.trim().is_empty() {
                                tracing::warn!(
                                    "[AI] Dropping persisted tool_call with empty function name: id={}",
                                    tc.id
                                );
                                return None;
                            }

                            let parsed_args = match parse_tool_call_arguments(&tc.function.arguments) {
                                Ok(value) => value,
                                Err(e) => {
                                    tracing::warn!(
                                        "[AI] Dropping persisted tool_call with invalid args JSON: id={}, name={}, err={}",
                                        tc.id,
                                        tc.function.name,
                                        e
                                    );
                                    return None;
                                }
                            };

                            Some(genai::chat::ToolCall {
                                call_id: tc.id,
                                fn_name: tc.function.name,
                                fn_arguments: parsed_args,
                                thought_signatures: None,
                            })
                        })
                        .collect();

                    if !genai_tool_calls.is_empty() {
                        pending_tool_call_ids = genai_tool_calls
                            .iter()
                            .map(|call| call.call_id.clone())
                            .collect();

                        let mut parts = Vec::new();
                        if !content.is_empty() {
                            parts.push(ContentPart::Text(content));
                        }
                        parts.extend(genai_tool_calls.into_iter().map(ContentPart::ToolCall));
                        messages.push(attach_reasoning_content(
                            GenaiMessage::assistant(MessageContent::from_parts(parts)),
                            reasoning_content,
                        ));
                        continue;
                    }
                }

                pending_tool_call_ids.clear();

                let has_reasoning_content = reasoning_content
                    .as_ref()
                    .map(|content| !content.is_empty())
                    .unwrap_or(false);
                if content.is_empty() && !has_reasoning_content {
                    tracing::warn!(
                        "[AI] Dropping empty assistant message after tool-call sanitization to avoid invalid payload."
                    );
                    continue;
                }

                messages.push(attach_reasoning_content(
                    GenaiMessage::assistant(content),
                    reasoning_content,
                ));
            }
            "tool" => {
                let call_id = msg.tool_call_id.unwrap_or_default();
                if call_id.is_empty() {
                    tracing::warn!("[AI] Dropping persisted tool message with empty tool_call_id");
                    continue;
                }

                if !pending_tool_call_ids.contains(&call_id) {
                    tracing::warn!(
                        "[AI] Dropping persisted tool message with unmatched tool_call_id: {}",
                        call_id
                    );
                    continue;
                }

                pending_tool_call_ids.remove(&call_id);
                messages.push(GenaiMessage::from(genai::chat::ToolResponse::new(
                    call_id,
                    msg.content.unwrap_or_default(),
                )));
            }
            _ => {
                pending_tool_call_ids.clear();
                messages.push(GenaiMessage::user(msg.content.unwrap_or_default()));
            }
        }
    }

    messages
}

pub(super) fn truncate_dialog_messages_for_history(
    mut dialog_messages: Vec<ChatMessage>,
    max_history: usize,
) -> Vec<ChatMessage> {
    if dialog_messages.is_empty() || max_history == 0 {
        return Vec::new();
    }

    let len = dialog_messages.len();
    let tentative_start = len.saturating_sub(max_history);

    let safe_start = dialog_messages
        .iter()
        .enumerate()
        .skip(tentative_start)
        .find(|(_, msg)| msg.role == "user")
        .map(|(idx, _)| idx)
        .or_else(|| {
            if tentative_start == 0 {
                None
            } else {
                dialog_messages[..tentative_start]
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, msg)| msg.role == "user")
                    .map(|(idx, _)| idx)
            }
        });

    let Some(start_idx) = safe_start else {
        tracing::warn!(
            "[AI] No user message found in history window; dropping dialog history to avoid invalid tool-call turn ordering."
        );
        return Vec::new();
    };

    if start_idx != tentative_start {
        tracing::debug!(
            "[AI] Adjusted history window start from {} to {} to preserve user-turn boundary.",
            tentative_start,
            start_idx
        );
    }

    dialog_messages.split_off(start_idx)
}

pub(super) async fn load_history(
    state: &Arc<AppState>,
    session_id: &str,
    is_agent_mode: bool,
    ssh_session_id: Option<&str>,
) -> Result<Vec<ChatMessage>, String> {
    let server_id: Option<String> = {
        let session_id_owned = session_id.to_string();
        state
            .db_manager
            .run_blocking(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT server_id FROM ai_sessions WHERE id = ?1",
                        params![session_id_owned],
                        |row| row.get::<_, String>(0),
                    )
                    .ok())
            })
            .await?
    };

    let (max_history, global_prompt, server_prompt) = {
        let config = state.config.lock().await;
        let max = config.general.ai_max_history as usize;
        let gp = config.additional_prompt.clone();
        let sp = if let Some(sid) = &server_id {
            config
                .servers
                .iter()
                .find(|s| s.id == *sid)
                .and_then(|s| s.additional_prompt.clone())
        } else {
            None
        };
        (max, gp, sp)
    };

    let dialog_messages: Vec<ChatMessage> = {
        let session_id_owned = session_id.to_string();
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
                    .query_map(params![session_id_owned], |row| {
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

                let mut all_messages: Vec<ChatMessage> = Vec::new();
                for row in rows {
                    let msg: ChatMessage = row.map_err(|e| e.to_string())?;
                    all_messages.push(msg);
                }

                let dialog_messages: Vec<ChatMessage> = all_messages
                    .into_iter()
                    .filter(|m| m.role != "system")
                    .collect();

                Ok(truncate_dialog_messages_for_history(
                    dialog_messages,
                    max_history,
                ))
            })
            .await?
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

    let mut full_system_prompt = format!("{}\n\n{}", SYSTEM_PROMPT, mode_desc);

    if let Some(gp) = global_prompt {
        full_system_prompt.push_str("\n\nGlobal Additional Prompt:\n");
        full_system_prompt.push_str(&gp);
    }

    if let Some(sp) = server_prompt {
        full_system_prompt.push_str("\n\nServer Additional Prompt:\n");
        full_system_prompt.push_str(&sp);
    }

    full_system_prompt.push_str(&system_context);

    messages.push(ChatMessage {
        role: "system".to_string(),
        content: Some(full_system_prompt),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
        created_at: None,
        model_id: None,
    });

    messages.extend(dialog_messages);

    let mut payload_messages: Vec<MessagePayload> = messages
        .iter()
        .map(|m| MessagePayload {
            role: m.role.clone(),
            content: m.content.clone(),
            reasoning_content: m.reasoning_content.clone(),
            tool_calls: m.tool_calls.as_ref().map(|calls| {
                calls
                    .iter()
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
            created_at: m.created_at.clone(),
            model_id: m.model_id.clone(),
        })
        .collect();

    let result = validate_and_fix_messages(&mut payload_messages);
    if result.was_fixed {
        tracing::warn!("[AI] Message sequence fixed: {:?}", result.fixes);
    }

    let validated_messages: Vec<ChatMessage> = payload_messages
        .into_iter()
        .map(|m| ChatMessage {
            role: m.role,
            content: m.content,
            reasoning_content: m.reasoning_content,
            tool_calls: m.tool_calls.map(|calls| {
                calls
                    .iter()
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
            created_at: m.created_at,
            model_id: m.model_id,
        })
        .collect();

    Ok(validated_messages)
}
