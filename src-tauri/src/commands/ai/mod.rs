pub mod copilot;
mod history;
mod stream_parsing;
mod tool_registry;
mod tool_runtime;
mod turn;
mod types;

pub use tool_registry::create_tools;
pub use types::{
    AccessTokenResponse, AiErrorEventPayload, AiToolCallEventPayload, ChatMessage,
    DeviceCodeResponse, FunctionCall, FunctionDefinition, ToolCall, ToolDefinition,
};

use history::enrich_user_message_with_tagged_files;
use tool_runtime::{
    build_recording_input_payload, build_run_in_terminal_timeout_failure_message,
    execute_command_in_exec_channel, try_reconnect_terminal_after_timeout,
    try_recover_terminal_after_timeout, START_MARKER_EXPECT_MS, TIMEOUT_RECOVERY_GRACE_MS,
};
use turn::{
    cancel_persisted_agent_run, create_agent_run, fail_agent_run, resume_agent_run, run_agent_loop,
    AgentRunResume, ToolApprovalAction,
};

#[cfg(test)]
const DUMMY_TOOL_RESPONSE: &str = "Interrupted or skipped by user";

#[cfg(test)]
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

use crate::commands::{AiRunGuard, AppState};
use crate::ssh_manager::ssh::SSHClient;
use futures::StreamExt;
use genai::chat::{ChatMessage as GenaiMessage, ChatStreamEvent};
use rusqlite::{params, OptionalExtension};
#[cfg(test)]
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Emitter, State, Window};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(super) const AI_STREAM_IDLE_TIMEOUT_SECS: u64 = 45;

/// User-cancelled AI run; treat as a normal terminal outcome, not a provider error.
pub const AI_CANCELLED: &str = "CANCELLED";

pub fn is_ai_cancelled(err: &str) -> bool {
    err == AI_CANCELLED
}

/// Pure outcome classification for tests and terminal event dispatch (no Tauri).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiRunOutcome {
    Completed,
    PendingTools,
    Cancelled,
    Failed(String),
}

/// Terminal event kind planned for a finished AI run (provider-agnostic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiRunFinishEvent {
    /// Pending tool confirmation: no done/cancelled/error event.
    None,
    /// Emit `ai-done-{session}` with request_id.
    Done,
    /// Emit `ai-cancelled-{session}` with request_id (normal terminal).
    Cancelled,
    /// Emit `ai-error-{session}` with structured payload; command returns Err.
    Error(String),
}

pub fn classify_ai_run_result(result: &Result<Option<Vec<ToolCall>>, String>) -> AiRunOutcome {
    match result {
        Ok(Some(calls)) if !calls.is_empty() => AiRunOutcome::PendingTools,
        Ok(_) => AiRunOutcome::Completed,
        Err(e) if is_ai_cancelled(e) => AiRunOutcome::Cancelled,
        Err(e) => AiRunOutcome::Failed(e.clone()),
    }
}

/// Map a run result to the event + command return value without touching Tauri.
/// Cancel is a successful terminal outcome (`Ok(())`), never an error emit.
pub fn plan_ai_run_finish(
    result: &Result<Option<Vec<ToolCall>>, String>,
) -> (AiRunFinishEvent, Result<(), String>) {
    match classify_ai_run_result(result) {
        AiRunOutcome::PendingTools => (AiRunFinishEvent::None, Ok(())),
        AiRunOutcome::Completed => (AiRunFinishEvent::Done, Ok(())),
        AiRunOutcome::Cancelled => (AiRunFinishEvent::Cancelled, Ok(())),
        AiRunOutcome::Failed(e) => (AiRunFinishEvent::Error(e.clone()), Err(e)),
    }
}

