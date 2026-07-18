use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use genai::chat::{ChatStreamEvent, Tool};
use rusqlite::{params, Connection, OptionalExtension};
use tauri::{Emitter, Manager, Window};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::history::{
    load_history, read_remote_file_via_sftp, to_genai_messages, ProviderCapabilities,
    READ_FILE_MAX_BYTES,
};
use super::stream_parsing::{
    accumulate_streamed_tool_call_chunk, append_reasoning_stream_text, append_response_stream_text,
    extract_required_timeout_seconds, extract_think_segments, extract_timeout,
    flush_reasoning_buffer, flush_response_buffer, flush_think_parser_remainder,
    merge_captured_and_streamed_tool_calls, normalize_streamed_tool_calls,
};
use super::tool_registry::{
    is_session_grant_eligible, tool_policy, PreparedToolCall, ToolPolicyEngine, ToolPreparation,
};
use super::tool_runtime::{
    build_recording_input_payload, build_run_in_terminal_timeout_failure_message,
    execute_command_in_exec_channel, try_reconnect_terminal_after_timeout,
    try_recover_terminal_after_timeout, START_MARKER_EXPECT_MS, TIMEOUT_RECOVERY_GRACE_MS,
};
use super::types::{
    AiMessageBatchPayload, AiReasoningEndPayload, AiToolCallEventPayload, ChatMessage,
    FunctionCall, ToolCall, ToolDefinition, ToolExecution, ToolOutcome, ToolOutcomeStatus,
};
use super::{AI_CANCELLED, AI_STREAM_IDLE_TIMEOUT_SECS};
use crate::commands::AppState;
use crate::ssh_manager::ssh::SSHClient;

/// Race a future against cancellation. Used for tool awaits that do not natively
/// observe the token (SFTP read, terminal buffer fetch, etc.).
async fn await_or_cancel<T, F>(token: &CancellationToken, fut: F) -> Result<T, String>
where
    F: std::future::Future<Output = T>,
{
    tokio::select! {
        biased;
        _ = token.cancelled() => Err(AI_CANCELLED.to_string()),
        value = fut => Ok(value),
    }
}

/// Stop command recording without blocking the AI cancel path.
/// SSH cleanup can hang; cancelled runs must still exit at the next scheduling point.
fn spawn_stop_command_recording(ssh_id: &str) {
    let ssh_id = ssh_id.to_string();
    tokio::spawn(async move {
        let _ = SSHClient::stop_command_recording(&ssh_id).await;
    });
}

async fn persist_tool_outcome(
    app_handle: &tauri::AppHandle,
    state: &Arc<AppState>,
    session_id: &str,
    request_id: &str,
    run_id: Option<&str>,
    turn_index: Option<i64>,
    outcome: &ToolOutcome,
) -> Result<(), String> {
    let tool_msg_id = Uuid::new_v4().to_string();
    let tool_call_id = outcome.tool_call_id.clone();
    let content = outcome.observation();
    let session_id = session_id.to_string();
    let session_id_for_db = session_id.clone();
    let run_id = run_id.map(str::to_string);
    let message_content = content.clone();
    state
        .db_manager
        .run_blocking(move |conn| {
            conn.execute(
                "INSERT INTO ai_messages (id, session_id, role, content, tool_call_id, run_id, turn_index)
                 VALUES (?1, ?2, 'tool', ?3, ?4, ?5, ?6)",
                params![
                    tool_msg_id,
                    session_id_for_db,
                    message_content,
                    tool_call_id,
                    run_id,
                    turn_index
                ],
            )
            .map_err(|error| error.to_string())?;
            Ok(())
        })
        .await?;
    let _ = app_handle.emit(
        &format!("ai-message-batch-{}", session_id),
        AiMessageBatchPayload {
            request_id: request_id.to_string(),
            messages: vec![ChatMessage {
                role: "tool".to_string(),
                content: Some(content),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: Some(outcome.tool_call_id.clone()),
                created_at: None,
                model_id: None,
            }],
        },
    );
    Ok(())
}

