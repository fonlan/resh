use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{Emitter, Window};

use super::types::{AiReasoningEndPayload, AiStreamTextPayload, FunctionCall, ToolCall};

pub(super) const STREAM_EMIT_INTERVAL: Duration = Duration::from_millis(20);
pub(super) const STREAM_EMIT_MAX_BUFFER_LEN: usize = 1024;
const THINK_OPEN_TAG: &str = "<think>";
const THINK_CLOSE_TAG: &str = "</think>";

pub(super) fn extract_timeout(arguments: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()?
        .get("timeoutSeconds")
        .and_then(|v| v.as_u64())
}

pub(super) fn extract_required_timeout_seconds(
    arguments: &serde_json::Value,
    tool_name: &str,
) -> Result<u64, String> {
    match arguments.get("timeoutSeconds") {
        Some(value) => match value.as_u64() {
            Some(timeout) if timeout > 0 => Ok(timeout),
            _ => Err(format!(
                "Error: Invalid 'timeoutSeconds' argument for {}. Provide a positive integer timeout in seconds with enough safety margin for the command.",
                tool_name
            )),
        },
        None => Err(format!(
            "Error: Missing required 'timeoutSeconds' argument for {}. Estimate the command runtime and provide a timeout with enough safety margin.",
            tool_name
        )),
    }
}

pub(super) fn accumulate_streamed_tool_call_chunk(
    accumulated_tool_calls: &mut Vec<ToolCall>,
    tool_call_id_aliases: &mut HashMap<String, String>,
    call_id: &str,
    fn_name: &str,
    args: &str,
) {
    let effective_id = tool_call_id_aliases
        .get(call_id)
        .cloned()
        .unwrap_or_else(|| call_id.to_string());

    if let Some(existing) = accumulated_tool_calls
        .iter_mut()
        .find(|tc| tc.id == effective_id)
    {
        existing.function.arguments.push_str(args);
        if existing.function.name.is_empty() && !fn_name.is_empty() {
            existing.function.name = fn_name.to_string();
        }
        return;
    }

    if fn_name.is_empty() && args.is_empty() {
        return;
    }

    // Some providers emit the function name and arguments under different call ids.
    // Route nameless argument chunks to the most recent named call and remember the alias.
    if fn_name.is_empty() && !args.is_empty() {
        if let Some(existing) = accumulated_tool_calls
            .iter_mut()
            .rev()
            .find(|tc| !tc.function.name.is_empty())
        {
            existing.function.arguments.push_str(args);
            tool_call_id_aliases.insert(call_id.to_string(), existing.id.clone());
            return;
        }
    }

    accumulated_tool_calls.push(ToolCall {
        id: effective_id.clone(),
        tool_type: "function".to_string(),
        function: FunctionCall {
            name: fn_name.to_string(),
            arguments: args.to_string(),
        },
        thought_signatures: None,
    });
    tool_call_id_aliases.insert(call_id.to_string(), effective_id);
}

pub(super) fn normalize_streamed_tool_calls(tool_calls: Vec<ToolCall>) -> Vec<ToolCall> {
    let mut normalized: Vec<ToolCall> = Vec::with_capacity(tool_calls.len());

    for mut call in tool_calls {
        let has_name = !call.function.name.trim().is_empty();
        let has_args = !call.function.arguments.trim().is_empty();

        if has_name {
            if !has_args {
                tracing::warn!(
                    "[AI] Empty arguments for streamed tool call id={}, name={}; normalizing to empty object",
                    call.id,
                    call.function.name
                );
                call.function.arguments = "{}".to_string();
            }
            normalized.push(call);
            continue;
        }

        if !has_args {
            tracing::debug!(
                "[AI] Dropping empty streamed tool-call fragment: id={}",
                call.id
            );
            continue;
        }

        if let Some(existing) = normalized
            .iter_mut()
            .rev()
            .find(|tc| !tc.function.name.trim().is_empty())
        {
            tracing::warn!(
                "[AI] Merging nameless streamed tool-call fragment id={} into id={}",
                call.id,
                existing.id
            );
            existing
                .function
                .arguments
                .push_str(&call.function.arguments);
        } else {
            tracing::warn!(
                "[AI] Dropping nameless streamed tool-call id={} because no named call exists",
                call.id
            );
        }
    }

    normalized
}

fn canonical_tool_call_id(id: &str, aliases: &HashMap<String, String>) -> String {
    let mut canonical = id;
    let mut seen = std::collections::HashSet::new();
    while let Some(next) = aliases.get(canonical) {
        if !seen.insert(canonical) {
            break;
        }
        canonical = next;
    }
    canonical.to_string()
}

