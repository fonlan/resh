pub mod copilot;
mod history;
mod stream_parsing;
mod tool_registry;
mod tool_runtime;
mod types;

pub use tool_registry::create_tools;
pub use types::{
    AccessTokenResponse, ChatMessage, DeviceCodeResponse, FunctionCall, FunctionDefinition,
    ToolCall, ToolDefinition,
};

use history::{
    enrich_user_message_with_tagged_files, load_history, read_remote_file_via_sftp,
    to_genai_messages, READ_FILE_MAX_BYTES,
};
use stream_parsing::{
    accumulate_streamed_tool_call_chunk, append_reasoning_stream_text, append_response_stream_text,
    extract_command, extract_required_timeout_seconds, extract_think_segments, extract_timeout,
    extract_wait_finish, flush_reasoning_buffer, flush_response_buffer,
    flush_think_parser_remainder, normalize_streamed_tool_calls,
};
use tool_runtime::{
    build_recording_input_payload, build_run_in_terminal_timeout_failure_message,
    execute_command_in_exec_channel, try_reconnect_terminal_after_timeout,
    try_recover_terminal_after_timeout, START_MARKER_EXPECT_MS, TIMEOUT_RECOVERY_GRACE_MS,
};

use crate::commands::AppState;
use crate::ssh_manager::ssh::SSHClient;
use futures::StreamExt;
use genai::chat::{ChatMessage as GenaiMessage, ChatStreamEvent, Tool};
use rusqlite::params;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager, State, Window};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const AI_STREAM_IDLE_TIMEOUT_SECS: u64 = 45;

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
mod streamed_tool_call_tests {
    use super::*;

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
    use super::*;

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

async fn execute_tools_and_save(
    app_handle: tauri::AppHandle,
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
            "get_selected_terminal_output" => {
                if let Some(ssh_id) = ssh_session_id {
                    match SSHClient::get_selected_terminal_output(ssh_id).await {
                        Ok(text) => {
                            if text.is_empty() {
                                "Error: No text is currently selected in the terminal.".to_string()
                            } else {
                                String::from_utf8_lossy(&strip_ansi_escapes::strip(&text))
                                    .to_string()
                            }
                        }
                        Err(e) => format!("Error: {}", e),
                    }
                } else {
                    "Error: No active terminal session linked to this chat.".to_string()
                }
            }
            "read_file" => {
                if let Some(ssh_id) = ssh_session_id {
                    if let Ok(args) =
                        serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                    {
                        let remote_path = args
                            .get("remote_path")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .trim()
                            .to_string();

                        if remote_path.is_empty() {
                            "Error: Missing 'remote_path' argument".to_string()
                        } else {
                            match read_remote_file_via_sftp(
                                ssh_id,
                                &remote_path,
                                READ_FILE_MAX_BYTES,
                            )
                            .await
                            {
                                Ok((file_content, truncated)) => {
                                    let mut response =
                                        format!("[File: {}]\n{}", remote_path, file_content);
                                    if truncated {
                                        response.push_str(&format!(
                                            "\n[Truncated to first {} bytes]",
                                            READ_FILE_MAX_BYTES
                                        ));
                                    }
                                    response
                                }
                                Err(e) => format!("Error reading file '{}': {}", remote_path, e),
                            }
                        }
                    } else {
                        "Error: Invalid arguments JSON".to_string()
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
                            match extract_required_timeout_seconds(&args, "run_in_terminal") {
                                Ok(timeout) => {
                                    let wait_finish = args
                                        .get("wait_finish")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(true);

                                    if !wait_finish {
                                        tracing::debug!(
                                            "[execute_tools] Command '{}' sent with wait_finish=false",
                                            cmd
                                        );
                                        let cmd_nl = format!("{}\n", cmd);
                                        match SSHClient::send_input(ssh_id, cmd_nl.as_bytes()).await {
                                            Ok(_) => {
                                                "Command sent to terminal without waiting for completion (wait_finish=false).".to_string()
                                            }
                                            Err(e) => format!("Failed to send command: {}", e),
                                        }
                                    } else {
                                        SSHClient::start_command_recording(ssh_id).await?;
                                        tracing::debug!(
                                            "[execute_tools] Command '{}' sent, timeout={}s",
                                            cmd,
                                            timeout
                                        );

                                        let input_payload = build_recording_input_payload(
                                            ssh_id,
                                            cmd,
                                            "execute_tools",
                                        )
                                        .await;

                                        if let Err(e) =
                                            SSHClient::send_input(ssh_id, input_payload.as_bytes())
                                                .await
                                        {
                                            let _ = SSHClient::stop_command_recording(ssh_id).await;
                                            return Err(format!("Failed to send command: {}", e));
                                        }

                                        let mut interval = tokio::time::interval(
                                            tokio::time::Duration::from_millis(100),
                                        );
                                        let mut elapsed = 0u64;
                                        let timeout_ms = timeout * 1000;
                                        let mut completion_detected = false;
                                        let mut timed_out = false;
                                        let mut start_marker_seen = false;

                                        loop {
                                            if cancellation_token.is_cancelled() {
                                                SSHClient::stop_command_recording(ssh_id)
                                                    .await
                                                    .ok();
                                                return Err("CANCELLED".to_string());
                                            }
                                            interval.tick().await;
                                            elapsed += 100;

                                            if !start_marker_seen {
                                                start_marker_seen =
                                                    SSHClient::is_recording_start_marker_seen(
                                                        ssh_id,
                                                    )
                                                    .await
                                                    .unwrap_or(false);
                                            }

                                            let is_completed =
                                                SSHClient::check_command_completed(ssh_id).await?;
                                            if is_completed {
                                                completion_detected = true;
                                                tracing::debug!(
                                                    "[execute_tools] Command '{}' completed at {}ms",
                                                    cmd,
                                                    elapsed
                                                );
                                                break;
                                            }

                                            if elapsed >= START_MARKER_EXPECT_MS
                                                && !start_marker_seen
                                            {
                                                tracing::warn!(
                                                    "[execute_tools] Start marker not observed within {}ms for command '{}'; treating foreground channel as stalled",
                                                    START_MARKER_EXPECT_MS,
                                                    cmd
                                                );
                                                timed_out = true;
                                                break;
                                            }

                                            if elapsed >= timeout_ms {
                                                tracing::warn!(
                                                    "[execute_tools] Timeout reached at {}ms for command '{}'",
                                                    elapsed,
                                                    cmd
                                                );
                                                timed_out = true;
                                                break;
                                            }
                                        }

                                        if timed_out && !completion_detected {
                                            let recovered = try_recover_terminal_after_timeout(
                                                ssh_id,
                                                cmd,
                                                "execute_tools",
                                            )
                                            .await;
                                            if !recovered {
                                                tracing::warn!(
                                                    "[execute_tools] Terminal not recovered within {}ms grace after timeout for command '{}'",
                                                    TIMEOUT_RECOVERY_GRACE_MS,
                                                    cmd
                                                );
                                                let _ = try_reconnect_terminal_after_timeout(
                                                    ssh_id,
                                                    cmd,
                                                    "execute_tools",
                                                )
                                                .await;
                                            }
                                        }

                                        match SSHClient::stop_command_recording(ssh_id).await {
                                            Ok(output) => {
                                                let clean_output = String::from_utf8_lossy(
                                                    &strip_ansi_escapes::strip(output.as_bytes()),
                                                )
                                                .to_string();
                                                if timed_out && !completion_detected {
                                                    build_run_in_terminal_timeout_failure_message(
                                                        timeout,
                                                        &clean_output,
                                                    )
                                                } else if clean_output.trim().is_empty() {
                                                    "Command produced no output".to_string()
                                                } else {
                                                    clean_output
                                                }
                                            }
                                            Err(e) => format!("Error getting output: {}", e),
                                        }
                                    }
                                }
                                Err(err) => err,
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
            "run_in_background" => {
                if let Some(ssh_id) = ssh_session_id {
                    if let Ok(args) =
                        serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                    {
                        if let Some(cmd) = args["command"].as_str() {
                            match extract_required_timeout_seconds(&args, "run_in_background") {
                                Ok(timeout) => {
                                    match execute_command_in_exec_channel(
                                        ssh_id,
                                        cmd,
                                        timeout,
                                        Some(&cancellation_token),
                                    )
                                    .await
                                    {
                                        Ok(exec_result) => {
                                            let clean_output = String::from_utf8_lossy(
                                                &strip_ansi_escapes::strip(
                                                    exec_result.output.as_bytes(),
                                                ),
                                            )
                                            .to_string();

                                            if exec_result.timed_out {
                                                if clean_output.trim().is_empty() {
                                                    format!(
                                                        "Error: run_in_background timed out after {}s.",
                                                        timeout
                                                    )
                                                } else {
                                                    format!(
                                                        "Error: run_in_background timed out after {}s.\n\n[Partial output]\n{}",
                                                        timeout,
                                                        clean_output.trim()
                                                    )
                                                }
                                            } else if let Some(status) = exec_result.exit_status {
                                                if status != 0 {
                                                    if clean_output.trim().is_empty() {
                                                        format!("Error: run_in_background command exited with status {}.", status)
                                                    } else {
                                                        format!("Error: run_in_background command exited with status {}.\n\n{}", status, clean_output)
                                                    }
                                                } else if clean_output.trim().is_empty() {
                                                    "Command completed successfully with no output"
                                                        .to_string()
                                                } else {
                                                    clean_output
                                                }
                                            } else if clean_output.trim().is_empty() {
                                                "Command completed with no output".to_string()
                                            } else {
                                                clean_output
                                            }
                                        }
                                        Err(e) => {
                                            if e == "CANCELLED" {
                                                return Err(e);
                                            }
                                            format!("Error: {}", e)
                                        }
                                    }
                                }
                                Err(err) => err,
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
            "sftp_download" => {
                if let Some(ssh_id) = ssh_session_id {
                    if let Ok(args) =
                        serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                    {
                        let remote_path =
                            args["remote_path"].as_str().unwrap_or_default().to_string();
                        let local_path = args["local_path"].as_str().map(|s| s.to_string());

                        if remote_path.is_empty() {
                            "Error: Missing 'remote_path' argument".to_string()
                        } else {
                            let metadata_res = tokio::time::timeout(
                                std::time::Duration::from_secs(5),
                                crate::sftp_manager::SftpManager::metadata(ssh_id, &remote_path),
                            )
                            .await;

                            match metadata_res {
                                Ok(Ok(_)) => {
                                    let final_local_path = if let Some(lp) = local_path {
                                        lp
                                    } else {
                                        let config = state.config.lock().await;
                                        let default_path = config.general.sftp.default_download_path.clone();
                                        if default_path.is_empty() {
                                            app_handle.path().download_dir()
                                                .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default())
                                                .join(std::path::Path::new(&remote_path).file_name().unwrap_or_default())
                                                .to_string_lossy()
                                                .to_string()
                                        } else {
                                            std::path::Path::new(&default_path)
                                                .join(std::path::Path::new(&remote_path).file_name().unwrap_or_default())
                                                .to_string_lossy()
                                                .to_string()
                                        }
                                    };

                                    let local_p = std::path::Path::new(&final_local_path);
                                    let mut parent_ok = true;
                                    let mut prep_error = String::new();
                                    if let Some(parent) = local_p.parent() {
                                        if !parent.exists() {
                                            if let Err(e) = std::fs::create_dir_all(parent) {
                                                parent_ok = false;
                                                prep_error = format!("Error creating local directory: {}", e);
                                            }
                                        }
                                    }

                                    if parent_ok {
                                    match crate::sftp_manager::SftpManager::download_file(
                                        app_handle.clone(),
                                        state.db_manager.clone(),
                                        ssh_id.to_string(),
                                        remote_path,
                                        final_local_path.clone(),
                                        None,
                                        Some(session_id.to_string()),
                                    ).await {
                                            Ok(task_id) => format!("Download started in background. Task ID: {}. Local path: {}. I will notify you once it's finished.", task_id, final_local_path),
                                            Err(e) => format!("Error starting download: {}", e),
                                        }
                                    } else {
                                        prep_error
                                    }
                                }
                                Ok(Err(_)) => format!("Error: Remote path '{}' does not exist or is inaccessible. Please confirm the path with the user.", remote_path),
                                Err(_) => {
                                match crate::sftp_manager::SftpManager::download_file(
                                    app_handle.clone(),
                                    state.db_manager.clone(),
                                    ssh_id.to_string(),
                                    remote_path,
                                    "queued_download".to_string(),
                                    None,
                                    Some(session_id.to_string()),
                                ).await {
                                        Ok(task_id) => format!("SFTP session is busy. Download task {} has been queued and will start as soon as possible.", task_id),
                                        Err(e) => format!("Error queuing download: {}", e),
                                    }
                                }
                            }
                        }
                    } else {
                        "Error: Invalid arguments JSON".to_string()
                    }
                } else {
                    "Error: No active terminal session linked to this chat.".to_string()
                }
            }
            "sftp_upload" => {
                if let Some(ssh_id) = ssh_session_id {
                    if let Ok(args) =
                        serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                    {
                        let local_path =
                            args["local_path"].as_str().unwrap_or_default().to_string();
                        let remote_path =
                            args["remote_path"].as_str().unwrap_or_default().to_string();

                        if local_path.is_empty() || remote_path.is_empty() {
                            "Error: Missing 'local_path' or 'remote_path' argument".to_string()
                        } else {
                            let local_p = std::path::Path::new(&local_path);
                            if !local_p.exists() {
                                format!("Error: Local source path '{}' does not exist.", local_path)
                            } else {
                                let remote_p = std::path::Path::new(&remote_path);
                                let remote_parent =
                                    remote_p.parent().and_then(|p| p.to_str()).unwrap_or("/");

                                let metadata_res = tokio::time::timeout(
                                    std::time::Duration::from_secs(5),
                                    crate::sftp_manager::SftpManager::metadata(
                                        ssh_id,
                                        remote_parent,
                                    ),
                                )
                                .await;

                                match metadata_res {
                                    Ok(Ok(_)) => {
                                        match crate::sftp_manager::SftpManager::upload_file(
                                            app_handle.clone(),
                                            state.db_manager.clone(),
                                            ssh_id.to_string(),
                                            local_path,
                                            remote_path.clone(),
                                            None,
                                            Some(session_id.to_string()),
                                        ).await {
                                            Ok(task_id) => format!("Upload started in background. Task ID: {}. Target: {}. I will notify you once it's finished.", task_id, remote_path),
                                            Err(e) => format!("Error starting upload: {}", e),
                                        }
                                    }
                                    Ok(Err(_)) => format!("Error: Remote target directory '{}' does not exist. Please confirm with the user whether to create it.", remote_parent),
                                    Err(_) => {
                                        match crate::sftp_manager::SftpManager::upload_file(
                                            app_handle.clone(),
                                            state.db_manager.clone(),
                                            ssh_id.to_string(),
                                            local_path,
                                            remote_path.clone(),
                                            None,
                                            Some(session_id.to_string()),
                                        ).await {
                                            Ok(task_id) => format!("SFTP session is busy. Upload task {} has been queued and will start as soon as possible.", task_id),
                                            Err(e) => format!("Error queuing upload: {}", e),
                                        }
                                    }
                                }
                            }
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
        let tool_call_id = call.id.clone();
        let tool_content = result.clone();
        {
            let session_id_owned = session_id.to_string();
            let tool_msg_id_owned = tool_msg_id.clone();
            let tool_call_id_owned = tool_call_id.clone();
            let result_owned = result.clone();
            state
                .db_manager
                .run_blocking(move |conn| {
                    conn.execute(
                        "INSERT INTO ai_messages (id, session_id, role, content, tool_call_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![tool_msg_id_owned, session_id_owned, "tool", result_owned, tool_call_id_owned],
                    )
                    .map_err(|e| e.to_string())?;
                    Ok(())
                })
                .await?;
        }

        let _ = app_handle.emit(
            &format!("ai-message-batch-{}", session_id),
            vec![ChatMessage {
                role: "tool".to_string(),
                content: Some(tool_content),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: Some(call.id.clone()),
                created_at: None,
                model_id: None,
            }],
        );
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

        let history: Vec<ChatMessage> = load_history(
            &state,
            &session_id,
            is_agent_mode,
            ssh_session_id.as_deref(),
        )
        .await?;
        let genai_history = to_genai_messages(history);

        let genai_tools: Option<Vec<Tool>> = tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| {
                    Tool::new(t.function.name.clone())
                        .with_description(t.function.description.clone())
                        .with_schema(t.function.parameters.clone())
                })
                .collect()
        });

        let mut stream = state
            .ai_manager
            .stream_chat(&channel, &model, genai_history, genai_tools, proxy)
            .await?;

        let mut full_content = String::new();
        let mut full_reasoning = String::new();
        let mut final_tool_calls: Option<Vec<ToolCall>> = None;
        let mut accumulated_tool_calls: Vec<ToolCall> = Vec::new();
        let mut tool_call_id_aliases: HashMap<String, String> = HashMap::new();
        let mut has_pending_reasoning = false;
        let mut response_emit_buffer = String::new();
        let mut reasoning_emit_buffer = String::new();
        let mut think_parser_buffer = String::new();
        let mut in_think_block = false;
        let mut last_emit_at = Instant::now();

        let response_event = format!("ai-response-{}", session_id);
        let reasoning_event = format!("ai-reasoning-{}", session_id);
        let reasoning_end_event = format!("ai-reasoning-end-{}", session_id);
        let error_event = format!("ai-error-{}", session_id);

        loop {
            let next_event = tokio::select! {
                _ = cancellation_token.cancelled() => {
                    flush_think_parser_remainder(
                        &window,
                        &response_event,
                        &reasoning_event,
                        &reasoning_end_event,
                        &mut think_parser_buffer,
                        in_think_block,
                        &mut full_content,
                        &mut full_reasoning,
                        &mut response_emit_buffer,
                        &mut reasoning_emit_buffer,
                        &mut has_pending_reasoning,
                        &mut last_emit_at,
                    )?;
                    flush_response_buffer(&window, &response_event, &mut response_emit_buffer)?;
                    flush_reasoning_buffer(&window, &reasoning_event, &mut reasoning_emit_buffer)?;
                    if has_pending_reasoning {
                        window
                            .emit(&reasoning_end_event, "end")
                            .map_err(|e| e.to_string())?;
                    }
                    return Err("CANCELLED".to_string());
                }
                maybe_event = tokio::time::timeout(
                    Duration::from_secs(AI_STREAM_IDLE_TIMEOUT_SECS),
                    stream.next()
                ) => {
                    match maybe_event {
                        Ok(event) => event,
                        Err(_) => {
                            flush_think_parser_remainder(
                                &window,
                                &response_event,
                                &reasoning_event,
                                &reasoning_end_event,
                                &mut think_parser_buffer,
                                in_think_block,
                                &mut full_content,
                                &mut full_reasoning,
                                &mut response_emit_buffer,
                                &mut reasoning_emit_buffer,
                                &mut has_pending_reasoning,
                                &mut last_emit_at,
                            )?;
                            flush_response_buffer(&window, &response_event, &mut response_emit_buffer)?;
                            flush_reasoning_buffer(&window, &reasoning_event, &mut reasoning_emit_buffer)?;
                            if has_pending_reasoning {
                                window
                                    .emit(&reasoning_end_event, "end")
                                    .map_err(|e| e.to_string())?;
                            }

                            let err_msg = format!(
                                "AI stream stalled for {} seconds without new events.",
                                AI_STREAM_IDLE_TIMEOUT_SECS
                            );
                            tracing::warn!(
                                "[AI] {} session_id={}, pending_tool_fragments={}",
                                err_msg,
                                session_id,
                                accumulated_tool_calls.len()
                            );
                            window
                                .emit(&error_event, err_msg.clone())
                                .map_err(|e| e.to_string())?;
                            return Err(err_msg);
                        }
                    }
                }
            };

            let Some(event_result) = next_event else {
                break;
            };

            match event_result {
                Ok(event) => match event {
                    ChatStreamEvent::Chunk(chunk) => {
                        think_parser_buffer.push_str(&chunk.content);
                        let segments =
                            extract_think_segments(&mut think_parser_buffer, &mut in_think_block);
                        for (is_reasoning, segment) in segments {
                            if is_reasoning {
                                append_reasoning_stream_text(
                                    &window,
                                    &reasoning_event,
                                    &segment,
                                    &mut full_reasoning,
                                    &mut reasoning_emit_buffer,
                                    &mut has_pending_reasoning,
                                    &mut last_emit_at,
                                )?;
                            } else {
                                append_response_stream_text(
                                    &window,
                                    &response_event,
                                    &reasoning_event,
                                    &reasoning_end_event,
                                    &segment,
                                    &mut full_content,
                                    &mut response_emit_buffer,
                                    &mut reasoning_emit_buffer,
                                    &mut has_pending_reasoning,
                                    &mut last_emit_at,
                                )?;
                            }
                        }
                    }
                    ChatStreamEvent::ReasoningChunk(chunk) => {
                        append_reasoning_stream_text(
                            &window,
                            &reasoning_event,
                            &chunk.content,
                            &mut full_reasoning,
                            &mut reasoning_emit_buffer,
                            &mut has_pending_reasoning,
                            &mut last_emit_at,
                        )?;
                    }
                    ChatStreamEvent::ToolCallChunk(chunk) => {
                        flush_think_parser_remainder(
                            &window,
                            &response_event,
                            &reasoning_event,
                            &reasoning_end_event,
                            &mut think_parser_buffer,
                            in_think_block,
                            &mut full_content,
                            &mut full_reasoning,
                            &mut response_emit_buffer,
                            &mut reasoning_emit_buffer,
                            &mut has_pending_reasoning,
                            &mut last_emit_at,
                        )?;
                        flush_response_buffer(&window, &response_event, &mut response_emit_buffer)?;
                        flush_reasoning_buffer(
                            &window,
                            &reasoning_event,
                            &mut reasoning_emit_buffer,
                        )?;
                        last_emit_at = Instant::now();

                        tracing::debug!(
                            "[AI] ToolCallChunk raw: call_id={}, fn_name={}, fn_arguments={:?}",
                            chunk.tool_call.call_id,
                            chunk.tool_call.fn_name,
                            chunk.tool_call.fn_arguments
                        );

                        let args = chunk
                            .tool_call
                            .fn_arguments
                            .as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| chunk.tool_call.fn_arguments.to_string());

                        tracing::debug!("[AI] ToolCallChunk extracted args: \"{}\"", args);

                        if chunk.tool_call.fn_name.is_empty() && args.is_empty() {
                            tracing::debug!("[AI] ToolCallChunk (skipped): empty fn_name and args");
                        } else {
                            let before_len = accumulated_tool_calls.len();
                            accumulate_streamed_tool_call_chunk(
                                &mut accumulated_tool_calls,
                                &mut tool_call_id_aliases,
                                &chunk.tool_call.call_id,
                                &chunk.tool_call.fn_name,
                                &args,
                            );

                            if let Some(alias_target) =
                                tool_call_id_aliases.get(&chunk.tool_call.call_id)
                            {
                                if let Some(existing) = accumulated_tool_calls
                                    .iter()
                                    .find(|tc| tc.id == alias_target.as_str())
                                {
                                    if accumulated_tool_calls.len() > before_len {
                                        tracing::debug!(
                                            "[AI] ToolCallChunk (new): src_id={}, mapped_id={}, name={}, args_len={}, total_len={}",
                                            chunk.tool_call.call_id,
                                            alias_target,
                                            existing.function.name,
                                            args.len(),
                                            existing.function.arguments.len()
                                        );
                                    } else {
                                        tracing::debug!(
                                            "[AI] ToolCallChunk (merged): src_id={}, mapped_id={}, name={}, args_len={}, total_len={}",
                                            chunk.tool_call.call_id,
                                            alias_target,
                                            existing.function.name,
                                            args.len(),
                                            existing.function.arguments.len()
                                        );
                                    }
                                }
                            }
                        }
                    }
                    ChatStreamEvent::End(end) => {
                        flush_think_parser_remainder(
                            &window,
                            &response_event,
                            &reasoning_event,
                            &reasoning_end_event,
                            &mut think_parser_buffer,
                            in_think_block,
                            &mut full_content,
                            &mut full_reasoning,
                            &mut response_emit_buffer,
                            &mut reasoning_emit_buffer,
                            &mut has_pending_reasoning,
                            &mut last_emit_at,
                        )?;
                        if has_pending_reasoning {
                            window
                                .emit(&format!("ai-reasoning-end-{}", session_id), "end")
                                .map_err(|e| e.to_string())?;
                            has_pending_reasoning = false;
                        }

                        let tool_calls = if let Some(content) = end.captured_content {
                            let raw_tool_calls = content.tool_calls();
                            tracing::debug!(
                                "[AI] End event captured_content tool_calls count: {}",
                                raw_tool_calls.len()
                            );

                            let calls_from_captured: Vec<ToolCall> = raw_tool_calls
                                .into_iter()
                                .filter(|tc| !tc.fn_name.is_empty())
                                .map(|tc| {
                                    let args = tc
                                        .fn_arguments
                                        .as_str()
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|| tc.fn_arguments.to_string());
                                    ToolCall {
                                        id: tc.call_id.clone(),
                                        tool_type: "function".to_string(),
                                        function: FunctionCall {
                                            name: tc.fn_name.clone(),
                                            arguments: args,
                                        },
                                    }
                                })
                                .collect();

                            if calls_from_captured.is_empty() && !accumulated_tool_calls.is_empty()
                            {
                                tracing::debug!("[AI] captured_content tool_calls is empty, fallback to accumulated tool_calls: {}", accumulated_tool_calls.len());
                                for acc_call in &accumulated_tool_calls {
                                    tracing::debug!(
                                        "[AI] Accumulated tool call: id={}, name={}, args=\"{}\"",
                                        acc_call.id,
                                        acc_call.function.name,
                                        acc_call.function.arguments
                                    );
                                }
                                accumulated_tool_calls.clone()
                            } else {
                                calls_from_captured
                            }
                        } else {
                            tracing::debug!("[AI] End event has no captured_content, using accumulated tool_calls: {}", accumulated_tool_calls.len());
                            for acc_call in &accumulated_tool_calls {
                                tracing::debug!(
                                    "[AI] Accumulated tool call: id={}, name={}, args=\"{}\"",
                                    acc_call.id,
                                    acc_call.function.name,
                                    acc_call.function.arguments
                                );
                            }
                            accumulated_tool_calls.clone()
                        };

                        let tool_calls = normalize_streamed_tool_calls(tool_calls);

                        for call in &tool_calls {
                            let timeout = extract_timeout(&call.function.arguments);
                            let command = extract_command(&call.function.arguments);
                            let wait_finish = extract_wait_finish(&call.function.arguments);
                            let timeout_display = timeout
                                .map(|value| format!("{}s", value))
                                .unwrap_or_else(|| "missing".to_string());
                            tracing::info!(
                                    "[AI] Tool call received: id={}, name={}, command=\"{}\", timeout={}, wait_finish={}, raw_args=\"{}\"",
                                    call.id,
                                    call.function.name,
                                    command.as_deref().unwrap_or("N/A"),
                                    timeout_display,
                                    wait_finish.unwrap_or(true),
                                    call.function.arguments
                                );
                        }

                        if !tool_calls.is_empty() {
                            *final_tool_calls.get_or_insert_with(Vec::new) = tool_calls;
                        }
                    }
                    _ => {}
                },
                Err(e) => {
                    flush_think_parser_remainder(
                        &window,
                        &response_event,
                        &reasoning_event,
                        &reasoning_end_event,
                        &mut think_parser_buffer,
                        in_think_block,
                        &mut full_content,
                        &mut full_reasoning,
                        &mut response_emit_buffer,
                        &mut reasoning_emit_buffer,
                        &mut has_pending_reasoning,
                        &mut last_emit_at,
                    )?;
                    flush_response_buffer(&window, &response_event, &mut response_emit_buffer)?;
                    flush_reasoning_buffer(&window, &reasoning_event, &mut reasoning_emit_buffer)?;

                    if has_pending_reasoning {
                        window.emit(&reasoning_end_event, "end").ok();
                    }

                    let err_msg = e.to_string();
                    window
                        .emit(&error_event, err_msg.clone())
                        .map_err(|e| e.to_string())?;
                    return Err(err_msg);
                }
            }
        }

        flush_think_parser_remainder(
            &window,
            &response_event,
            &reasoning_event,
            &reasoning_end_event,
            &mut think_parser_buffer,
            in_think_block,
            &mut full_content,
            &mut full_reasoning,
            &mut response_emit_buffer,
            &mut reasoning_emit_buffer,
            &mut has_pending_reasoning,
            &mut last_emit_at,
        )?;
        flush_response_buffer(&window, &response_event, &mut response_emit_buffer)?;
        flush_reasoning_buffer(&window, &reasoning_event, &mut reasoning_emit_buffer)?;

        if has_pending_reasoning {
            window.emit(&reasoning_end_event, "end").ok();
        }

        let ai_msg_id = Uuid::new_v4().to_string();
        {
            let tool_calls_json = if let Some(calls) = &final_tool_calls {
                Some(serde_json::to_string(calls).map_err(|e| format!("序列化失败: {}", e))?)
            } else {
                None
            };

            if !full_content.is_empty() || final_tool_calls.is_some() || !full_reasoning.is_empty()
            {
                let ai_msg_id_owned = ai_msg_id.clone();
                let session_id_owned = session_id.clone();
                let full_content_owned = full_content.clone();
                let full_reasoning_owned = full_reasoning.clone();
                let model_id_owned = model.id.clone();
                state
                    .db_manager
                    .run_blocking(move |conn| {
                        conn.execute(
                            "INSERT INTO ai_messages (id, session_id, role, content, reasoning_content, tool_calls, model_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                            params![ai_msg_id_owned, session_id_owned, "assistant", full_content_owned, full_reasoning_owned, tool_calls_json, model_id_owned],
                        )
                        .map_err(|e| e.to_string())?;
                        Ok(())
                    })
                    .await?;
            }
        }

        if let Some(calls) = &final_tool_calls {
            if !calls.is_empty() {
                let auto_exec_calls: Vec<ToolCall> = calls
                    .iter()
                    .filter(|c| {
                        c.function.name == "get_terminal_output"
                            || c.function.name == "get_selected_terminal_output"
                            || c.function.name == "read_file"
                    })
                    .cloned()
                    .collect();

                let confirm_calls: Vec<ToolCall> = calls
                    .iter()
                    .filter(|c| {
                        c.function.name != "get_terminal_output"
                            && c.function.name != "get_selected_terminal_output"
                            && c.function.name != "read_file"
                    })
                    .cloned()
                    .collect();

                if !auto_exec_calls.is_empty() {
                    tracing::info!(
                        "[AI] Auto-executing {} read-only tools (get_terminal_output/get_selected_terminal_output/read_file)",
                        auto_exec_calls.len()
                    );
                    execute_tools_and_save(
                        window.app_handle().clone(),
                        &state,
                        &session_id,
                        ssh_session_id.as_deref(),
                        auto_exec_calls,
                        cancellation_token.clone(),
                    )
                    .await?;
                }

                if !confirm_calls.is_empty() {
                    if is_agent_mode {
                        tracing::info!(
                            "[AI] {} tools need confirmation, emitting for frontend countdown",
                            confirm_calls.len()
                        );
                        window
                            .emit(
                                &format!("ai-tool-call-{}", session_id),
                                confirm_calls.clone(),
                            )
                            .map_err(|e| e.to_string())?;
                        return Ok(Some(confirm_calls));
                    } else {
                        tracing::warn!("[AI] Ask mode cannot execute tools other than read-only tools (get_terminal_output/get_selected_terminal_output/read_file), ignoring {} tool calls", confirm_calls.len());
                    }
                }

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
                )
                .await;
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
    session_id: String,
    command: String,
    timeout_seconds: Option<u64>,
    wait_finish: Option<bool>,
) -> Result<String, String> {
    let should_wait_finish = wait_finish.unwrap_or(true);
    if !should_wait_finish {
        let cmd_nl = format!("{}\n", command);
        SSHClient::send_input(&session_id, cmd_nl.as_bytes())
            .await
            .map_err(|e| format!("Failed to send command: {}", e))?;
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

    if let Err(e) = SSHClient::send_input(&session_id, input_payload.as_bytes()).await {
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
        .stream_chat(&channel, &model, title_messages, None, proxy)
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
