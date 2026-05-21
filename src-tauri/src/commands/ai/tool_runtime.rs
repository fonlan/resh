use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::ssh_manager::ssh::SSHClient;

#[derive(Debug)]
pub(super) struct ExecChannelCommandResult {
    pub output: String,
    pub exit_status: Option<u32>,
    pub timed_out: bool,
}

pub(super) const TIMEOUT_RECOVERY_POLL_MS: u64 = 100;
pub(super) const TIMEOUT_RECOVERY_GRACE_MS: u64 = 2000;
pub(super) const START_MARKER_EXPECT_MS: u64 = 1500;

pub(super) fn build_run_in_terminal_timeout_failure_message(
    timeout_seconds: u64,
    partial_output: &str,
) -> String {
    let base = format!(
        "Error: run_in_terminal timed out after {}s without detecting command completion. Treat this as a failed foreground execution. Use run_in_background to check whether the process is still running and terminate it if needed.",
        timeout_seconds
    );

    if partial_output.trim().is_empty() {
        base
    } else {
        format!("{}\n\n[Partial output]\n{}", base, partial_output.trim())
    }
}

pub(super) async fn try_recover_terminal_after_timeout(
    ssh_session_id: &str,
    command: &str,
    log_scope: &str,
) -> bool {
    if let Ok(diag) = SSHClient::get_command_recording_diagnostics(ssh_session_id, 160).await {
        tracing::warn!(
            "[{}] Timeout diagnostics for command '{}': {}",
            log_scope,
            command,
            diag
        );
    }

    if let Err(e) = SSHClient::send_interrupt(ssh_session_id).await {
        tracing::warn!(
            "[{}] Timeout recovery interrupt failed for command '{}': {}",
            log_scope,
            command,
            e
        );
        return false;
    }
    tracing::warn!(
        "[{}] Timeout recovery interrupt sent for command '{}'",
        log_scope,
        command
    );

    let mut interval =
        tokio::time::interval(tokio::time::Duration::from_millis(TIMEOUT_RECOVERY_POLL_MS));
    let mut elapsed = 0u64;
    while elapsed < TIMEOUT_RECOVERY_GRACE_MS {
        interval.tick().await;
        elapsed += TIMEOUT_RECOVERY_POLL_MS;
        match SSHClient::check_command_completed(ssh_session_id).await {
            Ok(true) => {
                tracing::warn!(
                    "[{}] Terminal recovered after timeout for command '{}' ({}ms after interrupt)",
                    log_scope,
                    command,
                    elapsed
                );
                return true;
            }
            Ok(false) => {}
            Err(e) => {
                tracing::debug!(
                    "[{}] Timeout recovery check failed for command '{}': {}",
                    log_scope,
                    command,
                    e
                );
                break;
            }
        }
    }

    false
}

pub(super) async fn try_reconnect_terminal_after_timeout(
    ssh_session_id: &str,
    command: &str,
    log_scope: &str,
) -> bool {
    match SSHClient::reconnect(ssh_session_id).await {
        Ok(_) => {
            tracing::warn!(
                "[{}] Timeout fallback reconnect succeeded for command '{}'",
                log_scope,
                command
            );
            true
        }
        Err(e) => {
            tracing::warn!(
                "[{}] Timeout fallback reconnect failed for command '{}': {}",
                log_scope,
                command,
                e
            );
            false
        }
    }
}

pub(super) async fn build_recording_input_payload(
    ssh_session_id: &str,
    command: &str,
    log_scope: &str,
) -> String {
    let cmd_nl = format!("{}\n", command);
    let start_marker = SSHClient::get_recording_start_marker_command(ssh_session_id).await;
    let done_marker = SSHClient::get_recording_marker_command(ssh_session_id).await;

    match (start_marker, done_marker) {
        (Ok(start), Ok(done)) => format!("{}{}{}", start, cmd_nl, done),
        (Err(start_err), Ok(done)) => {
            tracing::debug!(
                "[{}] Start marker unavailable for '{}': {}",
                log_scope,
                command,
                start_err
            );
            format!("{}{}", cmd_nl, done)
        }
        (Ok(start), Err(done_err)) => {
            tracing::debug!(
                "[{}] Completion marker unavailable for '{}': {}",
                log_scope,
                command,
                done_err
            );
            format!("{}{}", start, cmd_nl)
        }
        (Err(start_err), Err(done_err)) => {
            tracing::debug!(
                "[{}] Start+completion markers unavailable for '{}': start={}, done={}",
                log_scope,
                command,
                start_err,
                done_err
            );
            cmd_nl
        }
    }
}

pub(super) async fn execute_command_in_exec_channel(
    session_id: &str,
    command: &str,
    timeout_seconds: u64,
    cancellation_token: Option<&CancellationToken>,
) -> Result<ExecChannelCommandResult, String> {
    let timeout = timeout_seconds.max(1);
    let timeout_ms = timeout * 1000;

    let ssh_session = SSHClient::get_session_handle(session_id)
        .await
        .ok_or_else(|| "Session not found".to_string())?;

    let mut channel = ssh_session
        .channel_open_session()
        .await
        .map_err(|e| format!("Failed to open background exec channel: {}", e))?;

    channel
        .exec(true, command)
        .await
        .map_err(|e| format!("Failed to execute background command: {}", e))?;

    let mut output = String::new();
    let mut exit_status: Option<u32> = None;
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    let mut elapsed = 0u64;
    let mut timed_out = false;

    loop {
        if cancellation_token
            .map(|token| token.is_cancelled())
            .unwrap_or(false)
        {
            return Err("CANCELLED".to_string());
        }

        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { ref data }) => {
                        output.push_str(&String::from_utf8_lossy(data));
                    }
                    Some(russh::ChannelMsg::ExtendedData { ref data, .. }) => {
                        output.push_str(&String::from_utf8_lossy(data));
                    }
                    Some(russh::ChannelMsg::ExitStatus { exit_status: status }) => {
                        exit_status = Some(status);
                    }
                    Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => {
                        break;
                    }
                    Some(_) => {}
                }
            }
            _ = interval.tick() => {
                elapsed += 100;
                if elapsed >= timeout_ms {
                    timed_out = true;
                    break;
                }
            }
        }
    }

    Ok(ExecChannelCommandResult {
        output,
        exit_status,
        timed_out,
    })
}