/// Prefer cancelled over done/pending when the run token is already cancelled.
/// Closes the race between a successful empty-tools path and a late cancel.
pub fn plan_ai_run_finish_with_token(
    token: &CancellationToken,
    result: &Result<Option<Vec<ToolCall>>, String>,
) -> (AiRunFinishEvent, Result<(), String>) {
    if token.is_cancelled() {
        return plan_ai_run_finish(&Err(AI_CANCELLED.to_string()));
    }
    plan_ai_run_finish(result)
}

/// Dispatch run result: done / cancelled / error. Cancel returns Ok and emits `ai-cancelled-*`.
/// If `token` is already cancelled, always emit cancelled (never done/error for success paths).
fn finish_ai_run(
    window: &Window,
    session_id: &str,
    request_id: &str,
    result: Result<Option<Vec<ToolCall>>, String>,
) -> Result<(), String> {
    finish_ai_run_with_token(window, session_id, request_id, None, result)
}

fn finish_ai_run_with_token(
    window: &Window,
    session_id: &str,
    request_id: &str,
    token: Option<&CancellationToken>,
    result: Result<Option<Vec<ToolCall>>, String>,
) -> Result<(), String> {
    let (event, command_result) = match token {
        Some(token) => plan_ai_run_finish_with_token(token, &result),
        None => plan_ai_run_finish(&result),
    };
    match event {
        AiRunFinishEvent::None => {}
        AiRunFinishEvent::Done => {
            window
                .emit(&format!("ai-done-{}", session_id), request_id)
                .map_err(|e| e.to_string())?;
        }
        AiRunFinishEvent::Cancelled => {
            tracing::info!(
                "[AI] run cancelled session_id={} request_id={}",
                session_id,
                request_id
            );
            window
                .emit(&format!("ai-cancelled-{}", session_id), request_id)
                .ok();
        }
        AiRunFinishEvent::Error(ref e) => {
            tracing::error!(
                "[AI] run error session_id={} request_id={}: {}",
                session_id,
                request_id,
                e
            );
            window
                .emit(
                    &format!("ai-error-{}", session_id),
                    AiErrorEventPayload {
                        request_id: request_id.to_string(),
                        error: e.clone(),
                    },
                )
                .ok();
        }
    }
    command_result
}

#[cfg(test)]
mod ai_run_outcome_tests {
    use super::{
        classify_ai_run_result, plan_ai_run_finish, AiRunFinishEvent, AiRunOutcome, FunctionCall,
        ToolCall, AI_CANCELLED,
    };

