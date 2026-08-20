use std::collections::HashSet;
use std::sync::Arc;

use genai::chat::{ChatMessage as GenaiMessage, ContentPart, MessageContent, ToolResponse};
use rusqlite::params;
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};

use crate::ai::prompts::SYSTEM_PROMPT;
use crate::ai::validator::{
    validate_and_fix_messages, MessagePayload, ToolCallFunction, ToolCallPayload,
};
use crate::commands::AppState;
use crate::config::types::{AiChannel, AiModel};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolResultEncoding {
    IndividualToolMessages,
    GroupedUserMessage,
}

/// Provider capabilities deliberately describe transport constraints rather than business policy.
/// Unknown OpenAI-compatible endpoints default to the conservative path: no reasoning replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ProviderCapabilities {
    pub reasoning_round_trip: bool,
    pub preserve_thought_signatures: bool,
    pub tool_result_encoding: ToolResultEncoding,
    pub requires_tool_call_ids: bool,
    pub openai_compatible: bool,
}

impl ProviderCapabilities {
    pub(super) fn for_channel_and_model(channel: &AiChannel, model: &AiModel) -> Self {
        let provider = channel.provider.trim().to_ascii_lowercase();
        let model_name = model.name.trim().to_ascii_lowercase();
        let endpoint = channel
            .endpoint
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let is_anthropic = provider == "anthropic" || provider == "claude";
        let is_copilot = provider == "copilot";
        let supports_reasoning = !is_anthropic
            && !is_copilot
            && (endpoint.contains("api.openai.com")
                || endpoint.contains("deepseek")
                || model_name.contains("deepseek")
                || model_name.starts_with("o1")
                || model_name.starts_with("o3")
                || model_name.starts_with("o4")
                || model_name.starts_with("gpt-5"));

        Self {
            reasoning_round_trip: supports_reasoning,
            // Thought signatures are provider-specific opaque values. Replay them only for
            // adapters/models explicitly known to support reasoning round trips; unknown
            // OpenAI-compatible endpoints and Copilot must not receive provider-only fields.
            preserve_thought_signatures: supports_reasoning,
            tool_result_encoding: if is_anthropic {
                ToolResultEncoding::GroupedUserMessage
            } else {
                ToolResultEncoding::IndividualToolMessages
            },
            requires_tool_call_ids: true,
            openai_compatible: !is_anthropic,
        }
    }
}

pub(super) struct ProviderMessageEncoder {
    capabilities: ProviderCapabilities,
}

impl ProviderMessageEncoder {
    pub(super) fn new(capabilities: ProviderCapabilities) -> Self {
        Self { capabilities }
    }

    fn assistant_message(&self, message: &ChatMessage) -> Option<(GenaiMessage, HashSet<String>)> {
        let content = message.content.clone().unwrap_or_default();
        let reasoning_content = self
            .capabilities
            .reasoning_round_trip
            .then(|| message.reasoning_content.clone())
            .flatten()
            .filter(|reasoning| !reasoning.is_empty());
        let genai_tool_calls: Vec<genai::chat::ToolCall> = message
            .tool_calls
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tool_call| {
                if tool_call.function.name.trim().is_empty() {
                    tracing::warn!(
                        "[AI] Dropping persisted tool call with empty function name: id={}",
                        tool_call.id
                    );
                    return None;
                }

                let parsed_args = match parse_tool_call_arguments(&tool_call.function.arguments) {
                    Ok(value) => value,
                    Err(_) => {
                        // Parser diagnostics can echo malformed JSON; do not log tool arguments.
                        tracing::warn!(
                            "[AI] Dropping persisted tool call with invalid arguments: id={}, name={}",
                            tool_call.id,
                            tool_call.function.name,
                        );
                        return None;
                    }
                };

                Some(genai::chat::ToolCall {
                    call_id: tool_call.id,
                    fn_name: tool_call.function.name,
                    fn_arguments: parsed_args,
                    thought_signatures: self
                        .capabilities
                        .preserve_thought_signatures
                        .then_some(tool_call.thought_signatures)
                        .flatten(),
                })
            })
            .collect();

        if genai_tool_calls.is_empty() {
            if content.is_empty() && reasoning_content.is_none() {
                tracing::warn!(
                    "[AI] Dropping empty assistant message after tool-call sanitization to avoid invalid payload."
                );
                return None;
            }
            return Some((
                attach_reasoning_content(GenaiMessage::assistant(content), reasoning_content),
                HashSet::new(),
            ));
        }