fn choose_more_complete_arguments(streamed: &str, captured: &str) -> String {
    if captured.trim().is_empty() {
        return streamed.to_string();
    }
    let streamed_valid = parse_tool_call_arguments(streamed).is_ok();
    let captured_valid = parse_tool_call_arguments(captured).is_ok();
    match (streamed_valid, captured_valid) {
        (true, false) => streamed.to_string(),
        (false, true) => captured.to_string(),
        // If both values are usable (or both are partial), retaining the longer representation
        // avoids replacing a complete streamed JSON object with a shortened captured snapshot.
        _ if streamed.len() >= captured.len() => streamed.to_string(),
        _ => captured.to_string(),
    }
}

pub(super) fn merge_captured_and_streamed_tool_calls(
    streamed: Vec<ToolCall>,
    captured: Vec<ToolCall>,
    tool_call_id_aliases: &HashMap<String, String>,
) -> Vec<ToolCall> {
    let mut merged = streamed;

    for mut captured_call in captured {
        if captured_call.id.trim().is_empty() {
            tracing::warn!("[AI] Dropping captured tool call without a stable id");
            continue;
        }
        captured_call.id = canonical_tool_call_id(&captured_call.id, tool_call_id_aliases);

        if let Some(streamed_call) = merged.iter_mut().find(|call| call.id == captured_call.id) {
            if !captured_call.function.name.trim().is_empty() {
                streamed_call.function.name = captured_call.function.name;
            }
            streamed_call.function.arguments = choose_more_complete_arguments(
                &streamed_call.function.arguments,
                &captured_call.function.arguments,
            );
            if captured_call
                .thought_signatures
                .as_ref()
                .map(|signatures| !signatures.is_empty())
                .unwrap_or(false)
            {
                streamed_call.thought_signatures = captured_call.thought_signatures;
            }
        } else {
            merged.push(captured_call);
        }
    }

    merged
}

pub(super) fn parse_tool_call_arguments(arguments: &str) -> Result<serde_json::Value, String> {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::json!({}));
    }

    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) if value.is_object() => Ok(value),
        Ok(serde_json::Value::String(s)) if s.trim().is_empty() => Ok(serde_json::json!({})),
        Ok(value) => Err(format!("non-object JSON ({})", value)),
        Err(e) => Err(e.to_string()),
    }
}

fn trailing_tag_prefix_len(buffer: &str, tag: &str) -> usize {
    if buffer.is_empty() || tag.is_empty() {
        return 0;
    }

    let max_len = buffer.len().min(tag.len().saturating_sub(1));
    if max_len == 0 {
        return 0;
    }

    let mut char_boundary_prefix_lens: Vec<usize> = tag
        .char_indices()
        .skip(1)
        .map(|(idx, _)| idx)
        .filter(|&idx| idx <= max_len)
        .collect();
    char_boundary_prefix_lens.reverse();

    for len in char_boundary_prefix_lens {
        if buffer.ends_with(&tag[..len]) {
            return len;
        }
    }

    0
}

pub(super) fn extract_think_segments(
    parser_buffer: &mut String,
    in_think_block: &mut bool,
) -> Vec<(bool, String)> {
    let mut segments = Vec::new();

    loop {
        if *in_think_block {
            if let Some(close_idx) = parser_buffer.find(THINK_CLOSE_TAG) {
                if close_idx > 0 {
                    segments.push((true, parser_buffer[..close_idx].to_string()));
                }
                parser_buffer.drain(..close_idx + THINK_CLOSE_TAG.len());
                *in_think_block = false;
                continue;
            }

            let hold_len = trailing_tag_prefix_len(parser_buffer, THINK_CLOSE_TAG);
            let emit_len = parser_buffer.len().saturating_sub(hold_len);
            if emit_len == 0 {
                break;
            }

            segments.push((true, parser_buffer[..emit_len].to_string()));
            parser_buffer.drain(..emit_len);
            break;
        }

        if let Some(open_idx) = parser_buffer.find(THINK_OPEN_TAG) {
            if open_idx > 0 {
                segments.push((false, parser_buffer[..open_idx].to_string()));
            }
            parser_buffer.drain(..open_idx + THINK_OPEN_TAG.len());
            *in_think_block = true;
            continue;
        }

        let hold_len = trailing_tag_prefix_len(parser_buffer, THINK_OPEN_TAG);
        let emit_len = parser_buffer.len().saturating_sub(hold_len);
        if emit_len == 0 {
            break;
        }

        segments.push((false, parser_buffer[..emit_len].to_string()));
        parser_buffer.drain(..emit_len);
        break;
    }

    segments
}

pub(super) fn append_response_stream_text(
    window: &Window,
    response_event: &str,
    reasoning_event: &str,
    reasoning_end_event: &str,
    request_id: &str,
    text: &str,
    full_content: &mut String,
    response_emit_buffer: &mut String,
    reasoning_emit_buffer: &mut String,
    has_pending_reasoning: &mut bool,
    last_emit_at: &mut Instant,
) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    if *has_pending_reasoning {
        flush_reasoning_buffer(window, reasoning_event, request_id, reasoning_emit_buffer)?;
        window
            .emit(
                reasoning_end_event,
                AiReasoningEndPayload {
                    request_id: request_id.to_string(),
                    status: "end".to_string(),
                },
            )
            .map_err(|e| e.to_string())?;
        *has_pending_reasoning = false;
        *last_emit_at = Instant::now();
    }

    full_content.push_str(text);
    response_emit_buffer.push_str(text);

    if response_emit_buffer.len() >= STREAM_EMIT_MAX_BUFFER_LEN
        || last_emit_at.elapsed() >= STREAM_EMIT_INTERVAL
    {
        flush_response_buffer(window, response_event, request_id, response_emit_buffer)?;
        *last_emit_at = Instant::now();
    }

    Ok(())
}