async fn execute_tools(
    app_handle: tauri::AppHandle,
    state: &Arc<AppState>,
    session_id: &str,
    ssh_session_id: Option<&str>,
    tools: Vec<ToolCall>,
    cancellation_token: CancellationToken,
) -> Result<Vec<ToolOutcome>, String> {
    let mut outcomes = Vec::with_capacity(tools.len());
    for call in tools {
        if cancellation_token.is_cancelled() {
            return Err(AI_CANCELLED.to_string());
        }

        let result = match call.function.name.as_str() {
            "get_terminal_output" => {
                if let Some(ssh_id) = ssh_session_id {
                    let ssh_id = ssh_id.to_string();
                    match await_or_cancel(&cancellation_token, async move {
                        SSHClient::get_terminal_output(&ssh_id).await
                    })
                    .await
                    {
                        Err(e) if e == AI_CANCELLED => return Err(AI_CANCELLED.to_string()),
                        Ok(Ok(text)) => {
                            String::from_utf8_lossy(&strip_ansi_escapes::strip(&text)).to_string()
                        }
                        Ok(Err(e)) => format!("Error: {}", e),
                        Err(e) => format!("Error: {}", e),
                    }
                } else {
                    "Error: No active terminal session linked to this chat.".to_string()
                }
            }
            "get_selected_terminal_output" => {
                if let Some(ssh_id) = ssh_session_id {
                    let ssh_id = ssh_id.to_string();
                    match await_or_cancel(&cancellation_token, async move {
                        SSHClient::get_selected_terminal_output(&ssh_id).await
                    })
                    .await
                    {
                        Err(e) if e == AI_CANCELLED => return Err(AI_CANCELLED.to_string()),
                        Ok(Ok(text)) => {
                            if text.is_empty() {
                                "Error: No text is currently selected in the terminal.".to_string()
                            } else {
                                String::from_utf8_lossy(&strip_ansi_escapes::strip(&text))
                                    .to_string()
                            }
                        }
                        Ok(Err(e)) => format!("Error: {}", e),
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
                            let ssh_id = ssh_id.to_string();
                            let path_for_err = remote_path.clone();
                            match await_or_cancel(&cancellation_token, async move {
                                read_remote_file_via_sftp(
                                    &ssh_id,
                                    &remote_path,
                                    READ_FILE_MAX_BYTES,
                                )
                                .await
                            })
                            .await
                            {
                                Err(e) if e == AI_CANCELLED => {
                                    return Err(AI_CANCELLED.to_string());
                                }
                                Ok(Ok((file_content, truncated))) => {
                                    let mut response =
                                        format!("[File: {}]\n{}", path_for_err, file_content);
                                    if truncated {
                                        response.push_str(&format!(
                                            "\n[Truncated to first {} bytes]",
                                            READ_FILE_MAX_BYTES
                                        ));
                                    }
                                    response
                                }
                                Ok(Err(e)) => {
                                    format!("Error reading file '{}': {}", path_for_err, e)
                                }
                                Err(e) => format!("Error reading file '{}': {}", path_for_err, e),
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
                                        let command_block_event =
                                            format!("terminal-command-block:{}", ssh_id);
                                        let _ = app_handle.emit(&command_block_event, "start");
                                        let ssh_id_owned = ssh_id.to_string();
                                        match await_or_cancel(&cancellation_token, async move {
                                            SSHClient::send_input(&ssh_id_owned, cmd_nl.as_bytes())
                                                .await
                                        })
                                        .await
                                        {
                                            Err(e) if e == AI_CANCELLED => {
                                                let _ =
                                                    app_handle.emit(&command_block_event, "end");
                                                return Err(AI_CANCELLED.to_string());
                                            }
                                            Ok(Ok(_)) => {
                                                "Command sent to terminal without waiting for completion (wait_finish=false).".to_string()
                                            }
                                            Ok(Err(e)) => {
                                                let _ =
                                                    app_handle.emit(&command_block_event, "end");
                                                format!("Failed to send command: {}", e)
                                            }
                                            Err(e) => {
                                                let _ =
                                                    app_handle.emit(&command_block_event, "end");
                                                format!("Failed to send command: {}", e)
                                            }
                                        }
                                    } else {
                                        let ssh_id_for_start = ssh_id.to_string();
                                        match await_or_cancel(&cancellation_token, async move {
                                            SSHClient::start_command_recording(&ssh_id_for_start)
                                                .await
                                        })
                                        .await
                                        {
                                            Err(e) if e == AI_CANCELLED => {
                                                return Err(AI_CANCELLED.to_string());
                                            }
                                            Ok(Err(e)) => return Err(e),
                                            Err(e) => return Err(e),
                                            Ok(Ok(())) => {}
                                        }
                                        tracing::debug!(
                                            "[execute_tools] Command '{}' sent, timeout={}s",
                                            cmd,
                                            timeout
                                        );

                                        let input_payload =
                                            match await_or_cancel(&cancellation_token, async {
                                                build_recording_input_payload(
                                                    ssh_id,
                                                    cmd,
                                                    "execute_tools",
                                                )
                                                .await
                                            })
                                            .await
                                            {
                                                Err(e) if e == AI_CANCELLED => {
                                                    spawn_stop_command_recording(ssh_id);
                                                    return Err(AI_CANCELLED.to_string());
                                                }
                                                Ok(payload) => payload,
                                                Err(e) => {
                                                    let _ =
                                                        SSHClient::stop_command_recording(ssh_id)
                                                            .await;
                                                    return Err(e);
                                                }
                                            };

                                        let command_block_event =
                                            format!("terminal-command-block:{}", ssh_id);
                                        let _ = app_handle.emit(&command_block_event, "start");

                                        let ssh_id_for_send = ssh_id.to_string();
                                        let input_bytes = input_payload.into_bytes();
                                        match await_or_cancel(&cancellation_token, async move {
                                            SSHClient::send_input(&ssh_id_for_send, &input_bytes)
                                                .await
                                        })
                                        .await
                                        {
                                            Err(e) if e == AI_CANCELLED => {
                                                let _ =
                                                    app_handle.emit(&command_block_event, "end");
                                                spawn_stop_command_recording(ssh_id);
                                                return Err(AI_CANCELLED.to_string());
                                            }
                                            Ok(Ok(())) => {}
                                            Ok(Err(e)) => {
                                                let _ =
                                                    app_handle.emit(&command_block_event, "end");
                                                let _ =
                                                    SSHClient::stop_command_recording(ssh_id).await;
                                                return Err(format!(
                                                    "Failed to send command: {}",
                                                    e
                                                ));
                                            }
                                            Err(e) => {
                                                let _ =
                                                    app_handle.emit(&command_block_event, "end");
                                                let _ =
                                                    SSHClient::stop_command_recording(ssh_id).await;
                                                return Err(format!(
                                                    "Failed to send command: {}",
                                                    e
                                                ));
                                            }
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
                                            tokio::select! {
                                                biased;
                                                _ = cancellation_token.cancelled() => {
                                                    let _ =
                                                        app_handle.emit(&command_block_event, "end");
                                                    spawn_stop_command_recording(ssh_id);
                                                    return Err(AI_CANCELLED.to_string());
                                                }
                                                _ = interval.tick() => {}
                                            }
                                            elapsed += 100;

                                            if !start_marker_seen {
                                                let ssh_id_marker = ssh_id.to_string();
                                                match await_or_cancel(
                                                    &cancellation_token,
                                                    async move {
                                                        SSHClient::is_recording_start_marker_seen(
                                                            &ssh_id_marker,
                                                        )
                                                        .await
                                                    },
                                                )
                                                .await
                                                {
                                                    Err(e) if e == AI_CANCELLED => {
                                                        let _ = app_handle
                                                            .emit(&command_block_event, "end");
                                                        spawn_stop_command_recording(ssh_id);
                                                        return Err(AI_CANCELLED.to_string());
                                                    }
                                                    Ok(Ok(seen)) => start_marker_seen = seen,
                                                    Ok(Err(_)) | Err(_) => {
                                                        start_marker_seen = false
                                                    }
                                                }
                                            }

                                            let is_completed = {
                                                let ssh_id_check = ssh_id.to_string();
                                                match await_or_cancel(
                                                    &cancellation_token,
                                                    async move {
                                                        SSHClient::check_command_completed(
                                                            &ssh_id_check,
                                                        )
                                                        .await
                                                    },
                                                )
                                                .await
                                                {
                                                    Err(e) if e == AI_CANCELLED => {
                                                        let _ = app_handle
                                                            .emit(&command_block_event, "end");
                                                        spawn_stop_command_recording(ssh_id);
                                                        return Err(AI_CANCELLED.to_string());
                                                    }
                                                    Ok(Ok(is_completed)) => is_completed,
                                                    Ok(Err(e)) => {
                                                        let _ = app_handle
                                                            .emit(&command_block_event, "end");
                                                        return Err(e);
                                                    }
                                                    Err(e) => {
                                                        let _ = app_handle
                                                            .emit(&command_block_event, "end");
                                                        return Err(e);
                                                    }
                                                }
                                            };
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

                                        let _ = app_handle.emit(&command_block_event, "end");

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
                                            if e == AI_CANCELLED {
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

        let status = if result.starts_with("Error:") || result.starts_with("Failed") {
            ToolOutcomeStatus::Failed
        } else {
            ToolOutcomeStatus::Completed
        };
        let outcome = ToolOutcome {
            tool_call_id: call.id.clone(),
            status,
            content: result,
        };
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

pub(super) fn apply_reasoning_fallback(
    full_reasoning: &mut String,
    captured_reasoning_content: Option<String>,
) {
    if !full_reasoning.is_empty() {
        return;
    }

    if let Some(reasoning) = captured_reasoning_content.filter(|content| !content.is_empty()) {
        // ponytail: only fills empty reasoning; provider policy can graduate to model/channel strategy.
        *full_reasoning = reasoning;
    }
}

pub const MAX_MODEL_TURNS: u32 = 12;
pub const MAX_TOTAL_TOOL_CALLS: u32 = 128;
pub const MAX_IDENTICAL_TOOL_CALLS: u32 = 3;
pub const MAX_RUN_DURATION: Duration = Duration::from_secs(60 * 60);
const BUDGET_EXCEEDED_PREFIX: &str = "AGENT_BUDGET_EXCEEDED: ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStopReason {
    Completed,
    Cancelled,
    Failed,
    BudgetExceeded,
    // Set by DatabaseManager during startup recovery, not by a live in-process loop.
    #[allow(dead_code)]
    Interrupted,
}

#[derive(Debug, Clone)]
pub struct AgentBudget {
    pub model_turns: u32,
    pub total_tool_calls: u32,
    pub started_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct AgentRunContext {
    pub run_id: String,
    pub session_id: String,
    pub request_id: String,
    pub budget: AgentBudget,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn with_immediate_transaction<T>(
    conn: &Connection,
    operation: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|error| error.to_string())?;
    match operation(conn) {
        Ok(value) => conn
            .execute_batch("COMMIT")
            .map(|_| value)
            .map_err(|error| error.to_string()),
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn budget_error(message: impl Into<String>) -> String {
    format!("{}{}", BUDGET_EXCEEDED_PREFIX, message.into())
}

fn is_budget_error(error: &str) -> bool {
    error.starts_with(BUDGET_EXCEEDED_PREFIX)
}

async fn set_run_state(
    state: &Arc<AppState>,
    run_id: &str,
    status: &str,
    stop_reason: Option<&str>,
    active_request_id: Option<&str>,
) {
    let run_id = run_id.to_string();
    let status = status.to_string();
    let stop_reason = stop_reason.map(str::to_string);
    let active_request_id = active_request_id.map(str::to_string);
    let terminal_at = matches!(
        status.as_str(),
        "completed" | "cancelled" | "failed" | "budgetExceeded" | "interrupted"
    )
    .then(now_ms);
    if let Err(error) = state
        .db_manager
        .run_blocking(move |conn| {
            conn.execute(
                "UPDATE ai_runs SET status = ?1, stop_reason = ?2,
                     active_request_id = COALESCE(?3, active_request_id),
                     completed_at_ms = COALESCE(?4, completed_at_ms),
                     updated_at = CURRENT_TIMESTAMP WHERE id = ?5",
                params![status, stop_reason, active_request_id, terminal_at, run_id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
    {
        tracing::error!("[AI] Failed to persist run state: {}", error);
    }
}

async fn finish_persisted_run(
    state: &Arc<AppState>,
    context: &AgentRunContext,
    result: &Result<Option<Vec<ToolCall>>, String>,
) {
    let (status, reason, invocation_status) = match result {
        Ok(Some(_)) => return,
        Ok(None) => ("completed", AgentStopReason::Completed, None),
        Err(error) if error == AI_CANCELLED => {
            ("cancelled", AgentStopReason::Cancelled, Some("cancelled"))
        }
        Err(error) if is_budget_error(error) => (
            "budgetExceeded",
            AgentStopReason::BudgetExceeded,
            Some("interrupted"),
        ),
        Err(_) => ("failed", AgentStopReason::Failed, Some("failed")),
    };
    set_run_state(
        state,
        &context.run_id,
        status,
        Some(&format!("{:?}", reason)),
        None,
    )
    .await;
    if let Some(invocation_status) = invocation_status {
        let run_id = context.run_id.clone();
        let status = invocation_status.to_string();
        if let Err(error) = state
            .db_manager
            .run_blocking(move |conn| {
                conn.execute(
                    "UPDATE ai_tool_invocations SET status = ?1, updated_at = CURRENT_TIMESTAMP
                 WHERE run_id = ?2 AND status = 'executing'",
                    params![status, run_id],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            })
            .await
        {
            tracing::error!("[AI] Failed to finalize active tool invocations: {}", error);
        }
    }
}

pub(super) async fn fail_agent_run(
    state: &Arc<AppState>,
    context: &AgentRunContext,
    error: String,
) {
    finish_persisted_run(state, context, &Err(error)).await;
}

pub(super) async fn create_agent_run(
    state: &Arc<AppState>,
    session_id: &str,
    request_id: &str,
) -> Result<AgentRunContext, String> {
    let context = AgentRunContext {
        run_id: Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        request_id: request_id.to_string(),
        budget: AgentBudget {
            model_turns: 0,
            total_tool_calls: 0,
            started_at_ms: now_ms(),
        },
    };
    let insert = context.clone();
    state
        .db_manager
        .run_blocking(move |conn| {
            with_immediate_transaction(conn, |conn| {
                conn.execute(
                    "UPDATE ai_tool_invocations SET status = 'interrupted', updated_at = CURRENT_TIMESTAMP
                     WHERE status = 'awaitingApproval' AND run_id IN (
                         SELECT id FROM ai_runs WHERE session_id = ?1 AND status = 'awaitingApproval'
                     )",
                    params![insert.session_id],
                )
                .map_err(|error| error.to_string())?;
                conn.execute(
                    "UPDATE ai_runs SET status = 'interrupted', stop_reason = 'superseded_by_new_run',
                         completed_at_ms = ?1, updated_at = CURRENT_TIMESTAMP
                     WHERE session_id = ?2 AND status = 'awaitingApproval'",
                    params![insert.budget.started_at_ms, insert.session_id],
                )
                .map_err(|error| error.to_string())?;
                conn.execute(
                    "INSERT INTO ai_runs (id, session_id, request_id, active_request_id, status, model_turn_count, total_tool_call_count, started_at_ms)
                     VALUES (?1, ?2, ?3, ?4, 'running', 0, 0, ?5)",
                    params![
                        insert.run_id,
                        insert.session_id,
                        insert.request_id,
                        insert.request_id,
                        insert.budget.started_at_ms
                    ],
                )
                .map_err(|error| error.to_string())?;
                Ok(())
            })
        })
        .await?;
    Ok(context)
}

pub(super) async fn cancel_persisted_agent_run(
    state: &Arc<AppState>,
    session_id: &str,
    request_id: &str,
) -> Result<bool, String> {
    let session_id = session_id.to_string();
    let request_id = request_id.to_string();
    let completed_at_ms = now_ms();
    state
        .db_manager
        .run_blocking(move |conn| {
            with_immediate_transaction(conn, |conn| {
                let run_id: Option<String> = conn
                    .query_row(
                        "SELECT id FROM ai_runs
                         WHERE session_id = ?1 AND active_request_id = ?2
                           AND status IN ('running', 'awaitingApproval')
                         LIMIT 1",
                        params![session_id, request_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|error| error.to_string())?;
                let Some(run_id) = run_id else {
                    return Ok(false);
                };
                conn.execute(
                    "UPDATE ai_tool_invocations SET status = 'cancelled', updated_at = CURRENT_TIMESTAMP
                     WHERE run_id = ?1 AND status IN ('proposed', 'awaitingApproval', 'executing')",
                    params![run_id],
                )
                .map_err(|error| error.to_string())?;
                conn.execute(
                    "UPDATE ai_runs SET status = 'cancelled', stop_reason = 'Cancelled',
                         completed_at_ms = ?1, updated_at = CURRENT_TIMESTAMP
                     WHERE id = ?2 AND status IN ('running', 'awaitingApproval')",
                    params![completed_at_ms, run_id],
                )
                .map_err(|error| error.to_string())?;
                Ok(true)
            })
        })
        .await
}

async fn update_budget(state: &Arc<AppState>, context: &AgentRunContext) -> Result<(), String> {
    let run_id = context.run_id.clone();
    let model_turns = context.budget.model_turns;
    let total_tool_calls = context.budget.total_tool_calls;
    state
        .db_manager
        .run_blocking(move |conn| {
            conn.execute(
                "UPDATE ai_runs SET model_turn_count = ?1, total_tool_call_count = ?2,
                 updated_at = CURRENT_TIMESTAMP WHERE id = ?3",
                params![model_turns, total_tool_calls, run_id],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
        .await
}

async fn mark_invocations(
    state: &Arc<AppState>,
    run_id: &str,
    ids: &[String],
    status: &str,
) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let run_id = run_id.to_string();
    let ids = ids.to_vec();
    let status = status.to_string();
    state
        .db_manager
        .run_blocking(move |conn| {
            for id in ids {
                conn.execute(
                    "UPDATE ai_tool_invocations SET status = ?1, updated_at = CURRENT_TIMESTAMP
                 WHERE run_id = ?2 AND tool_call_id = ?3",
                    params![status, run_id, id],
                )
                .map_err(|e| e.to_string())?;
            }
            Ok(())
        })
        .await
}

#[derive(Default)]
struct PreparedToolBatch {
    execute: Vec<PreparedToolCall>,
    awaiting_approval: Vec<PreparedToolCall>,
    immediate: Vec<ToolOutcome>,
}

async fn load_session_grants(
    state: &Arc<AppState>,
    session_id: &str,
) -> Result<HashSet<String>, String> {
    let session_id = session_id.to_string();
    state
        .db_manager
        .run_blocking(move |conn| {
            let mut statement = conn
                .prepare("SELECT tool_name FROM ai_tool_approval_grants WHERE session_id = ?1")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![session_id], |row| row.get::<_, String>(0))
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<HashSet<_>, _>>()
                .map_err(|error| error.to_string())
        })
        .await
}

async fn prepare_tool_batch(
    state: &Arc<AppState>,
    session_id: &str,
    calls: Vec<ToolCall>,
    is_agent_mode: bool,
) -> Result<PreparedToolBatch, String> {
    let grants = load_session_grants(state, session_id).await?;
    let mut batch = PreparedToolBatch::default();
    for call in calls {
        let has_session_grant = grants.contains(&call.function.name);
        match ToolPolicyEngine::prepare(call, is_agent_mode, has_session_grant) {
            ToolPreparation::Execute(call) => batch.execute.push(call),
            ToolPreparation::AwaitApproval(call) => batch.awaiting_approval.push(call),
            ToolPreparation::Immediate(outcome) => batch.immediate.push(outcome),
        }
    }
    Ok(batch)
}

async fn persist_terminal_outcomes(
    app_handle: &tauri::AppHandle,
    state: &Arc<AppState>,
    context: &AgentRunContext,
    request_id: &str,
    turn_index: i64,
    outcomes: &[ToolOutcome],
) -> Result<(), String> {
    for outcome in outcomes {
        persist_tool_outcome(
            app_handle,
            state,
            &context.session_id,
            request_id,
            Some(&context.run_id),
            Some(turn_index),
            outcome,
        )
        .await?;
        mark_invocations(
            state,
            &context.run_id,
            std::slice::from_ref(&outcome.tool_call_id),
            outcome.status.as_db_status(),
        )
        .await?;
    }
    Ok(())
}

async fn persist_approval_requests(
    state: &Arc<AppState>,
    context: &AgentRunContext,
    calls: &[PreparedToolCall],
) -> Result<Vec<String>, String> {
    let run_id = context.run_id.clone();
    let requests: Vec<(String, String)> = calls
        .iter()
        .map(|call| (call.call.id.clone(), Uuid::new_v4().to_string()))
        .collect();
    let updates = requests.clone();
    state
        .db_manager
        .run_blocking(move |conn| {
            with_immediate_transaction(conn, |conn| {
                for (tool_call_id, approval_id) in updates {
                    let changed = conn
                        .execute(
                            "UPDATE ai_tool_invocations
                             SET status = 'awaitingApproval', approval_id = ?1, updated_at = CURRENT_TIMESTAMP
                             WHERE run_id = ?2 AND tool_call_id = ?3 AND status = 'proposed'",
                            params![approval_id, run_id, tool_call_id],
                        )
                        .map_err(|error| error.to_string())?;
                    if changed != 1 {
                        return Err("Tool invocation is no longer available for approval".to_string());
                    }
                }
                Ok(())
            })
        })
        .await?;
    Ok(requests
        .into_iter()
        .map(|(_, approval_id)| approval_id)
        .collect())
}

async fn execute_prepared_tool(
    app_handle: tauri::AppHandle,
    state: Arc<AppState>,
    session_id: String,
    ssh_session_id: Option<String>,
    prepared: PreparedToolCall,
    cancellation_token: CancellationToken,
) -> Result<ToolOutcome, String> {
    let call = prepared.call;
    match execute_tools(
        app_handle,
        &state,
        &session_id,
        ssh_session_id.as_deref(),
        vec![call.clone()],
        cancellation_token,
    )
    .await
    {
        Ok(mut outcomes) => outcomes
            .pop()
            .ok_or_else(|| "Tool executor returned no outcome".to_string()),
        Err(error) if error == AI_CANCELLED => Err(error),
        Err(error) => Ok(ToolOutcome {
            tool_call_id: call.id,
            status: ToolOutcomeStatus::Failed,
            content: error,
        }),
    }
}

async fn execute_prepared_batch(
    app_handle: tauri::AppHandle,
    state: Arc<AppState>,
    context: &AgentRunContext,
    ssh_session_id: Option<String>,
    calls: Vec<PreparedToolCall>,
    cancellation_token: CancellationToken,
) -> Result<Vec<ToolOutcome>, String> {
    if calls.is_empty() {
        return Ok(Vec::new());
    }
    let parallel = calls
        .iter()
        .all(|call| call.policy.execution == ToolExecution::Parallel);
    if !parallel {
        let mut outcomes = Vec::with_capacity(calls.len());
        for call in calls {
            outcomes.push(
                execute_prepared_tool(
                    app_handle.clone(),
                    state.clone(),
                    context.session_id.clone(),
                    ssh_session_id.clone(),
                    call,
                    cancellation_token.clone(),
                )
                .await?,
            );
        }
        return Ok(outcomes);
    }

    let mut indexed = futures::stream::iter(calls.into_iter().enumerate().map(|(index, call)| {
        let app_handle = app_handle.clone();
        let state = state.clone();
        let session_id = context.session_id.clone();
        let ssh_session_id = ssh_session_id.clone();
        let token = cancellation_token.clone();
        async move {
            let outcome =
                execute_prepared_tool(app_handle, state, session_id, ssh_session_id, call, token)
                    .await;
            (index, outcome)
        }
    }))
    .buffer_unordered(3)
    .collect::<Vec<_>>()
    .await;
    indexed.sort_by_key(|(index, _)| *index);
    indexed
        .into_iter()
        .map(|(_, outcome)| outcome)
        .collect::<Result<Vec<_>, _>>()
}

async fn persist_proposed_tools(
    state: &Arc<AppState>,
    context: &AgentRunContext,
    turn_index: i64,
    calls: &[ToolCall],
) -> Result<(), String> {
    let run_id = context.run_id.clone();
    let calls = calls.to_vec();
    state.db_manager.run_blocking(move |conn| {
        for call in calls {
            conn.execute(
                "INSERT INTO ai_tool_invocations (id, run_id, tool_call_id, tool_name, arguments_json, status, turn_index)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'proposed', ?6)",
                params![Uuid::new_v4().to_string(), run_id, call.id, call.function.name, call.function.arguments, turn_index],
            ).map_err(|e| e.to_string())?;
        }
        Ok(())
    }).await
}

async fn reserve_model_turn(
    state: &Arc<AppState>,
    context: &mut AgentRunContext,
) -> Result<(), String> {
    if now_ms().saturating_sub(context.budget.started_at_ms) as u64
        > MAX_RUN_DURATION.as_millis() as u64
    {
        return Err(budget_error("maximum wall-clock run duration reached"));
    }
    if context.budget.model_turns >= MAX_MODEL_TURNS {
        return Err(budget_error("maximum model turns reached"));
    }
    context.budget.model_turns += 1;
    update_budget(state, context).await
}

async fn reserve_tool_batch(
    state: &Arc<AppState>,
    context: &mut AgentRunContext,
    calls: &[ToolCall],
) -> Result<(), String> {
    if now_ms().saturating_sub(context.budget.started_at_ms) as u64
        > MAX_RUN_DURATION.as_millis() as u64
    {
        return Err(budget_error("maximum wall-clock run duration reached"));
    }
    if context
        .budget
        .total_tool_calls
        .saturating_add(calls.len() as u32)
        > MAX_TOTAL_TOOL_CALLS
    {
        return Err(budget_error("maximum total tool calls reached"));
    }
    let call_count = calls.len() as u32;
    let run_id = context.run_id.clone();
    let calls = calls.to_vec();
    let repeated = state.db_manager.run_blocking(move |conn| {
        let mut seen: HashMap<(String, String), u32> = HashMap::new();
        for call in calls {
            let key = (call.function.name, call.function.arguments);
            let next = seen.entry(key).or_default();
            *next += 1;
        }
        for ((name, arguments), pending_count) in seen {
            let persisted: u32 = conn.query_row(
                "SELECT COUNT(*) FROM ai_tool_invocations WHERE run_id = ?1 AND tool_name = ?2 AND arguments_json = ?3",
                params![run_id, name, arguments], |row| row.get(0),
            ).map_err(|e| e.to_string())?;
            if persisted.saturating_add(pending_count) > MAX_IDENTICAL_TOOL_CALLS {
                return Ok(true);
            }
        }
        Ok(false)
    }).await?;
    if repeated {
        return Err(budget_error("maximum identical tool calls reached"));
    }
    context.budget.total_tool_calls += call_count;
    update_budget(state, context).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolApprovalAction {
    Accept,
    AcceptForSession,
    Decline,
    Cancel,
}

impl ToolApprovalAction {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "accept" => Ok(Self::Accept),
            "acceptForSession" => Ok(Self::AcceptForSession),
            "decline" => Ok(Self::Decline),
            "cancel" => Ok(Self::Cancel),
            _ => Err(
                "approval_action must be accept, acceptForSession, decline, or cancel".to_string(),
            ),
        }
    }

    fn terminal_status(self) -> Option<ToolOutcomeStatus> {
        match self {
            Self::Accept | Self::AcceptForSession => None,
            Self::Decline => Some(ToolOutcomeStatus::Declined),
            Self::Cancel => Some(ToolOutcomeStatus::Cancelled),
        }
    }
}

pub(super) enum AgentRunResume {
    Resumed {
        context: AgentRunContext,
        tools: Vec<ToolCall>,
        turn_index: i64,
    },
    AlreadyResolved,
    InProgress,
}

pub(super) async fn resume_agent_run(
    state: &Arc<AppState>,
    session_id: &str,
    request_id: &str,
    is_agent_mode: bool,
    run_id: &str,
    turn_index: i64,
    tool_call_ids: &[String],
    approval_ids: &[String],
    approval_action: ToolApprovalAction,
) -> Result<AgentRunResume, String> {
    if tool_call_ids.is_empty() || tool_call_ids.len() != approval_ids.len() {
        return Err(
            "Every approval response must include matching tool_call_ids and approval_ids"
                .to_string(),
        );
    }
    let response_ids: HashMap<String, String> = tool_call_ids
        .iter()
        .cloned()
        .zip(approval_ids.iter().cloned())
        .collect();
    if response_ids.len() != tool_call_ids.len() {
        return Err("Tool call ids must not contain duplicates".to_string());
    }

    let session_id = session_id.to_string();
    let request_id = request_id.to_string();
    let run_id = run_id.to_string();
    state
        .db_manager
        .run_blocking(move |conn| {
            with_immediate_transaction(conn, |conn| {
                let run: Option<(String, i64, i64, i64)> = conn
                    .query_row(
                        "SELECT request_id, model_turn_count, total_tool_call_count, started_at_ms
                         FROM ai_runs WHERE id = ?1 AND session_id = ?2 AND status = 'awaitingApproval'",
                        params![run_id, session_id],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .optional()
                    .map_err(|error| error.to_string())?;

                let Some((original_request_id, model_turns, total_tool_calls, started_at_ms)) = run else {
                    // Do not replace the in-memory run token for a duplicate approval.
                    // A terminal result is idempotently resolved; an executing result is
                    // already owned by the original approval request.
                    let mut matched_count = 0usize;
                    let mut terminal_count = 0usize;
                    let mut has_executing = false;
                    for (tool_call_id, approval_id) in &response_ids {
                        let status: Option<String> = conn
                            .query_row(
                                "SELECT status FROM ai_tool_invocations
                                 WHERE run_id = ?1 AND turn_index = ?2 AND tool_call_id = ?3 AND approval_id = ?4",
                                params![run_id, turn_index, tool_call_id, approval_id],
                                |row| row.get(0),
                            )
                            .optional()
                            .map_err(|error| error.to_string())?;
                        if let Some(status) = status {
                            matched_count += 1;
                            if matches!(
                                status.as_str(),
                                "completed" | "failed" | "declined" | "cancelled" | "interrupted"
                            ) {
                                terminal_count += 1;
                            }
                            has_executing |= status == "executing";
                        }
                    }
                    if terminal_count == response_ids.len() {
                        return Ok(AgentRunResume::AlreadyResolved);
                    }
                    if matched_count == response_ids.len() && has_executing {
                        return Ok(AgentRunResume::InProgress);
                    }
                    return Err("No awaiting approval run matches the supplied run identity".to_string());
                };

                let mut statement = conn
                    .prepare(
                        "SELECT tool_call_id, tool_name, arguments_json, approval_id
                         FROM ai_tool_invocations
                         WHERE run_id = ?1 AND turn_index = ?2 AND status = 'awaitingApproval'
                         ORDER BY rowid ASC",
                    )
                    .map_err(|error| error.to_string())?;
                let rows = statement
                    .query_map(params![run_id, turn_index], |row| {
                        Ok((
                            ToolCall {
                                id: row.get(0)?,
                                tool_type: "function".to_string(),
                                function: FunctionCall {
                                    name: row.get(1)?,
                                    arguments: row.get(2)?,
                                },
                                thought_signatures: None,
                            },
                            row.get::<_, Option<String>>(3)?,
                        ))
                    })
                    .map_err(|error| error.to_string())?;
                let pending = rows
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                if pending.len() != response_ids.len()
                    || pending.iter().any(|(call, approval_id)| {
                        response_ids.get(&call.id) != approval_id.as_ref()
                    })
                {
                    return Err("Approval response must match every pending tool in the run turn".to_string());
                }

                let calls: Vec<ToolCall> = pending.into_iter().map(|(call, _)| call).collect();
                if matches!(approval_action, ToolApprovalAction::Accept | ToolApprovalAction::AcceptForSession)
                    && !is_agent_mode
                    && calls.iter().any(|call| {
                        tool_policy(&call.function.name)
                            .map(|policy| !policy.allowed_modes.contains(&super::types::ToolMode::Ask))
                            .unwrap_or(true)
                    })
                {
                    return Err("Execution denied: the approval response is not allowed in Ask mode.".to_string());
                }

                if approval_action == ToolApprovalAction::AcceptForSession {
                    for call in &calls {
                        if tool_policy(&call.function.name)
                            .is_some_and(|policy| is_session_grant_eligible(&policy))
                        {
                            conn.execute(
                                "INSERT OR IGNORE INTO ai_tool_approval_grants (session_id, tool_name) VALUES (?1, ?2)",
                                params![session_id, call.function.name],
                            )
                            .map_err(|error| error.to_string())?;
                        }
                    }
                }

                if let Some(status) = approval_action.terminal_status() {
                    let observation = serde_json::json!({
                        "status": status.as_db_status(),
                        "error": if status == ToolOutcomeStatus::Declined {
                            "The user declined this tool call."
                        } else {
                            "The user cancelled this tool call before execution."
                        },
                    })
                    .to_string();
                    for call in &calls {
                        conn.execute(
                            "UPDATE ai_tool_invocations SET status = ?1, updated_at = CURRENT_TIMESTAMP
                             WHERE run_id = ?2 AND tool_call_id = ?3 AND status = 'awaitingApproval'",
                            params![status.as_db_status(), run_id, call.id],
                        )
                        .map_err(|error| error.to_string())?;
                        conn.execute(
                            "INSERT INTO ai_messages (id, session_id, role, content, tool_call_id, run_id, turn_index)
                             VALUES (?1, ?2, 'tool', ?3, ?4, ?5, ?6)",
                            params![
                                Uuid::new_v4().to_string(),
                                session_id,
                                observation,
                                call.id,
                                run_id,
                                turn_index
                            ],
                        )
                        .map_err(|error| error.to_string())?;
                    }
                } else {
                    for call in &calls {
                        conn.execute(
                            "UPDATE ai_tool_invocations SET status = 'executing', updated_at = CURRENT_TIMESTAMP
                             WHERE run_id = ?1 AND tool_call_id = ?2 AND status = 'awaitingApproval'",
                            params![run_id, call.id],
                        )
                        .map_err(|error| error.to_string())?;
                    }
                }

                conn.execute(
                    "UPDATE ai_runs SET status = 'running', active_request_id = ?1, updated_at = CURRENT_TIMESTAMP
                     WHERE id = ?2 AND status = 'awaitingApproval'",
                    params![request_id, run_id],
                )
                .map_err(|error| error.to_string())?;

                Ok(AgentRunResume::Resumed {
                    context: AgentRunContext {
                        run_id,
                        session_id,
                        request_id: original_request_id,
                        budget: AgentBudget {
                            model_turns: model_turns as u32,
                            total_tool_calls: total_tool_calls as u32,
                            started_at_ms,
                        },
                    },
                    tools: if approval_action.terminal_status().is_none() {
                        calls
                    } else {
                        Vec::new()
                    },
                    turn_index,
                })
            })
        })
        .await
}

pub(super) async fn run_agent_loop(
    window: Window,
    state: Arc<AppState>,
    mut context: AgentRunContext,
    model_id: String,
    channel_id: String,
    is_agent_mode: bool,
    tools: Option<Vec<ToolDefinition>>,
    cancellation_token: CancellationToken,
    ssh_session_id: Option<String>,
    thinking_level: Option<String>,
    request_id: String,
    resumed_tools: Option<(Vec<ToolCall>, i64)>,
) -> Result<Option<Vec<ToolCall>>, String> {
    let result = async {
        let mut pending_execution = resumed_tools;
        loop {
            if cancellation_token.is_cancelled() {
                return Err(AI_CANCELLED.to_string());
            }
            if now_ms().saturating_sub(context.budget.started_at_ms) as u64
                > MAX_RUN_DURATION.as_millis() as u64
            {
                return Err(budget_error("maximum wall-clock run duration reached"));
            }
            if let Some((calls, turn_index)) = pending_execution.take() {
                let mut prepared = Vec::with_capacity(calls.len());
                let mut immediate = Vec::new();
                for call in calls {
                    match tool_policy(&call.function.name) {
                        Some(policy) => prepared.push(PreparedToolCall { call, policy }),
                        None => immediate.push(ToolOutcome {
                            tool_call_id: call.id,
                            status: ToolOutcomeStatus::Failed,
                            content: "Tool is no longer registered and was not executed."
                                .to_string(),
                        }),
                    }
                }
                persist_terminal_outcomes(
                    &window.app_handle().clone(),
                    &state,
                    &context,
                    &request_id,
                    turn_index,
                    &immediate,
                )
                .await?;
                let outcomes = execute_prepared_batch(
                    window.app_handle().clone(),
                    state.clone(),
                    &context,
                    ssh_session_id.clone(),
                    prepared,
                    cancellation_token.clone(),
                )
                .await?;
                persist_terminal_outcomes(
                    &window.app_handle().clone(),
                    &state,
                    &context,
                    &request_id,
                    turn_index,
                    &outcomes,
                )
                .await?;
                continue;
            }

            reserve_model_turn(&state, &mut context).await?;
            let turn_index = context.budget.model_turns as i64;
            let calls = stream_model_turn(
                window.clone(),
                state.clone(),
                context.session_id.clone(),
                model_id.clone(),
                channel_id.clone(),
                is_agent_mode,
                tools.clone(),
                cancellation_token.clone(),
                ssh_session_id.clone(),
                thinking_level.clone(),
                request_id.clone(),
                context.run_id.clone(),
                turn_index,
                context.budget.total_tool_calls,
            )
            .await?;
            let Some(calls) = calls.filter(|calls| !calls.is_empty()) else {
                return Ok(None);
            };
            reserve_tool_batch(&state, &mut context, &calls).await?;
            persist_proposed_tools(&state, &context, turn_index, &calls).await?;

            let batch =
                prepare_tool_batch(&state, &context.session_id, calls, is_agent_mode).await?;
            persist_terminal_outcomes(
                &window.app_handle().clone(),
                &state,
                &context,
                &request_id,
                turn_index,
                &batch.immediate,
            )
            .await?;

            if !batch.execute.is_empty() {
                let ids: Vec<String> = batch
                    .execute
                    .iter()
                    .map(|call| call.call.id.clone())
                    .collect();
                mark_invocations(&state, &context.run_id, &ids, "executing").await?;
                let outcomes = execute_prepared_batch(
                    window.app_handle().clone(),
                    state.clone(),
                    &context,
                    ssh_session_id.clone(),
                    batch.execute,
                    cancellation_token.clone(),
                )
                .await?;
                persist_terminal_outcomes(
                    &window.app_handle().clone(),
                    &state,
                    &context,
                    &request_id,
                    turn_index,
                    &outcomes,
                )
                .await?;
            }

            if !batch.awaiting_approval.is_empty() {
                let approval_ids =
                    persist_approval_requests(&state, &context, &batch.awaiting_approval).await?;
                set_run_state(
                    &state,
                    &context.run_id,
                    "awaitingApproval",
                    None,
                    Some(&request_id),
                )
                .await;
                let approval_calls: Vec<ToolCall> = batch
                    .awaiting_approval
                    .iter()
                    .map(|call| call.call.clone())
                    .collect();
                let approval_policies = batch
                    .awaiting_approval
                    .iter()
                    .map(|call| call.policy.approval)
                    .collect();
                window
                    .emit(
                        &format!("ai-tool-call-{}", context.session_id),
                        AiToolCallEventPayload {
                            request_id: request_id.clone(),
                            run_id: context.run_id.clone(),
                            turn_index,
                            tool_calls: approval_calls.clone(),
                            approval_ids,
                            approval_policies,
                        },
                    )
                    .map_err(|e| e.to_string())?;
                return Ok(Some(approval_calls));
            }
        }
    }
    .await;
    let result = if cancellation_token.is_cancelled() {
        Err(AI_CANCELLED.to_string())
    } else {
        result
    };
    finish_persisted_run(&state, &context, &result).await;
    result
}

pub fn stream_model_turn(
    window: Window,
    state: Arc<AppState>,
    session_id: String,
    model_id: String,
    channel_id: String,
    is_agent_mode: bool,
    tools: Option<Vec<ToolDefinition>>,
    cancellation_token: CancellationToken,
    ssh_session_id: Option<String>,
    thinking_level: Option<String>,
    request_id: String,
    run_id: String,
    turn_index: i64,
    cumulative_tool_calls: u32,
) -> futures::future::BoxFuture<'static, Result<Option<Vec<ToolCall>>, String>> {
    Box::pin(async move {
        if cancellation_token.is_cancelled() {
            return Err(AI_CANCELLED.to_string());
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

        if cancellation_token.is_cancelled() {
            return Err(AI_CANCELLED.to_string());
        }

        let history: Vec<ChatMessage> = tokio::select! {
            _ = cancellation_token.cancelled() => {
                return Err(AI_CANCELLED.to_string());
            }
            history = load_history(
                &state,
                &session_id,
                is_agent_mode,
                ssh_session_id.as_deref(),
                &channel,
                &model,
            ) => history?,
        };
        let provider_capabilities = ProviderCapabilities::for_channel_and_model(&channel, &model);
        let genai_history = to_genai_messages(history, provider_capabilities);

        let genai_tools: Option<Vec<Tool>> = tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| {
                    Tool::new(t.function.name.clone())
                        .with_description(t.function.description.clone())
                        .with_schema(t.function.parameters.clone())
                })
                .collect()
        });

        if cancellation_token.is_cancelled() {
            return Err(AI_CANCELLED.to_string());
        }

        // Interruptible stream open: cancel must not wait for provider connect.
        let mut stream = tokio::select! {
            _ = cancellation_token.cancelled() => {
                return Err(AI_CANCELLED.to_string());
            }
            stream_result = state.ai_manager.stream_chat(
                &channel,
                &model,
                genai_history,
                genai_tools,
                proxy,
                thinking_level.as_deref(),
            ) => stream_result?,
        };

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

        loop {
            // Cancellation-first: when both ready, prefer cancel over chunk delivery.
            let next_event = tokio::select! {
                biased;
                _ = cancellation_token.cancelled() => {
                    // Do not flush undelivered response/reasoning/tool-call tails on cancel.
                    if has_pending_reasoning {
                        window
                            .emit(
                            &reasoning_end_event,
                            AiReasoningEndPayload {
                                request_id: request_id.clone(),
                                status: "end".to_string(),
                            },
                        )
                            .map_err(|e| e.to_string())?;
                    }
                    return Err(AI_CANCELLED.to_string());
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
                                &request_id,
                                &mut think_parser_buffer,
                                in_think_block,
                                &mut full_content,
                                &mut full_reasoning,
                                &mut response_emit_buffer,
                                &mut reasoning_emit_buffer,
                                &mut has_pending_reasoning,
                                &mut last_emit_at,
                            )?;
                            flush_response_buffer(&window, &response_event, &request_id, &mut response_emit_buffer)?;
                            flush_reasoning_buffer(&window, &reasoning_event, &request_id, &mut reasoning_emit_buffer)?;
                            if has_pending_reasoning {
                                window
                                    .emit(
                            &reasoning_end_event,
                            AiReasoningEndPayload {
                                request_id: request_id.clone(),
                                status: "end".to_string(),
                            },
                        )
                                    .map_err(|e| e.to_string())?;
                            }

                            let err_msg = format!(
                                "AI stream stalled for {} seconds without new events.",
                                AI_STREAM_IDLE_TIMEOUT_SECS
                            );
                            tracing::warn!(
                                "[AI] {} session_id={}, request_id={}, pending_tool_fragments={}",
                                err_msg,
                                session_id,
                                request_id,
                                accumulated_tool_calls.len()
                            );
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
                                    &request_id,
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
                                    &request_id,
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
                            &request_id,
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
                            &request_id,
                            &mut think_parser_buffer,
                            in_think_block,
                            &mut full_content,
                            &mut full_reasoning,
                            &mut response_emit_buffer,
                            &mut reasoning_emit_buffer,
                            &mut has_pending_reasoning,
                            &mut last_emit_at,
                        )?;
                        flush_response_buffer(
                            &window,
                            &response_event,
                            &request_id,
                            &mut response_emit_buffer,
                        )?;
                        flush_reasoning_buffer(
                            &window,
                            &reasoning_event,
                            &request_id,
                            &mut reasoning_emit_buffer,
                        )?;
                        last_emit_at = Instant::now();

                        tracing::debug!(
                            "[AI] ToolCallChunk received: call_id={}, name_present={}, arguments_len={}",
                            chunk.tool_call.call_id,
                            !chunk.tool_call.fn_name.is_empty(),
                            chunk.tool_call.fn_arguments.to_string().len()
                        );

                        let args = chunk
                            .tool_call
                            .fn_arguments
                            .as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| chunk.tool_call.fn_arguments.to_string());

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
                        let captured_usage = end.captured_usage.clone();
                        let provider_stop_reason = end
                            .captured_stop_reason
                            .as_ref()
                            .map(|reason| reason.raw().to_string());
                        flush_think_parser_remainder(
                            &window,
                            &response_event,
                            &reasoning_event,
                            &reasoning_end_event,
                            &request_id,
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
                                .emit(
                                    &format!("ai-reasoning-end-{}", session_id),
                                    AiReasoningEndPayload {
                                        request_id: request_id.clone(),
                                        status: "end".to_string(),
                                    },
                                )
                                .map_err(|e| e.to_string())?;
                            has_pending_reasoning = false;
                        }

                        apply_reasoning_fallback(
                            &mut full_reasoning,
                            end.captured_reasoning_content,
                        );

                        let captured_tool_calls = if let Some(content) = end.captured_content {
                            let raw_tool_calls = content.tool_calls();
                            tracing::debug!(
                                "[AI] Stream end captured tool calls: count={}",
                                raw_tool_calls.len()
                            );
                            raw_tool_calls
                                .into_iter()
                                .filter(|tool_call| !tool_call.fn_name.is_empty())
                                .map(|tool_call| {
                                    let arguments = tool_call
                                        .fn_arguments
                                        .as_str()
                                        .map(str::to_string)
                                        .unwrap_or_else(|| tool_call.fn_arguments.to_string());
                                    ToolCall {
                                        id: tool_call.call_id.clone(),
                                        tool_type: "function".to_string(),
                                        function: FunctionCall {
                                            name: tool_call.fn_name.clone(),
                                            arguments,
                                        },
                                        thought_signatures: tool_call.thought_signatures.clone(),
                                    }
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                        let tool_calls =
                            normalize_streamed_tool_calls(merge_captured_and_streamed_tool_calls(
                                accumulated_tool_calls.clone(),
                                captured_tool_calls,
                                &tool_call_id_aliases,
                            ));

                        for call in &tool_calls {
                            let timeout = extract_timeout(&call.function.arguments);
                            let timeout_display = timeout
                                .map(|value| format!("{}s", value))
                                .unwrap_or_else(|| "missing".to_string());
                            tracing::info!(
                                "[AI] Tool call received: id={}, name={}, timeout={}, arguments_len={}, has_thought_signatures={}",
                                call.id,
                                call.function.name,
                                timeout_display,
                                call.function.arguments.len(),
                                call.thought_signatures.is_some()
                            );
                        }

                        tracing::info!(
                            target: "ai_turn",
                            run_id = %run_id,
                            request_id = %request_id,
                            turn_index,
                            provider_stop_reason = ?provider_stop_reason,
                            prompt_tokens = ?captured_usage.as_ref().and_then(|usage| usage.prompt_tokens),
                            completion_tokens = ?captured_usage.as_ref().and_then(|usage| usage.completion_tokens),
                            total_tokens = ?captured_usage.as_ref().and_then(|usage| usage.total_tokens),
                            cumulative_model_turns = turn_index,
                            cumulative_tool_calls = cumulative_tool_calls,
                            emitted_tool_calls = tool_calls.len(),
                            "AI model turn completed"
                        );

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
                        &request_id,
                        &mut think_parser_buffer,
                        in_think_block,
                        &mut full_content,
                        &mut full_reasoning,
                        &mut response_emit_buffer,
                        &mut reasoning_emit_buffer,
                        &mut has_pending_reasoning,
                        &mut last_emit_at,
                    )?;
                    flush_response_buffer(
                        &window,
                        &response_event,
                        &request_id,
                        &mut response_emit_buffer,
                    )?;
                    flush_reasoning_buffer(
                        &window,
                        &reasoning_event,
                        &request_id,
                        &mut reasoning_emit_buffer,
                    )?;

                    if has_pending_reasoning {
                        window
                            .emit(
                                &reasoning_end_event,
                                AiReasoningEndPayload {
                                    request_id: request_id.clone(),
                                    status: "end".to_string(),
                                },
                            )
                            .ok();
                    }

                    let err_msg = e.to_string();
                    return Err(err_msg);
                }
            }
        }

        flush_think_parser_remainder(
            &window,
            &response_event,
            &reasoning_event,
            &reasoning_end_event,
            &request_id,
            &mut think_parser_buffer,
            in_think_block,
            &mut full_content,
            &mut full_reasoning,
            &mut response_emit_buffer,
            &mut reasoning_emit_buffer,
            &mut has_pending_reasoning,
            &mut last_emit_at,
        )?;
        flush_response_buffer(
            &window,
            &response_event,
            &request_id,
            &mut response_emit_buffer,
        )?;
        flush_reasoning_buffer(
            &window,
            &reasoning_event,
            &request_id,
            &mut reasoning_emit_buffer,
        )?;

        if has_pending_reasoning {
            window
                .emit(
                    &reasoning_end_event,
                    AiReasoningEndPayload {
                        request_id: request_id.clone(),
                        status: "end".to_string(),
                    },
                )
                .ok();
        }

        let ai_msg_id = Uuid::new_v4().to_string();
        let tool_calls_json = final_tool_calls
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| format!("序列化失败: {}", e))?;
        if !full_content.is_empty() || final_tool_calls.is_some() || !full_reasoning.is_empty() {
            let ai_msg_id_owned = ai_msg_id.clone();
            let session_id_owned = session_id.clone();
            let full_content_owned = full_content.clone();
            let full_reasoning_owned = full_reasoning.clone();
            let model_id_owned = model.id.clone();
            let run_id_owned = run_id.clone();
            state.db_manager.run_blocking(move |conn| {
                conn.execute(
                    "INSERT INTO ai_messages (id, session_id, role, content, reasoning_content, tool_calls, model_id, run_id, turn_index)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![ai_msg_id_owned, session_id_owned, "assistant", full_content_owned, full_reasoning_owned, tool_calls_json, model_id_owned, run_id_owned, turn_index],
                ).map_err(|e| e.to_string())?;
                Ok(())
            }).await?;
        }

        Ok(final_tool_calls)
    })
}

#[cfg(test)]
mod await_or_cancel_tests {
    use super::{
        await_or_cancel, budget_error, is_budget_error, AI_CANCELLED, MAX_IDENTICAL_TOOL_CALLS,
        MAX_MODEL_TURNS, MAX_RUN_DURATION, MAX_TOTAL_TOOL_CALLS,
    };
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn cancels_blocked_future_without_waiting_for_it() {
        let token = CancellationToken::new();
        let token_cancel = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            token_cancel.cancel();
        });

        let started = std::time::Instant::now();
        let result = await_or_cancel(&token, async {
            // Simulates a slow SFTP / history await that does not observe the token.
            tokio::time::sleep(Duration::from_secs(30)).await;
            "done"
        })
        .await;
        assert_eq!(result, Err(AI_CANCELLED.to_string()));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "cancel must not wait for the blocked future; elapsed {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn returns_ok_when_future_finishes_first() {
        let token = CancellationToken::new();
        let result = await_or_cancel(&token, async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            42
        })
        .await;
        assert_eq!(result, Ok(42));
        assert!(!token.is_cancelled());
    }
    #[test]
    fn agent_budget_constants_match_phase_one_contract() {
        assert_eq!(MAX_MODEL_TURNS, 12);
        assert_eq!(MAX_TOTAL_TOOL_CALLS, 128);
        assert_eq!(MAX_IDENTICAL_TOOL_CALLS, 3);
        assert_eq!(MAX_RUN_DURATION, Duration::from_secs(60 * 60));
    }

    #[test]
    fn budget_errors_are_distinct_from_provider_failures() {
        assert!(is_budget_error(&budget_error(
            "maximum model turns reached"
        )));
        assert!(!is_budget_error("provider disconnected"));
    }
}