        let pending_tool_call_ids = genai_tool_calls
            .iter()
            .map(|call| call.call_id.clone())
            .collect();
        let mut parts = Vec::new();
        if !content.is_empty() {
            parts.push(ContentPart::Text(content));
        }
        parts.extend(genai_tool_calls.into_iter().map(ContentPart::ToolCall));
        Some((
            attach_reasoning_content(
                GenaiMessage::assistant(MessageContent::from_parts(parts)),
                reasoning_content,
            ),
            pending_tool_call_ids,
        ))
    }

    pub(super) fn encode(&self, history: &[ChatMessage]) -> Vec<GenaiMessage> {
        let mut messages = Vec::with_capacity(history.len());
        let mut pending_tool_call_ids: HashSet<String> = HashSet::new();
        let mut index = 0;

        while index < history.len() {
            let message = &history[index];
            match message.role.as_str() {
                "system" => {
                    pending_tool_call_ids.clear();
                    messages.push(GenaiMessage::system(
                        message.content.clone().unwrap_or_default(),
                    ));
                }
                "user" => {
                    pending_tool_call_ids.clear();
                    messages.push(GenaiMessage::user(
                        message.content.clone().unwrap_or_default(),
                    ));
                }
                "assistant" => {
                    let Some((assistant, pending_ids)) = self.assistant_message(message) else {
                        index += 1;
                        continue;
                    };
                    messages.push(assistant);
                    pending_tool_call_ids = pending_ids;

                    if self.capabilities.tool_result_encoding
                        == ToolResultEncoding::GroupedUserMessage
                        && !pending_tool_call_ids.is_empty()
                    {
                        let mut tool_responses = Vec::new();
                        while index + 1 < history.len() && history[index + 1].role == "tool" {
                            index += 1;
                            let tool_message = &history[index];
                            let call_id = tool_message.tool_call_id.clone().unwrap_or_default();
                            if call_id.is_empty() || !pending_tool_call_ids.remove(&call_id) {
                                tracing::warn!(
                                    "[AI] Dropping Anthropic tool result without a matching call id: {}",
                                    call_id
                                );
                                continue;
                            }
                            tool_responses.push(ToolResponse::new(
                                call_id,
                                tool_message.content.clone().unwrap_or_default(),
                            ));
                        }

                        if !tool_responses.is_empty() {
                            // Anthropic requires every result for one assistant tool-use turn in
                            // one immediate user message. MessageContent preserves result-before-
                            // text ordering because this message contains no text parts.
                            messages.push(GenaiMessage::user(MessageContent::from(tool_responses)));
                        }
                    }
                }
                "tool" => {
                    let call_id = message.tool_call_id.clone().unwrap_or_default();
                    if call_id.is_empty()
                        || (self.capabilities.requires_tool_call_ids
                            && !pending_tool_call_ids.remove(&call_id))
                    {
                        tracing::warn!(
                            "[AI] Dropping persisted tool result with unmatched tool call id: {}",
                            call_id
                        );
                    } else {
                        messages.push(GenaiMessage::from(ToolResponse::new(
                            call_id,
                            message.content.clone().unwrap_or_default(),
                        )));
                    }
                }
                _ => {
                    pending_tool_call_ids.clear();
                    messages.push(GenaiMessage::user(
                        message.content.clone().unwrap_or_default(),
                    ));
                }
            }
            index += 1;
        }

        tracing::debug!(
            "[AI] Encoded provider history: openai_compatible={}, reasoning_round_trip={}, tool_result_encoding={:?}, input_messages={}, output_messages={}",
            self.capabilities.openai_compatible,
            self.capabilities.reasoning_round_trip,
            self.capabilities.tool_result_encoding,
            history.len(),
            messages.len()
        );
        messages
    }
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

pub(super) fn to_genai_messages(
    history: Vec<ChatMessage>,
    capabilities: ProviderCapabilities,
) -> Vec<GenaiMessage> {
    ProviderMessageEncoder::new(capabilities).encode(&history)
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ContextBudget {
    context_window_tokens: usize,
    response_reserve_tokens: usize,
}

impl ContextBudget {
    fn input_limit_tokens(self) -> usize {
        self.context_window_tokens
            .saturating_sub(self.response_reserve_tokens)
            .max(1_024)
    }
}

fn context_budget_for_model(channel: &AiChannel, model: &AiModel) -> ContextBudget {
    let inferred_window = if channel.provider.eq_ignore_ascii_case("anthropic") {
        200_000
    } else if model.name.to_ascii_lowercase().contains("deepseek") {
        64_000
    } else {
        // Conservative for unknown OpenAI-compatible endpoints: compact before most common
        // 32k windows overflow rather than assuming a large provider-specific limit.
        32_000
    };
    let context_window_tokens = model.context_window.unwrap_or(inferred_window).max(2_048) as usize;
    let response_reserve_tokens = model
        .response_reserve
        .map(|value| value as usize)
        .unwrap_or_else(|| context_window_tokens.min(8_192) / 4)
        .min(context_window_tokens.saturating_sub(1_024));

    ContextBudget {
        context_window_tokens,
        response_reserve_tokens,
    }
}

fn approximate_tokens(message: &ChatMessage) -> usize {
    let tool_calls = message
        .tool_calls
        .as_ref()
        .map(|calls| {
            calls
                .iter()
                .map(|call| {
                    call.id.len()
                        + call.function.name.len()
                        + call.function.arguments.len()
                        + call
                            .thought_signatures
                            .as_ref()
                            .map(|signatures| signatures.iter().map(String::len).sum::<usize>())
                            .unwrap_or_default()
                })
                .sum::<usize>()
        })
        .unwrap_or_default();
    let characters = message.role.len()
        + message
            .content
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + message
            .reasoning_content
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + message
            .tool_call_id
            .as_ref()
            .map(String::len)
            .unwrap_or_default()
        + tool_calls;
    (characters.saturating_add(3) / 4).max(1)
}

fn truncate_for_context(text: &str, max_chars: usize, label: &str) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let prefix: String = text.chars().take(max_chars).collect();
    format!(
        "{}\n[{} truncated for model context; original length: {} characters]",
        prefix,
        label,
        text.chars().count()
    )
}