pub(super) fn append_reasoning_stream_text(
    window: &Window,
    reasoning_event: &str,
    request_id: &str,
    text: &str,
    full_reasoning: &mut String,
    reasoning_emit_buffer: &mut String,
    has_pending_reasoning: &mut bool,
    last_emit_at: &mut Instant,
) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    *has_pending_reasoning = true;
    full_reasoning.push_str(text);
    reasoning_emit_buffer.push_str(text);

    if reasoning_emit_buffer.len() >= STREAM_EMIT_MAX_BUFFER_LEN
        || last_emit_at.elapsed() >= STREAM_EMIT_INTERVAL
    {
        flush_reasoning_buffer(window, reasoning_event, request_id, reasoning_emit_buffer)?;
        *last_emit_at = Instant::now();
    }

    Ok(())
}

pub(super) fn flush_think_parser_remainder(
    window: &Window,
    response_event: &str,
    reasoning_event: &str,
    reasoning_end_event: &str,
    request_id: &str,
    parser_buffer: &mut String,
    in_think_block: bool,
    full_content: &mut String,
    full_reasoning: &mut String,
    response_emit_buffer: &mut String,
    reasoning_emit_buffer: &mut String,
    has_pending_reasoning: &mut bool,
    last_emit_at: &mut Instant,
) -> Result<(), String> {
    if parser_buffer.is_empty() {
        return Ok(());
    }

    let remaining = std::mem::take(parser_buffer);
    if in_think_block {
        append_reasoning_stream_text(
            window,
            reasoning_event,
            request_id,
            &remaining,
            full_reasoning,
            reasoning_emit_buffer,
            has_pending_reasoning,
            last_emit_at,
        )
    } else {
        append_response_stream_text(
            window,
            response_event,
            reasoning_event,
            reasoning_end_event,
            request_id,
            &remaining,
            full_content,
            response_emit_buffer,
            reasoning_emit_buffer,
            has_pending_reasoning,
            last_emit_at,
        )
    }
}

pub(super) fn flush_response_buffer(
    window: &Window,
    response_event: &str,
    request_id: &str,
    buffer: &mut String,
) -> Result<(), String> {
    if buffer.is_empty() {
        return Ok(());
    }

    let content = std::mem::take(buffer);
    window
        .emit(
            response_event,
            AiStreamTextPayload {
                request_id: request_id.to_string(),
                content,
            },
        )
        .map_err(|e| e.to_string())
}

pub(super) fn flush_reasoning_buffer(
    window: &Window,
    reasoning_event: &str,
    request_id: &str,
    buffer: &mut String,
) -> Result<(), String> {
    if buffer.is_empty() {
        return Ok(());
    }

    let content = std::mem::take(buffer);
    window
        .emit(
            reasoning_event,
            AiStreamTextPayload {
                request_id: request_id.to_string(),
                content,
            },
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::merge_captured_and_streamed_tool_calls;
    use crate::commands::ai::types::{FunctionCall, ToolCall};
    use std::collections::HashMap;

    fn tool_call(id: &str, name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            tool_type: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: arguments.to_string(),
            },
            thought_signatures: None,
        }
    }

    #[test]
    fn captured_partial_call_does_not_replace_complete_streamed_arguments() {
        let merged = merge_captured_and_streamed_tool_calls(
            vec![tool_call(
                "call_1",
                "write_file",
                r#"{"path":"/tmp/a","content":"complete"}"#,
            )],
            vec![tool_call("call_1", "write_file", "{}")],
            &HashMap::new(),
        );

        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].function.arguments,
            r#"{"path":"/tmp/a","content":"complete"}"#
        );
    }

    #[test]
    fn captured_alias_merges_into_streamed_stable_id_without_dropping_other_calls() {
        let mut aliases = HashMap::new();
        aliases.insert("captured_alias".to_string(), "call_1".to_string());
        let merged = merge_captured_and_streamed_tool_calls(
            vec![
                tool_call("call_1", "read_file", r#"{"path":"/tmp/a"}"#),
                tool_call("call_2", "read_file", r#"{"path":"/tmp/b"}"#),
            ],
            vec![tool_call(
                "captured_alias",
                "read_file",
                r#"{"path":"/tmp/a"}"#,
            )],
            &aliases,
        );

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "call_1");
        assert_eq!(merged[1].id, "call_2");
    }
}
