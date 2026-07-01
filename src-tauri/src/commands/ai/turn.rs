use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use genai::chat::{ChatStreamEvent, Tool};
use rusqlite::params;
use tauri::{Emitter, Manager, Window};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::history::{
    load_history, read_remote_file_via_sftp, to_genai_messages, READ_FILE_MAX_BYTES,
};
use super::stream_parsing::{
    accumulate_streamed_tool_call_chunk, append_reasoning_stream_text, append_response_stream_text,
    extract_command, extract_required_timeout_seconds, extract_think_segments, extract_timeout,
    extract_wait_finish, flush_reasoning_buffer, flush_response_buffer,
    flush_think_parser_remainder, normalize_streamed_tool_calls,
};
use super::tool_runtime::{
    build_recording_input_payload, build_run_in_terminal_timeout_failure_message,
    execute_command_in_exec_channel, try_reconnect_terminal_after_timeout,
    try_recover_terminal_after_timeout, START_MARKER_EXPECT_MS, TIMEOUT_RECOVERY_GRACE_MS,
};
use super::types::{ChatMessage, FunctionCall, ToolCall, ToolDefinition};
use super::{is_read_only_tool, AI_STREAM_IDLE_TIMEOUT_SECS};
use crate::commands::AppState;
use crate::ssh_manager::ssh::SSHClient;

pub(super) async fn execute_tools_and_save(
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
                                        let command_block_event =
                                            format!("terminal-command-block:{}", ssh_id);
                                        let _ = app_handle.emit(&command_block_event, "start");
                                        match SSHClient::send_input(ssh_id, cmd_nl.as_bytes()).await {
                                            Ok(_) => {
                                                "Command sent to terminal without waiting for completion (wait_finish=false).".to_string()
                                            }
                                            Err(e) => {
                                                let _ = app_handle.emit(&command_block_event, "end");
                                                format!("Failed to send command: {}", e)
                                            }
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

                                        let command_block_event =
                                            format!("terminal-command-block:{}", ssh_id);
                                        let _ = app_handle.emit(&command_block_event, "start");

                                        if let Err(e) =
                                            SSHClient::send_input(ssh_id, input_payload.as_bytes())
                                                .await
                                        {
                                            let _ = app_handle.emit(&command_block_event, "end");
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
                                                let _ =
                                                    app_handle.emit(&command_block_event, "end");
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
                                                match SSHClient::check_command_completed(ssh_id)
                                                    .await
                                                {
                                                    Ok(is_completed) => is_completed,
                                                    Err(e) => {
                                                        let _ = app_handle
                                                            .emit(&command_block_event, "end");
                                                        return Err(e);
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

                        apply_reasoning_fallback(
                            &mut full_reasoning,
                            end.captured_reasoning_content,
                        );

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
                    .filter(|c| is_read_only_tool(&c.function.name))
                    .cloned()
                    .collect();

                let confirm_calls: Vec<ToolCall> = calls
                    .iter()
                    .filter(|c| !is_read_only_tool(&c.function.name))
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