fn bound_observations_for_context(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .cloned()
        .map(|mut message| {
            if let Some(content) = message.content.as_deref() {
                let (limit, label) = if message.role == "tool" {
                    (6_000, "tool observation")
                } else {
                    (12_000, "message")
                };
                message.content = Some(truncate_for_context(content, limit, label));
            }
            if let Some(reasoning) = message.reasoning_content.as_deref() {
                message.reasoning_content =
                    Some(truncate_for_context(reasoning, 4_000, "reasoning"));
            }
            message
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum ContextField {
    Content(usize),
    Reasoning(usize),
}

fn largest_context_field(messages: &[ChatMessage]) -> Option<ContextField> {
    let mut largest: Option<(usize, ContextField)> = None;
    let mut consider = |length: usize, field: ContextField| {
        if largest
            .as_ref()
            .map(|(current, _)| length > *current)
            .unwrap_or(true)
        {
            largest = Some((length, field));
        }
    };

    for (message_index, message) in messages.iter().enumerate() {
        if let Some(content) = message.content.as_deref() {
            if content.chars().count() > 128 {
                consider(
                    content.chars().count(),
                    ContextField::Content(message_index),
                );
            }
        }
        if let Some(reasoning) = message.reasoning_content.as_deref() {
            if reasoning.chars().count() > 64 {
                consider(
                    reasoning.chars().count(),
                    ContextField::Reasoning(message_index),
                );
            }
        }
    }

    largest.map(|(_, field)| field)
}

fn shrink_context_field(messages: &mut [ChatMessage], field: ContextField) -> bool {
    match field {
        ContextField::Content(message_index) => {
            let Some(content) = messages[message_index].content.clone() else {
                return false;
            };
            let label = if messages[message_index].role == "tool" {
                "tool observation"
            } else {
                "message"
            };
            let length = content.chars().count();
            let replacement = if length > 512 {
                truncate_for_context(&content, (length / 2).max(256), label)
            } else {
                format!("[{} omitted to fit the model context]", label)
            };
            if replacement == content {
                return false;
            }
            messages[message_index].content = Some(replacement);
            true
        }
        ContextField::Reasoning(message_index) => {
            let Some(reasoning) = messages[message_index].reasoning_content.clone() else {
                return false;
            };
            let length = reasoning.chars().count();
            let replacement = if length > 512 {
                truncate_for_context(&reasoning, (length / 2).max(256), "reasoning")
            } else {
                "[reasoning omitted to fit the model context]".to_string()
            };
            if replacement == reasoning {
                return false;
            }
            messages[message_index].reasoning_content = Some(replacement);
            true
        }
    }
}

fn fit_messages_to_context_limit(
    messages: &mut [ChatMessage],
    input_limit_tokens: usize,
) -> Result<(), String> {
    let initial_tokens: usize = messages.iter().map(approximate_tokens).sum();
    let mut reductions = 0usize;

    while messages.iter().map(approximate_tokens).sum::<usize>() > input_limit_tokens {
        if reductions >= 10_000 {
            return Err(
                "AI context could not be compacted within the configured model budget.".to_string(),
            );
        }
        let Some(field) = largest_context_field(messages) else {
            return Err("AI context is too large to preserve a valid tool-call history within the configured model budget.".to_string());
        };
        if !shrink_context_field(messages, field) {
            return Err(
                "AI context compaction made no progress while enforcing the model budget."
                    .to_string(),
            );
        }
        reductions += 1;
    }

    if reductions > 0 {
        tracing::warn!(
            "[AI] Context hard-limit enforcement: original_tokens≈{}, final_tokens≈{}, input_limit_tokens={}, reductions={}",
            initial_tokens,
            messages.iter().map(approximate_tokens).sum::<usize>(),
            input_limit_tokens,
            reductions
        );
    }
    Ok(())
}

fn split_complete_turns(messages: &[ChatMessage]) -> Vec<Vec<ChatMessage>> {
    let mut turns = Vec::new();
    let mut current = Vec::new();

    for message in messages.iter().cloned() {
        if message.role == "user" && !current.is_empty() {
            turns.push(current);
            current = Vec::new();
        }
        current.push(message);
    }
    if !current.is_empty() {
        turns.push(current);
    }
    turns
}

fn structured_compaction_summary(
    discarded_turns: &[Vec<ChatMessage>],
    kept_turns: &[Vec<ChatMessage>],
) -> ChatMessage {
    let discarded_messages: Vec<&ChatMessage> = discarded_turns.iter().flatten().collect();
    let kept_messages: Vec<&ChatMessage> = kept_turns.iter().flatten().collect();
    let current_goal = kept_messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .or_else(|| {
            discarded_messages
                .iter()
                .rev()
                .find(|message| message.role == "user")
        })
        .and_then(|message| message.content.as_deref())
        .map(|content| truncate_for_context(content, 1_200, "current task goal"))
        .unwrap_or_else(|| "No explicit user goal was retained.".to_string());

    let constraints: Vec<String> = discarded_messages
        .iter()
        .filter(|message| message.role == "user")
        .filter_map(|message| message.content.as_deref())
        .rev()
        .take(3)
        .map(|content| truncate_for_context(content, 600, "constraint"))
        .collect();
    let observations: Vec<String> = discarded_messages
        .iter()
        .filter(|message| message.role == "tool")
        .filter_map(|message| message.content.as_deref())
        .rev()
        .take(4)
        .map(|content| truncate_for_context(content, 800, "tool observation"))
        .collect();

    let mut summary = format!(
        "\n\n[Context compaction summary]\nCurrent task goal:\n- {}\n",
        current_goal
    );
    if constraints.is_empty() {
        summary.push_str("Key constraints / decisions:\n- No earlier user constraints retained.\n");
    } else {
        summary.push_str("Key constraints / decisions:\n");
        for constraint in constraints.into_iter().rev() {
            summary.push_str("- ");
            summary.push_str(&constraint);
            summary.push('\n');
        }
    }
    if observations.is_empty() {
        summary.push_str("Remote system state / observations:\n- No tool observations retained.\n");
    } else {
        summary.push_str("Remote system state / observations:\n");
        for observation in observations.into_iter().rev() {
            summary.push_str("- ");
            summary.push_str(&observation);
            summary.push('\n');
        }
    }
    summary
        .push_str("Full raw history remains available in SQLite; this summary is request-local.\n");

    ChatMessage {
        role: "system".to_string(),
        content: Some(summary),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
        created_at: None,
        model_id: None,
    }
}

pub(super) fn compact_dialog_messages_for_context(
    dialog_messages: Vec<ChatMessage>,
    budget: ContextBudget,
) -> Vec<ChatMessage> {
    let bounded = bound_observations_for_context(&dialog_messages);
    let input_limit = budget.input_limit_tokens();
    let total_tokens: usize = bounded.iter().map(approximate_tokens).sum();
    if total_tokens <= input_limit {
        return bounded;
    }

    let turns = split_complete_turns(&bounded);
    let mut kept_start = turns.len();
    let mut kept_tokens: usize = 0;
    for index in (0..turns.len()).rev() {
        let turn_tokens: usize = turns[index].iter().map(approximate_tokens).sum();
        if kept_start == turns.len() || kept_tokens.saturating_add(turn_tokens) <= input_limit {
            kept_start = index;
            kept_tokens = kept_tokens.saturating_add(turn_tokens);
        } else {
            break;
        }
    }

    let (discarded, kept) = turns.split_at(kept_start);
    let mut compacted = Vec::new();
    if !discarded.is_empty() {
        compacted.push(structured_compaction_summary(discarded, kept));
    }
    compacted.extend(kept.iter().flatten().cloned());

    tracing::info!(
        "[AI] Context compacted: original_tokens≈{}, compacted_tokens≈{}, input_limit_tokens={}, context_window_tokens={}, response_reserve_tokens={}, discarded_turns={}, kept_turns={}",
        total_tokens,
        compacted.iter().map(approximate_tokens).sum::<usize>(),
        input_limit,
        budget.context_window_tokens,
        budget.response_reserve_tokens,
        discarded.len(),
        kept.len()
    );
    compacted
}

/// Ratio of the effective context window at which LLM compaction triggers.
///
/// Industry consensus (Claude Code ~83.5%, grok build 85%, pi window−16K):
/// compaction must fire before the window is full because the compaction
/// request itself needs room for its input plus the summary output.
pub(super) const COMPACTION_THRESHOLD_RATIO: f64 = 0.85;

/// Cap on a single compaction summary output (tokens). Kept modest so the
/// compaction request itself fits inside the window even at the trigger point.
pub(super) const COMPACTION_OUTPUT_TOKEN_CAP: u64 = 4_096;

/// Consecutive compaction failures after which auto-compaction is skipped for
/// the session (circuit breaker, mirroring Claude Code's `XLd=3`).
pub(super) const COMPACTION_MAX_CONSECUTIVE_FAILURES: i64 = 3;

/// Effective context window used for the compaction threshold:
/// `context_window − response_reserve` (the room a next response needs).
pub(super) fn effective_context_window(context_window: u64, response_reserve: u64) -> u64 {
    context_window.saturating_sub(response_reserve).max(1_024)
}

/// True when the last response's total tokens reached the compaction threshold.
pub(super) fn should_compact_context(total_tokens: u64, context_window: u64, response_reserve: u64) -> bool {
    let effective = effective_context_window(context_window, response_reserve);
    total_tokens as f64 >= effective as f64 * COMPACTION_THRESHOLD_RATIO
}

/// Max output tokens for the compaction request:
/// `min(0.8 × response_reserve, COMPACTION_OUTPUT_TOKEN_CAP)` (pi's formula,
/// capped so even small windows leave the compaction request room to breathe).
pub(super) fn compaction_output_budget(response_reserve: u64) -> u64 {
    let from_reserve = (response_reserve as f64 * 0.8) as u64;
    from_reserve.min(COMPACTION_OUTPUT_TOKEN_CAP).max(256)
}

/// Serialize dialog messages to plain text for the summarizer, mirroring pi's
/// `serializeConversation` and opencode's `serialize`: tool calls and results
/// are rendered inline so the model sees the full exchange without receiving
/// provider-shaped tool messages.
pub(super) fn serialize_messages_for_compaction(messages: &[ChatMessage]) -> String {
    let mut parts = Vec::new();
    for message in messages {
        match message.role.as_str() {
            "user" => {
                if let Some(content) = message.content.as_deref() {
                    if !content.is_empty() {
                        parts.push(format!("[User]: {content}"));
                    }
                }
            }
            "assistant" => {
                if let Some(content) = message.content.as_deref() {
                    if !content.is_empty() {
                        parts.push(format!("[Assistant]: {content}"));
                    }
                }
                if let Some(reasoning) = message.reasoning_content.as_deref() {
                    if !reasoning.is_empty() {
                        parts.push(format!("[Assistant reasoning]: {reasoning}"));
                    }
                }
                if let Some(calls) = message.tool_calls.as_ref() {
                    for call in calls {
                        parts.push(format!(
                            "[Assistant tool call]: {}({})",
                            call.function.name, call.function.arguments
                        ));
                    }
                }
            }
            "tool" => {
                let content = message.content.as_deref().unwrap_or_default();
                let mut text = format!("[Tool result]: {content}");
                if let Some(id) = message.tool_call_id.as_deref() {
                    text = format!("[Tool result ({id})]: {content}");
                }
                parts.push(text);
            }
            _ => {
                if let Some(content) = message.content.as_deref() {
                    if !content.is_empty() {
                        parts.push(format!("[{}]: {content}", message.role));
                    }
                }
            }
        }
    }
    parts.join("\n\n")
}

/// Extract the `<summary>...</summary>` block from a compaction response,
/// mirroring Claude Code's `formatCompactSummary()`. Falls back to the raw
/// text with any `<analysis>` scratchpad stripped.
pub(super) fn extract_compaction_summary(raw: &str) -> String {
    if let Some(start) = raw.find("<summary>") {
        let after = &raw[start + "<summary>".len()..];
        if let Some(end) = after.find("</summary>") {
            let summary = after[..end].trim();
            if !summary.is_empty() {
                return summary.to_string();
            }
        }
    }
    // No <summary> tags: drop an <analysis> scratchpad if present, keep the rest.
    let stripped = strip_analysis_block(raw);
    let trimmed = stripped.trim();
    if trimmed.is_empty() {
        raw.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// Remove a `<analysis>...</analysis>` scratchpad block (and the tags
/// themselves) from a compaction response, keeping any surrounding text.
fn strip_analysis_block(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut rest = raw;
    loop {
        match rest.find("<analysis>") {
            Some(start) => {
                result.push_str(&rest[..start]);
                let after = &rest[start + "<analysis>".len()..];
                match after.find("</analysis>") {
                    Some(end) => {
                        rest = &after[end + "</analysis>".len()..];
                    }
                    None => {
                        // Unterminated analysis tag: keep the remainder as-is.
                        result.push_str(after);
                        break;
                    }
                }
            }
            None => {
                result.push_str(rest);
                break;
            }
        }
    }
    result
}

/// Build the LLM compaction prompt (Claude Code 9-section style). When a
/// previous summary exists it is passed as authoritative context that must be
/// carried forward (Codex issue #14347's "progressive amnesia" fix).
pub(super) fn build_compaction_prompt(previous_summary: Option<&str>) -> String {
    let mut prompt = String::from(
        "CRITICAL: Respond with TEXT ONLY. Do NOT call any tools.\n\
         - Do NOT use Read, Bash, Grep, Glob, Edit, Write, or ANY other tool.\n\
         - You already have all the context you need in the conversation above.\n\
         - Tool calls will be REJECTED and will waste your only turn — you will fail the task.\n\
         - Your entire response must be plain text: an <analysis> block followed by a <summary> block.\n\n\
         Your task is to create a detailed summary of the conversation so far, paying close \
         attention to the user's explicit requests and your previous actions.\n\
         This summary should be thorough in capturing technical details, code patterns, and \
         architectural decisions that would be essential for continuing development work \
         without losing context.\n\n\
         Before providing your final summary, wrap your analysis in <analysis> tags to organize \
         your thoughts and ensure you've covered all necessary points. In your analysis process:\n\
         1. Chronologically analyze each message and section of the conversation. For each \
         section thoroughly identify:\n\
            - The user's explicit requests and intents\n\
            - Your approach to addressing the user's requests\n\
            - Key decisions, technical concepts and code patterns\n\
            - Specific details like: file names / full code snippets / function signatures / file edits\n\
            - Errors that you ran into and how you fixed them\n\
            - Pay special attention to specific user feedback that you received, especially if \
         the user told you to do something differently.\n\
         2. Double-check for technical accuracy and completeness, addressing each required \
         element thoroughly.\n\n\
         Your summary should include the following sections:\n\
         1. Primary Request and Intent: Capture all of the user's explicit requests and intents in detail\n\
         2. Key Technical Concepts: List all important technical concepts, technologies, and frameworks discussed.\n\
         3. Files and Code Sections: Enumerate specific files and code sections examined, modified, or created. \
         Pay special attention to the most recent messages and include full code snippets where applicable.\n\
         4. Errors and fixes: List all errors that you ran into, and how you fixed them.\n\
         5. Problem Solving: Document problems solved and any ongoing troubleshooting efforts.\n\
         6. All user messages: List ALL user messages that are not tool results. These are critical \
         for understanding the users' feedback and changing intent.\n\
         7. Pending Tasks: Outline any pending tasks that you have explicitly been asked to work on.\n\
         8. Current Work: Describe in detail precisely what was being worked on immediately before \
         this summary request, paying special attention to the most recent messages from both user \
         and assistant. Include file names and code snippets where applicable.\n\
         9. Optional Next Step: List the next step that you will take that is related to the most \
         recent work you were doing. IMPORTANT: ensure that this step is DIRECTLY in line with the \
         user's most recent explicit requests. If there is a next step, include direct quotes from \
         the most recent conversation showing exactly what task you were working on and where you \
         left off. This should be verbatim to prevent any drift in task interpretation.\n\n\
         Example output structure:\n\
         <analysis>\n\
         [Chronological, section-by-section reasoning — this is a scratchpad and will be discarded.]\n\
         </analysis>\n\n\
         <summary>\n\
         1. Primary Request and Intent:\n   ...\n\
         2. Key Technical Concepts:\n   ...\n\
         [...all 9 sections...]\n\
         </summary>\n",
    );

    if let Some(previous) = previous_summary {
        if !previous.is_empty() {
            prompt.push_str(&format!(
                "\n\nThis conversation contains a summary from a previous compaction. \
                 Treat it as authoritative for the early history and carry its still-relevant \
                 information forward into your new summary (progress, key decisions and WHY, \
                 outcomes, direction). Never discard entries from previous compactions — each \
                 compaction adds to a cumulative record.\n\n\
                 <previous_summary>\n{previous}\n</previous_summary>\n"
            ));
        }
    }

    prompt.push_str(
        "\n\n<conversation>\n{conversation}\n</conversation>\n\n\
         IMPORTANT: Do NOT use any tools. You MUST respond with ONLY the <analysis> and \
         <summary> blocks as your text output.",
    );
    prompt
}

/// Group messages into assistant-turn groups so compaction can cut only at
/// complete-turn boundaries (tool_use/tool_result pairs are never split,
/// mirroring Claude Code's `Cdr` grouping and pi's "never cut at toolResult").
fn split_complete_turn_groups(messages: &[ChatMessage]) -> Vec<Vec<ChatMessage>> {
    let mut groups: Vec<Vec<ChatMessage>> = Vec::new();
    for message in messages {
        if message.role == "assistant" && message.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
            // A tool-calling assistant message starts a new group; everything
            // after it (tool results, follow-up text) joins until the next
            // assistant message.
            groups.push(vec![message.clone()]);
        } else if message.role == "assistant" && !groups.is_empty() {
            // Plain assistant text after a tool group: append to the current
            // group only if the group has no plain text yet, else start a new one.
            let last = groups.last_mut().expect("groups is not empty");
            if last.iter().all(|m| m.role == "tool" || m.tool_calls.is_some()) {
                last.push(message.clone());
            } else {
                groups.push(vec![message.clone()]);
            }
        } else if message.role == "assistant" {
            groups.push(vec![message.clone()]);
        } else if let Some(last) = groups.last_mut() {
            last.push(message.clone());
        } else {
            // Leading non-assistant messages (e.g. the very first user message).
            groups.push(vec![message.clone()]);
        }
    }
    groups
}

/// Select which message groups to summarize and which to keep verbatim.
///
/// Keeps the most recent groups within `keep_tokens` (mirroring pi's
/// `keepRecentTokens` default 20K); everything older is summarized. The cut is
/// always at a complete-turn boundary, so tool pairs survive.
pub(super) fn select_compaction_split(
    messages: &[ChatMessage],
    keep_tokens: usize,
) -> (Vec<ChatMessage>, Vec<ChatMessage>) {
    let groups = split_complete_turn_groups(messages);
    let mut kept_start = groups.len();
    let mut kept_tokens = 0usize;
    for index in (0..groups.len()).rev() {
        let group_tokens: usize = groups[index]
            .iter()
            .map(|m| {
                let chars = m
                    .content
                    .as_deref()
                    .map(str::len)
                    .unwrap_or_default()
                    + m
                        .reasoning_content
                        .as_deref()
                        .map(str::len)
                        .unwrap_or_default()
                    + m
                        .tool_calls
                        .as_ref()
                        .map(|calls| {
                            calls
                                .iter()
                                .map(|call| call.function.name.len() + call.function.arguments.len())
                                .sum::<usize>()
                        })
                        .unwrap_or_default();
                (chars + 3) / 4
            })
            .sum();
        if kept_start == groups.len() || kept_tokens.saturating_add(group_tokens) <= keep_tokens {
            kept_start = index;
            kept_tokens = kept_tokens.saturating_add(group_tokens);
        } else {
            break;
        }
    }

    let (to_summarize, to_keep) = groups.split_at(kept_start);
    (
        to_summarize.iter().flatten().cloned().collect(),
        to_keep.iter().flatten().cloned().collect(),
    )
}

pub(super) async fn load_history(
    state: &Arc<AppState>,
    session_id: &str,
    is_agent_mode: bool,
    ssh_session_id: Option<&str>,
    channel: &AiChannel,
    model: &AiModel,
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

    let (global_prompt, server_prompt) = {
        let config = state.config.lock().await;
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
        (gp, sp)
    };

    let dialog_messages: Vec<ChatMessage> = {
        let session_id_owned = session_id.to_string();
        state
            .db_manager
            .run_blocking(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT role, content, reasoning_content, tool_calls, tool_call_id, created_at, model_id, is_compaction_summary
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
                        let is_compaction_summary: bool = row
                            .get::<_, i64>(7)
                            .map(|value| value != 0)
                            .unwrap_or(false);

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

                        Ok((
                            ChatMessage {
                                role,
                                content,
                                reasoning_content: reasoning_raw,
                                tool_calls,
                                tool_call_id,
                                created_at: Some(created_at),
                                model_id,
                            },
                            is_compaction_summary,
                        ))
                    })
                    .map_err(|e| e.to_string())?;

                let mut all_messages: Vec<(ChatMessage, bool)> = Vec::new();
                for row in rows {
                    let msg = row.map_err(|e| e.to_string())?;
                    all_messages.push(msg);
                }

                // The last compaction summary replaces everything before it.
                // Messages before the summary stay in the DB (the UI renders
                // the original conversation) but are not sent to the model
                // again. The summary itself is kept as a user-role message.
                let mut last_summary_index: Option<usize> = None;
                for (index, (_, is_summary)) in all_messages.iter().enumerate() {
                    if *is_summary {
                        last_summary_index = Some(index);
                    }
                }

                let dialog_messages: Vec<ChatMessage> = if let Some(summary_index) =
                    last_summary_index
                {
                    all_messages
                        .into_iter()
                        .enumerate()
                        .filter(|(index, (msg, _))| *index >= summary_index && msg.role != "system")
                        .map(|(_, (msg, _))| msg)
                        .collect()
                } else {
                    all_messages
                        .into_iter()
                        .map(|(msg, _)| msg)
                        .filter(|m| m.role != "system")
                        .collect()
                };

                Ok(dialog_messages)
            })
            .await?
    };
    let context_budget = context_budget_for_model(channel, model);
    let mut dialog_messages = compact_dialog_messages_for_context(dialog_messages, context_budget);
    let compaction_summary = match dialog_messages.first() {
        Some(message) if message.role == "system" => dialog_messages.remove(0).content,
        _ => None,
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
    if let Some(summary) = compaction_summary {
        // Keep compaction context in the sole system message. A mid-history system message is
        // invalid for the validator and rejected by stricter providers such as Anthropic.
        full_system_prompt.push_str(&summary);
    }

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
    fit_messages_to_context_limit(&mut messages, context_budget.input_limit_tokens())?;

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
                        thought_signatures: c.thought_signatures.clone(),
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

    let mut validated_messages: Vec<ChatMessage> = payload_messages
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
                        thought_signatures: c.thought_signatures.clone(),
                    })
                    .collect()
            }),
            tool_call_id: m.tool_call_id,
            created_at: m.created_at,
            model_id: m.model_id,
        })
        .collect();

    // Validation can add dummy tool results for interrupted historical calls. Enforce the
    // configured input budget again after that structural repair, without rewriting the
    // recorded tool-call arguments or provider signatures.
    fit_messages_to_context_limit(&mut validated_messages, context_budget.input_limit_tokens())?;

    Ok(validated_messages)
}