    fn sample_tool_call() -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            tool_type: "function".to_string(),
            function: FunctionCall {
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            },
        }
    }

    #[test]
    fn classify_distinguishes_cancel_error_and_complete() {
        assert_eq!(classify_ai_run_result(&Ok(None)), AiRunOutcome::Completed);
        assert_eq!(
            classify_ai_run_result(&Ok(Some(vec![]))),
            AiRunOutcome::Completed
        );
        assert_eq!(
            classify_ai_run_result(&Ok(Some(vec![sample_tool_call()]))),
            AiRunOutcome::PendingTools
        );
        assert_eq!(
            classify_ai_run_result(&Err(AI_CANCELLED.to_string())),
            AiRunOutcome::Cancelled
        );
        assert_eq!(
            classify_ai_run_result(&Err("provider down".to_string())),
            AiRunOutcome::Failed("provider down".to_string())
        );
    }

    #[test]
    fn plan_finish_cancel_is_ok_and_never_error_event() {
        let (event, result) = plan_ai_run_finish(&Err(AI_CANCELLED.to_string()));
        assert_eq!(event, AiRunFinishEvent::Cancelled);
        assert!(result.is_ok());

        // Cancel must not share the error-dispatch path used by provider failures.
        let (err_event, err_result) = plan_ai_run_finish(&Err("provider down".to_string()));
        assert_eq!(
            err_event,
            AiRunFinishEvent::Error("provider down".to_string())
        );
        assert_eq!(err_result, Err("provider down".to_string()));
        assert_ne!(err_event, AiRunFinishEvent::Cancelled);
    }

    #[test]
    fn plan_finish_done_pending_and_failed_paths() {
        let (done_event, done_ok) = plan_ai_run_finish(&Ok(None));
        assert_eq!(done_event, AiRunFinishEvent::Done);
        assert!(done_ok.is_ok());

        let (pending_event, pending_ok) = plan_ai_run_finish(&Ok(Some(vec![sample_tool_call()])));
        assert_eq!(pending_event, AiRunFinishEvent::None);
        assert!(pending_ok.is_ok());

        let (empty_tools_event, empty_tools_ok) = plan_ai_run_finish(&Ok(Some(vec![])));
        assert_eq!(empty_tools_event, AiRunFinishEvent::Done);
        assert!(empty_tools_ok.is_ok());
    }

    #[test]
    fn plan_finish_with_token_prefers_cancel_over_done() {
        use super::plan_ai_run_finish_with_token;
        use tokio_util::sync::CancellationToken;

        let live = CancellationToken::new();
        let (event, result) = plan_ai_run_finish_with_token(&live, &Ok(None));
        assert_eq!(event, AiRunFinishEvent::Done);
        assert!(result.is_ok());

        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let (event, result) = plan_ai_run_finish_with_token(&cancelled, &Ok(None));
        assert_eq!(event, AiRunFinishEvent::Cancelled);
        assert!(result.is_ok());

        let (event, result) =
            plan_ai_run_finish_with_token(&cancelled, &Ok(Some(vec![sample_tool_call()])));
        assert_eq!(event, AiRunFinishEvent::Cancelled);
        assert!(result.is_ok());

        // Provider failure still surfaces as error when not cancelled.
        let (event, result) =
            plan_ai_run_finish_with_token(&live, &Err("provider down".to_string()));
        assert_eq!(event, AiRunFinishEvent::Error("provider down".to_string()));
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod ai_cancel_race_async_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio_util::sync::CancellationToken;

    use crate::commands::AiRunRegistry;

    /// Stream event kinds observed by a turn-like consumer (provider-agnostic).
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum MockStreamEvent {
        Response(String),
        Reasoning(String),
        ToolCall(String),
    }

    /// Mirrors the stream loop shape in `turn.rs`:
    /// biased cancel-first select over a blocking `stream.next()`, and **no**
    /// flush of undelivered tails when cancelled.
    async fn drain_mock_stream_until_cancelled(
        token: CancellationToken,
        mut rx: tokio::sync::mpsc::Receiver<MockStreamEvent>,
    ) -> Vec<MockStreamEvent> {
        let mut accepted = Vec::new();
        loop {
            let next = tokio::select! {
                biased;
                _ = token.cancelled() => {
                    // Intentionally do not drain remaining rx items (no tail flush).
                    break;
                }
                item = rx.recv() => item,
            };
            match next {
                Some(chunk) => accepted.push(chunk),
                None => break,
            }
        }
        accepted
    }

    /// Slow producer that does **not** observe the cancel token (like a remote
    /// SSE body still enqueued while the consumer already cancelled).
    async fn produce_slow_chunks(
        tx: tokio::sync::mpsc::Sender<MockStreamEvent>,
        n: usize,
        delay: Duration,
    ) {
        for i in 0..n {
            let event = if i % 5 == 4 {
                MockStreamEvent::ToolCall(format!("tool-{i}"))
            } else if i % 3 == 0 {
                MockStreamEvent::Reasoning(format!("think-{i}"))
            } else {
                MockStreamEvent::Response(format!("chunk-{i}"))
            };
            if tx.send(event).await.is_err() {
                break;
            }
            tokio::time::sleep(delay).await;
        }
    }

    #[tokio::test]
    async fn slow_stream_stops_accepting_after_cancel_without_tail_flush() {
        let token = CancellationToken::new();
        let (tx, rx) = tokio::sync::mpsc::channel::<MockStreamEvent>(64);

        // Producer ignores cancel (real providers keep delivering until dropped).
        let producer = tokio::spawn(async move {
            produce_slow_chunks(tx, 40, Duration::from_millis(10)).await;
        });

        let consumer_token = token.clone();
        let consumer =
            tokio::spawn(
                async move { drain_mock_stream_until_cancelled(consumer_token, rx).await },
            );

        // Mid-stream cancel after some chunks arrive.
        tokio::time::sleep(Duration::from_millis(55)).await;
        token.cancel();

        let accepted = tokio::time::timeout(Duration::from_secs(2), consumer)
            .await
            .expect("consumer should exit after cancel")
            .expect("consumer task");

        // Producer may still finish; accepted must stop at cancel boundary.
        let _ = tokio::time::timeout(Duration::from_secs(2), producer).await;

        assert!(
            !accepted.is_empty(),
            "should have accepted some chunks before cancel"
        );
        assert!(
            accepted.len() < 40,
            "cancel must stop accepting before the full sequence; got {}",
            accepted.len()
        );
        assert!(token.is_cancelled());
        // Mixed kinds prove response/reasoning/tool paths share the same gate.
        assert!(
            accepted
                .iter()
                .any(|e| matches!(e, MockStreamEvent::Response(_))),
            "expected at least one response chunk"
        );
    }

    #[tokio::test]
    async fn first_chunk_wait_can_cancel_before_any_accept() {
        let token = CancellationToken::new();
        let (tx, rx) = tokio::sync::mpsc::channel::<MockStreamEvent>(4);

        // Hold the first chunk until after cancel (first-token wait).
        let producer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let _ = tx
                .send(MockStreamEvent::Response("late-first".into()))
                .await;
        });

        let consumer_token = token.clone();
        let consumer =
            tokio::spawn(
                async move { drain_mock_stream_until_cancelled(consumer_token, rx).await },
            );

        tokio::time::sleep(Duration::from_millis(20)).await;
        token.cancel();

        let accepted = tokio::time::timeout(Duration::from_secs(2), consumer)
            .await
            .expect("consumer exits on cancel while blocked on next()")
            .expect("consumer task");
        let _ = tokio::time::timeout(Duration::from_secs(2), producer).await;

        assert!(
            accepted.is_empty(),
            "cancel during first-chunk wait must accept no events; got {:?}",
            accepted
        );
    }

    #[tokio::test]
    async fn cancel_matching_request_wakes_token_mismatched_does_not() {
        let registry = AiRunRegistry::new();
        let token_a = registry.register("sess-1", "req-a");
        assert!(!registry.cancel("sess-1", "req-other"));
        assert!(!token_a.is_cancelled());

        let wake = tokio::spawn({
            let token_a = token_a.clone();
            async move {
                token_a.cancelled().await;
            }
        });

        assert!(registry.cancel("sess-1", "req-a"));
        tokio::time::timeout(Duration::from_secs(1), wake)
            .await
            .expect("matching cancel should wake waiters")
            .expect("wake task");
        assert!(token_a.is_cancelled());
    }

    #[tokio::test]
    async fn replaced_request_a_finish_does_not_clear_b_and_cancel_b_wakes() {
        let registry = AiRunRegistry::new();
        let token_a = registry.register("sess-1", "req-a");
        let token_b = registry.register("sess-1", "req-b");

        assert!(token_a.is_cancelled());
        assert!(!token_b.is_cancelled());
        assert_eq!(
            registry.current_request_id("sess-1").as_deref(),
            Some("req-b")
        );

        // Old request A ends (AiRunGuard Drop / clear_if_matches).
        registry.clear_if_matches("sess-1", "req-a");
        assert_eq!(
            registry.current_request_id("sess-1").as_deref(),
            Some("req-b")
        );
        assert!(!token_b.is_cancelled());
        assert!(!registry.cancel("sess-1", "req-a"));
        assert!(!token_b.is_cancelled());

        let wake_b = tokio::spawn({
            let token_b = token_b.clone();
            async move {
                token_b.cancelled().await;
            }
        });
        assert!(registry.cancel("sess-1", "req-b"));
        tokio::time::timeout(Duration::from_secs(1), wake_b)
            .await
            .expect("cancel B should wake B token")
            .expect("wake task");
        assert!(token_b.is_cancelled());

        registry.clear_if_matches("sess-1", "req-b");
        assert!(registry.current_request_id("sess-1").is_none());
    }

    #[tokio::test]
    async fn immediate_resend_old_stream_cannot_pollute_new_request() {
        let registry = Arc::new(AiRunRegistry::new());
        let (tx_a, rx_a) = tokio::sync::mpsc::channel::<MockStreamEvent>(32);
        let (tx_b, rx_b) = tokio::sync::mpsc::channel::<MockStreamEvent>(32);

        let token_a = registry.register("sess-resend", "req-a");
        let consumer_a = tokio::spawn({
            let token_a = token_a.clone();
            async move { drain_mock_stream_until_cancelled(token_a, rx_a).await }
        });

        // Slow A stream still producing after UI stop + immediate resend.
        let producer_a = tokio::spawn(async move {
            produce_slow_chunks(tx_a, 30, Duration::from_millis(8)).await;
        });

        tokio::time::sleep(Duration::from_millis(30)).await;

        // User stops A (registry cancel) then immediately starts B.
        assert!(registry.cancel("sess-resend", "req-a"));
        let token_b = registry.register("sess-resend", "req-b");
        // register B also cancels A if cancel already did; either way A is cancelled.
        assert!(token_a.is_cancelled());
        assert!(!token_b.is_cancelled());
        assert_eq!(
            registry.current_request_id("sess-resend").as_deref(),
            Some("req-b")
        );

        let consumer_b = tokio::spawn({
            let token_b = token_b.clone();
            async move { drain_mock_stream_until_cancelled(token_b, rx_b).await }
        });
        let producer_b = tokio::spawn(async move {
            produce_slow_chunks(tx_b, 6, Duration::from_millis(5)).await;
        });

        // Stale cancel/clear for A must not kill B.
        registry.clear_if_matches("sess-resend", "req-a");
        assert!(!registry.cancel("sess-resend", "req-a"));
        assert!(!token_b.is_cancelled());

        let accepted_a = tokio::time::timeout(Duration::from_secs(2), consumer_a)
            .await
            .expect("A consumer exits")
            .expect("A task");
        let accepted_b = tokio::time::timeout(Duration::from_secs(2), consumer_b)
            .await
            .expect("B consumer finishes")
            .expect("B task");
        let _ = tokio::time::timeout(Duration::from_secs(2), producer_a).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), producer_b).await;

        assert!(
            accepted_a.len() < 30,
            "A stopped early: {}",
            accepted_a.len()
        );
        assert_eq!(
            accepted_b.len(),
            6,
            "B should fully accept its own stream; got {:?}",
            accepted_b
        );
        assert!(!token_b.is_cancelled());

        // Finish B cleanly.
        registry.clear_if_matches("sess-resend", "req-b");
        assert!(registry.current_request_id("sess-resend").is_none());
    }

    #[tokio::test]
    async fn concurrent_register_stress_single_current_and_cancelled_losers() {
        let registry = Arc::new(AiRunRegistry::new());
        let mut handles = Vec::new();
        let tokens = Arc::new(std::sync::Mutex::new(Vec::new()));

        for t in 0..12 {
            let registry = Arc::clone(&registry);
            let tokens = Arc::clone(&tokens);
            handles.push(tokio::spawn(async move {
                for r in 0..40 {
                    let id = format!("req-{t}-{r}");
                    let token = registry.register("sess-stress", &id);
                    tokens.lock().expect("tokens lock").push((id, token));
                    // Yield so other tasks interleave register calls.
                    tokio::task::yield_now().await;
                }
            }));
        }

        for handle in handles {
            handle.await.expect("stress task");
        }

        let current = registry
            .current_request_id("sess-stress")
            .expect("final current");

        let snapshot = tokens.lock().expect("tokens lock");
        let mut live_for_current = 0usize;
        let mut live_for_others = 0usize;
        for (id, token) in snapshot.iter() {
            if !token.is_cancelled() {
                if id == &current {
                    live_for_current += 1;
                } else {
                    live_for_others += 1;
                }
            }
        }
        assert_eq!(
            live_for_others, 0,
            "every non-current request token must be cancelled"
        );
        assert_eq!(
            live_for_current, 1,
            "current request must keep exactly one live token"
        );
        assert!(registry.cancel("sess-stress", &current));
        registry.clear_if_matches("sess-stress", &current);
        assert!(registry.current_request_id("sess-stress").is_none());
    }
}

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
    request_id: String,
) -> Result<(), String> {
    if request_id.is_empty() {
        return Err("request_id is required".to_string());
    }
    let cancelled_in_memory = state.cancel_ai_run(&session_id, &request_id);
    let cancelled_persisted =
        cancel_persisted_agent_run(state.inner(), &session_id, &request_id).await?;
    if cancelled_in_memory || cancelled_persisted {
        tracing::info!(
            "[AI] cancel requested session_id={} request_id={} persisted={}",
            session_id,
            request_id,
            cancelled_persisted
        );
    } else {
        tracing::debug!(
            "[AI] cancel ignored (no matching run) session_id={} request_id={}",
            session_id,
            request_id
        );
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
    request_id: String,
) -> Result<(), String> {
    if request_id.is_empty() {
        return Err("request_id is required".to_string());
    }

    // Register before any await so cancel can interrupt prep / DB / stream setup.
    let token = state.register_ai_run(&session_id, &request_id);
    let _run_guard = AiRunGuard::new(
        state.inner().clone(),
        session_id.clone(),
        request_id.clone(),
    );

    if token.is_cancelled() {
        return finish_ai_run(
            &window,
            &session_id,
            &request_id,
            Err(AI_CANCELLED.to_string()),
        );
    }

    let _ = window.emit(&format!("ai-started-{}", session_id), &request_id);

    let bound_ssh_session_id: Option<String> = match {
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
            .await
    } {
        Ok(value) => value,
        Err(error) => return finish_ai_run(&window, &session_id, &request_id, Err(error)),
    };

    if token.is_cancelled() {
        return finish_ai_run(
            &window,
            &session_id,
            &request_id,
            Err(AI_CANCELLED.to_string()),
        );
    }

    let content =
        enrich_user_message_with_tagged_files(&content, bound_ssh_session_id.as_deref()).await;

    if token.is_cancelled() {
        return finish_ai_run(
            &window,
            &session_id,
            &request_id,
            Err(AI_CANCELLED.to_string()),
        );
    }

    let run_context = match create_agent_run(state.inner(), &session_id, &request_id).await {
        Ok(context) => context,
        Err(error) => return finish_ai_run(&window, &session_id, &request_id, Err(error)),
    };

    let user_msg_id = Uuid::new_v4().to_string();
    let user_message_result = {
        let user_msg_id_owned = user_msg_id.clone();
        let session_id_owned = session_id.clone();
        let content_owned = content.clone();
        let run_id = run_context.run_id.clone();
        state
            .db_manager
            .run_blocking(move |conn| {
                conn.execute(
                    "INSERT INTO ai_messages (id, session_id, role, content, run_id, turn_index) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                    params![user_msg_id_owned, session_id_owned, "user", content_owned, run_id],
                )
                .map_err(|error| error.to_string())?;
                Ok(())
            })
            .await
    };
    if let Err(error) = user_message_result {
        fail_agent_run(state.inner(), &run_context, error.clone()).await;
        return finish_ai_run(&window, &session_id, &request_id, Err(error));
    }

    let is_agent_mode = mode.as_deref() == Some("agent");
    let is_ask_mode = mode.as_deref() == Some("ask");
    let tools = if is_agent_mode || is_ask_mode {
        Some(create_tools(is_agent_mode))
    } else {
        None
    };
    let result = run_agent_loop(
        window.clone(),
        state.inner().clone(),
        run_context,
        model_id,
        channel_id,
        is_agent_mode,
        tools,
        token.clone(),
        bound_ssh_session_id,
        thinking_level,
        request_id.clone(),
        None,
    )
    .await;

    finish_ai_run_with_token(&window, &session_id, &request_id, Some(&token), result)
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
    request_id: String,
) -> Result<(), String> {
    if request_id.is_empty() {
        return Err("request_id is required".to_string());
    }

    let token = state.register_ai_run(&session_id, &request_id);
    let _run_guard = AiRunGuard::new(
        state.inner().clone(),
        session_id.clone(),
        request_id.clone(),
    );

    if token.is_cancelled() {
        return finish_ai_run(
            &window,
            &session_id,
            &request_id,
            Err(AI_CANCELLED.to_string()),
        );
    }

    let _ = window.emit(&format!("ai-started-{}", session_id), &request_id);

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

    if token.is_cancelled() {
        return finish_ai_run(
            &window,
            &session_id,
            &request_id,
            Err(AI_CANCELLED.to_string()),
        );
    }

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

    if token.is_cancelled() {
        return finish_ai_run(
            &window,
            &session_id,
            &request_id,
            Err(AI_CANCELLED.to_string()),
        );
    }

    let run_context = match create_agent_run(state.inner(), &session_id, &request_id).await {
        Ok(context) => context,
        Err(error) => return finish_ai_run(&window, &session_id, &request_id, Err(error)),
    };
    let is_agent_mode = mode.as_deref() == Some("agent");
    let is_ask_mode = mode.as_deref() == Some("ask");
    let tools = if is_agent_mode || is_ask_mode {
        Some(create_tools(is_agent_mode))
    } else {
        None
    };
    let result = run_agent_loop(
        window.clone(),
        state.inner().clone(),
        run_context,
        model_id,
        channel_id,
        is_agent_mode,
        tools,
        token.clone(),
        bound_ssh_session_id,
        thinking_level,
        request_id.clone(),
        None,
    )
    .await;

    finish_ai_run_with_token(&window, &session_id, &request_id, Some(&token), result)
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
    run_id: String,
    turn_index: i64,
    tool_call_ids: Vec<String>,
    approval_ids: Vec<String>,
    approval_action: String,
    thinking_level: Option<String>,
    request_id: String,
) -> Result<(), String> {
    if request_id.is_empty() || run_id.is_empty() {
        return Err("request_id and run_id are required".to_string());
    }
    let approval_action = ToolApprovalAction::parse(&approval_action)?;
    let is_agent_mode = mode.as_deref() == Some("agent");
    let registration_generation = state.reserve_ai_run_generation();

    // Claim the persisted approval before touching the per-session cancellation
    // registry. Duplicate delivery must not replace and cancel the live executor.
    let resumed = resume_agent_run(
        state.inner(),
        &session_id,
        &request_id,
        is_agent_mode,
        &run_id,
        turn_index,
        &tool_call_ids,
        &approval_ids,
        approval_action,
    )
    .await?;
    let AgentRunResume::Resumed {
        context: run_context,
        tools: approved_calls,
        turn_index,
    } = resumed
    else {
        return Ok(());
    };

    let Some(token) =
        state.register_ai_run_if_not_superseded(&session_id, &request_id, registration_generation)
    else {
        // The approval was claimed before a newer request attached its token. Do
        // not let the stale approval execute side effects or replace that request.
        let _ = cancel_persisted_agent_run(state.inner(), &session_id, &request_id).await?;
        return finish_ai_run(
            &window,
            &session_id,
            &request_id,
            Err(AI_CANCELLED.to_string()),
        );
    };
    let _run_guard = AiRunGuard::new(
        state.inner().clone(),
        session_id.clone(),
        request_id.clone(),
    );
    if token.is_cancelled() {
        return finish_ai_run(
            &window,
            &session_id,
            &request_id,
            Err(AI_CANCELLED.to_string()),
        );
    }
    let _ = window.emit(&format!("ai-started-{}", session_id), &request_id);

    let db = state.db_manager.clone();
    let session_for_binding = session_id.clone();
    let requested_ssh = ssh_session_id.clone();
    let bound_ssh_session_id = match tokio::select! {
        biased;
        _ = token.cancelled() => return finish_ai_run(&window, &session_id, &request_id, Err(AI_CANCELLED.to_string())),
        value = db.run_blocking(move |conn| {
            let bound: Option<String> = conn.query_row(
                "SELECT ssh_session_id FROM ai_sessions WHERE id = ?1", params![session_for_binding], |row| row.get(0),
            ).ok();
            Ok(bound.filter(|id| !id.is_empty()).or(requested_ssh))
        }) => value,
    } {
        Ok(value) => value,
        Err(error) => return finish_ai_run(&window, &session_id, &request_id, Err(error)),
    };

    let tools = Some(create_tools(is_agent_mode));
    let result = run_agent_loop(
        window.clone(),
        state.inner().clone(),
        run_context,
        model_id,
        channel_id,
        is_agent_mode,
        tools,
        token.clone(),
        bound_ssh_session_id,
        thinking_level,
        request_id.clone(),
        Some((approved_calls, turn_index)),
    )
    .await;
    finish_ai_run_with_token(&window, &session_id, &request_id, Some(&token), result)
}

/// Returns durable approval state for the currently selected session. The UI restores
/// this directly instead of inferring pending calls from the last assistant message.
#[tauri::command]
pub async fn get_pending_tool_approvals(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<Option<AiToolCallEventPayload>, String> {
    state
        .db_manager
        .run_blocking(move |conn| {
            let pending: Option<(String, String, i64)> = conn
                .query_row(
                    "SELECT r.id, r.active_request_id, i.turn_index
                     FROM ai_runs r JOIN ai_tool_invocations i ON i.run_id = r.id
                     WHERE r.session_id = ?1 AND r.status = 'awaitingApproval'
                       AND i.status = 'awaitingApproval'
                     ORDER BY r.updated_at DESC, i.rowid ASC LIMIT 1",
                    params![session_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            let Some((run_id, request_id, turn_index)) = pending else {
                return Ok(None);
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
                .query_map(params![run_id.clone(), turn_index], |row| {
                    Ok((
                        ToolCall {
                            id: row.get(0)?,
                            tool_type: "function".to_string(),
                            function: FunctionCall {
                                name: row.get(1)?,
                                arguments: row.get(2)?,
                            },
                        },
                        row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    ))
                })
                .map_err(|error| error.to_string())?;
            let entries = rows
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            if entries.is_empty()
                || entries
                    .iter()
                    .any(|(_, approval_id)| approval_id.is_empty())
            {
                return Err("Persisted approval state is incomplete".to_string());
            }
            let approval_policies = entries
                .iter()
                .map(|(call, _)| {
                    tool_registry::tool_policy(&call.function.name)
                        .map(|policy| policy.approval)
                        .ok_or_else(|| "Persisted approval references an unknown tool".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Some(AiToolCallEventPayload {
                request_id,
                run_id,
                turn_index,
                tool_calls: entries.iter().map(|(call, _)| call.clone()).collect(),
                approval_ids: entries
                    .into_iter()
                    .map(|(_, approval_id)| approval_id)
                    .collect(),
                approval_policies,
            }))
        })
        .await
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