#[cfg(test)]
mod context_tests {
    use super::*;

    fn message(role: &str, content: String) -> ChatMessage {
        ChatMessage {
            role: role.to_string(),
            content: Some(content),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            created_at: None,
            model_id: None,
        }
    }

    fn tool_call(id: &str, arguments: String) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            tool_type: "function".to_string(),
            function: FunctionCall {
                name: "write_file".to_string(),
                arguments,
            },
            thought_signatures: None,
        }
    }

    #[test]
    fn hard_context_limit_keeps_tool_batch_structure_and_valid_arguments() {
        let call_id = "call_1";
        let mut assistant = message("assistant", "assistant response ".repeat(500));
        assistant.tool_calls = Some(vec![tool_call(call_id, r#"{"path":"/tmp/a"}"#.to_string())]);
        let mut tool = message("tool", "terminal observation ".repeat(2_000));
        tool.tool_call_id = Some(call_id.to_string());

        let mut messages = vec![
            message("system", "system instruction ".repeat(500)),
            message("user", "current goal ".repeat(1_000)),
            assistant,
            tool,
        ];

        fit_messages_to_context_limit(&mut messages, 512).unwrap();

        assert!(messages.iter().map(approximate_tokens).sum::<usize>() <= 512);
        let retained_call = messages[2].tool_calls.as_ref().unwrap().first().unwrap();
        assert_eq!(retained_call.id, call_id);
        assert!(parse_tool_call_arguments(&retained_call.function.arguments).is_ok());
        assert_eq!(messages[3].tool_call_id.as_deref(), Some(call_id));
    }

    #[test]
    fn hard_context_limit_does_not_rewrite_tool_inputs_or_thought_signatures() {
        let arguments = format!(r#"{{"path":"/tmp/a","content":"{}"}}"#, "x".repeat(12_000));
        let signatures = Some(vec!["opaque-signature".repeat(1_000)]);
        let mut assistant = message("assistant", String::new());
        let mut call = tool_call("call_1", arguments.clone());
        call.thought_signatures = signatures.clone();
        assistant.tool_calls = Some(vec![call]);

        let mut messages = vec![assistant];
        assert!(fit_messages_to_context_limit(&mut messages, 512).is_err());

        let retained_call = messages[0].tool_calls.as_ref().unwrap().first().unwrap();
        assert_eq!(retained_call.function.arguments, arguments);
        assert_eq!(retained_call.thought_signatures, signatures);
    }

    #[test]
    fn compaction_discards_only_complete_older_turns() {
        let first_user = message("user", "first task ".repeat(500));
        let mut first_assistant = message("assistant", String::new());
        first_assistant.tool_calls = Some(vec![tool_call("call_1", "{}".to_string())]);
        let mut first_tool = message("tool", "first observation ".repeat(800));
        first_tool.tool_call_id = Some("call_1".to_string());
        let latest_user = message("user", "latest task ".repeat(800));

        let compacted = compact_dialog_messages_for_context(
            vec![first_user, first_assistant, first_tool, latest_user.clone()],
            ContextBudget {
                context_window_tokens: 2_048,
                response_reserve_tokens: 1_024,
            },
        );

        assert_eq!(
            compacted.first().map(|item| item.role.as_str()),
            Some("system")
        );
        assert_eq!(
            compacted.last().and_then(|item| item.content.as_ref()),
            latest_user.content.as_ref()
        );
        assert!(!compacted
            .iter()
            .any(|item| item.tool_call_id.as_deref() == Some("call_1")));
    }

    #[test]
    fn unknown_openai_compatible_endpoint_does_not_replay_reasoning_or_signatures() {
        let channel = AiChannel {
            id: "channel".to_string(),
            name: "Custom".to_string(),
            provider: "openai".to_string(),
            endpoint: Some("https://llm.example.invalid/v1".to_string()),
            api_key: None,
            proxy_id: None,
            is_active: true,
            synced: true,
            updated_at: "now".to_string(),
        };
        let model = AiModel {
            id: "model".to_string(),
            name: "custom-model".to_string(),
            channel_id: channel.id.clone(),
            context_window: None,
            response_reserve: None,
            enabled: true,
            synced: true,
            updated_at: "now".to_string(),
        };

        let capabilities = ProviderCapabilities::for_channel_and_model(&channel, &model);

        assert!(!capabilities.reasoning_round_trip);
        assert!(!capabilities.preserve_thought_signatures);
    }

    #[test]
    fn anthropic_groups_one_assistant_turns_tool_results_into_one_message() {
        let mut assistant = message("assistant", String::new());
        assistant.tool_calls = Some(vec![
            ToolCall {
                id: "call_1".to_string(),
                tool_type: "function".to_string(),
                function: FunctionCall {
                    name: "read_file".to_string(),
                    arguments: "{}".to_string(),
                },
                thought_signatures: None,
            },
            ToolCall {
                id: "call_2".to_string(),
                tool_type: "function".to_string(),
                function: FunctionCall {
                    name: "read_file".to_string(),
                    arguments: "{}".to_string(),
                },
                thought_signatures: None,
            },
        ]);
        let mut first_result = message("tool", "first result".to_string());
        first_result.tool_call_id = Some("call_1".to_string());
        let mut second_result = message("tool", "second result".to_string());
        second_result.tool_call_id = Some("call_2".to_string());

        let encoded = to_genai_messages(
            vec![assistant, first_result, second_result],
            ProviderCapabilities {
                reasoning_round_trip: false,
                preserve_thought_signatures: false,
                tool_result_encoding: ToolResultEncoding::GroupedUserMessage,
                requires_tool_call_ids: true,
                openai_compatible: false,
            },
        );

        assert_eq!(encoded.len(), 2);
    }

    #[test]
    fn compaction_threshold_fires_at_85_percent_of_effective_window() {
        // 64K window, 8K reserve → effective 56K; threshold ≈ 47.6K.
        assert!(!should_compact_context(40_000, 64_000, 8_192));
        assert!(!should_compact_context(47_000, 64_000, 8_192));
        assert!(should_compact_context(48_000, 64_000, 8_192));
        assert!(should_compact_context(60_000, 64_000, 8_192));
        // A missing reserve still leaves a sane floor.
        assert!(should_compact_context(60_000, 64_000, 0));
        // Tiny windows still trigger before overflow.
        assert!(should_compact_context(1_800, 2_048, 256));
    }

    #[test]
    fn compaction_output_budget_is_bounded() {
        assert_eq!(compaction_output_budget(8_192), 4_096); // 0.8×8192 > cap
        assert_eq!(compaction_output_budget(2_048), 1_638); // 0.8×2048
        assert_eq!(compaction_output_budget(256), 256); // floor
        assert_eq!(compaction_output_budget(0), 256); // floor even at 0
    }

    #[test]
    fn extract_compaction_summary_prefers_summary_block() {
        let raw = "<analysis>\nsome scratchpad\n</analysis>\n<summary>\n1. Primary Request\n2. Key Concepts\n</summary>\n";
        let summary = extract_compaction_summary(raw);
        assert_eq!(summary, "1. Primary Request\n2. Key Concepts");
    }

    #[test]
    fn extract_compaction_summary_falls_back_without_tags() {
        let raw = "plain summary text without tags";
        assert_eq!(extract_compaction_summary(raw), raw);
        let with_analysis = "<analysis>scratch</analysis>\nkept text";
        assert_eq!(extract_compaction_summary(with_analysis), "kept text");
    }

    #[test]
    fn compaction_prompt_carries_previous_summary() {
        let prompt = build_compaction_prompt(None);
        assert!(prompt.contains("Primary Request and Intent"));
        assert!(prompt.contains("CRITICAL: Respond with TEXT ONLY"));
        assert!(prompt.contains("Do NOT call any tools"));
        assert!(!prompt.contains("<previous_summary>"));

        let with_previous = build_compaction_prompt(Some("OLD SUMMARY"));
        assert!(with_previous.contains("<previous_summary>"));
        assert!(with_previous.contains("OLD SUMMARY"));
        assert!(with_previous.contains("Never discard entries from previous compactions"));
    }

    #[test]
    fn serialize_messages_renders_roles_and_tool_pairs() {
        let mut assistant = message("assistant", String::new());
        assistant.tool_calls = Some(vec![tool_call("call_1", "{\"path\":\"/a\"}".to_string())]);
        assistant.reasoning_content = Some("thinking...".to_string());
        let mut result = message("tool", "observed output".to_string());
        result.tool_call_id = Some("call_1".to_string());
        let user = message("user", "please fix it".to_string());

        let text = serialize_messages_for_compaction(&[user, assistant, result]);
        assert!(text.contains("[User]: please fix it"));
        assert!(text.contains("[Assistant reasoning]: thinking..."));
        assert!(text.contains("[Assistant tool call]: write_file({\"path\":\"/a\"})"));
        assert!(text.contains("[Tool result (call_1)]: observed output"));
    }

    #[test]
    fn select_compaction_split_keeps_recent_turns_and_never_splits_tool_pairs() {
        let mut tool_assistant = message("assistant", String::new());
        tool_assistant.tool_calls =
            Some(vec![tool_call("call_1", "{}".to_string())]);
        let mut tool_result = message("tool", "result A".to_string());
        tool_result.tool_call_id = Some("call_1".to_string());

        let history = vec![
            message("user", "old task".to_string()),
            message("assistant", "did old work".to_string()),
            message("user", "new task".to_string()),
            tool_assistant,
            tool_result,
            message("assistant", "done".to_string()),
        ];

        // Tiny keep budget: only the final assistant message survives verbatim.
        let (to_summarize, to_keep) = select_compaction_split(&history, 8);
        assert!(!to_summarize.is_empty());
        assert!(to_keep.iter().any(|m| m.content.as_deref() == Some("done")));
        // Tool result must not appear in to_keep without its call (pairs stay whole).
        let kept_tool_results: Vec<&str> = to_keep
            .iter()
            .filter(|m| m.role == "tool")
            .filter_map(|m| m.content.as_deref())
            .collect();
        let kept_tool_calls = to_keep
            .iter()
            .filter(|m| m.role == "assistant" && m.tool_calls.is_some())
            .count();
        assert_eq!(kept_tool_results.len(), kept_tool_calls);

        // Generous budget keeps everything verbatim → nothing to summarize.
        let (to_summarize_all, to_keep_all) = select_compaction_split(&history, 1_000_000);
        assert!(to_summarize_all.is_empty());
        assert_eq!(to_keep_all.len(), history.len());
    }
}
