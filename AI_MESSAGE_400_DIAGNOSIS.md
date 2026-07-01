# AI Message Assembly 400 Diagnosis

Scope: Phase 1 only. This is evidence collection and risk confirmation; no runtime code changes were made.

## Confirmed Request Path

1. `send_chat_message` inserts the new user row into `ai_messages`, then calls `run_ai_turn`.
   - `src-tauri/src/commands/ai/mod.rs:521-621`
2. `run_ai_turn` calls `load_history`, converts it with `to_genai_messages`, then calls `AiManager::stream_chat`.
   - `src-tauri/src/commands/ai/turn.rs:650-672`
3. `load_history` reads persisted rows from DB ordered by `created_at, rowid`, prepends a synthetic system message, passes a `MessagePayload` copy through `validate_and_fix_messages`, and converts it back to `ChatMessage`.
   - `src-tauri/src/commands/ai/history.rs:337-540`
4. `to_genai_messages` maps `ChatMessage` into `genai::chat::ChatMessage`, preserving assistant `reasoning_content` via `with_reasoning_content` and converting tool calls/tool responses into GenAI parts.
   - `src-tauri/src/commands/ai/history.rs:159-287`
5. `AiManager::stream_chat` creates `ChatRequest`, optionally attaches tools, selects the OpenAI-compatible adapter, enables content capture, and calls `genai_client.exec_chat_stream(...).stream`.
   - `src-tauri/src/ai/manager.rs:137-199`

So the model-bound path is:

```text
DB ai_messages
  -> load_history
  -> validate_and_fix_messages
  -> to_genai_messages
  -> AiManager::stream_chat
  -> genai exec_chat_stream / stream_chat
```

## Mutation Points

`role`
- DB rows are created as `user`, `assistant`, or `tool` in `send_chat_message`, `run_ai_turn`, and `execute_tools_and_save`.
- `load_history` adds a synthetic `system` message before persisted dialog rows.
- `to_genai_messages` treats unknown roles as `user`.
- `validate_message_sequence` reports unknown or misplaced roles but does not rewrite them.

`content`
- User content can be expanded by `enrich_user_message_with_tagged_files` before DB insert.
- DB empty `content` is converted to `None` in both `get_ai_messages` and `load_history`.
- `merge_duplicate_roles` concatenates content for consecutive non-tool same-role messages.
- `fix_tool_call_sequences` can insert dummy tool content: `Interrupted or skipped by user`.
- Stream chunks and parsed `<think>` leftovers append to `full_content`, then assistant content is persisted.
- `to_genai_messages` drops an assistant that has empty content and no reasoning after invalid tool-call sanitization.

`reasoning_content`
- DB has a nullable `reasoning_content` column.
- Stream `ReasoningChunk` events and parsed `<think>` segments append to `full_reasoning`, then it is persisted on assistant rows.
- `merge_duplicate_roles` concatenates assistant reasoning for consecutive assistant messages.
- `remove_empty_assistant_messages` and `trim_trailing_empty_assistant` consider non-empty reasoning as a valid assistant message.
- `to_genai_messages` attaches non-empty assistant reasoning with `with_reasoning_content`, including assistant messages with `tool_calls`.
- Current `AiManager::stream_chat` enables `with_capture_content(true)` only. In genai 0.6.0-beta.18, `StreamEnd.captured_content` contains text/tool calls/thought signatures, while reasoning is a separate `captured_reasoning_content` field that is only populated when reasoning capture is enabled. Therefore `captured_content` is not a reasoning fallback source as-is.

`tool_calls`
- `run_ai_turn` accumulates streamed `ToolCallChunk`s, then prefers `end.captured_content.tool_calls()` when present and falls back to accumulated chunks.
- `normalize_streamed_tool_calls` drops malformed/orphan tool calls and normalizes empty args to `{}`.
- Assistant `tool_calls` are JSON persisted in `run_ai_turn`.
- `validate_and_fix_messages` may insert missing dummy tool responses but does not directly rewrite assistant `tool_calls`.
- `to_genai_messages` drops persisted tool calls with empty function names or invalid JSON arguments; if none remain and no content/reasoning remains, it drops the assistant message.
- Frontend `appendToolCalls` mutates the current in-memory last assistant message, or creates a new assistant message if the last visible message is not assistant.

`tool_call_id`
- `execute_tools_and_save` persists tool rows with `tool_call_id = call.id` and emits a matching frontend tool message.
- `validate_tool_call_ids` clears tool IDs that do not exist in any assistant `tool_calls`.
- `fix_tool_call_sequences` removes orphan tool messages and inserts dummy tool messages with missing call IDs.
- `to_genai_messages` drops persisted tool messages with empty or unmatched `tool_call_id` relative to the last assistant tool call set.

## DeepSeek Thinking Tool-Call Minimum Legal Sequence

DeepSeek official thinking-mode docs say that when an assistant performs a tool call, that assistant turn's `reasoning_content` must be fully passed back in subsequent requests; otherwise the API returns 400. Source: https://api-docs.deepseek.com/guides/thinking_mode

Minimal OpenAI-compatible sequence for one tool call:

```json
[
  { "role": "system", "content": "..." },
  { "role": "user", "content": "How is the weather tomorrow?" },
  {
    "role": "assistant",
    "content": "",
    "reasoning_content": "I need the date, then weather.",
    "tool_calls": [
      {
        "id": "call_1",
        "type": "function",
        "function": { "name": "get_date", "arguments": "{}" }
      }
    ]
  },
  { "role": "tool", "tool_call_id": "call_1", "content": "2026-04-20" },
  {
    "role": "assistant",
    "content": "Tomorrow is 2026-04-20...",
    "reasoning_content": "The tool returned the date; now I can answer."
  },
  { "role": "user", "content": "What about Guangzhou?" }
]
```

For N tool calls, the assistant `tool_calls` array must be followed by N consecutive `tool` messages whose `tool_call_id`s exactly match those call IDs. In DeepSeek thinking mode, the assistant row that has `tool_calls` must also round-trip its complete `reasoning_content` in later requests.

## Confirmed Risks

1. Reasoning persistence for tool-call turns is mostly present, but has one fallback gap.
   - Happy path: `ReasoningChunk` or parsed `<think>` content is saved as `full_reasoning`, then `to_genai_messages` round-trips it with tool calls. This satisfies DeepSeek for streams that expose reasoning chunks.
   - Gap: `end.captured_content` cannot recover reasoning because genai stores reasoning separately as `captured_reasoning_content`, and the current options do not enable `capture_reasoning_content`. If a provider/backend only makes complete reasoning available via end capture, Resh would persist empty reasoning on the tool-call assistant row. For DeepSeek thinking mode, the next request can then 400.

2. Mixed safe + confirmation tool calls can make `execute_agent_tools` unable to find the assistant.
   - `run_ai_turn` auto-executes read-only tools first and writes their `tool` messages to DB, then emits/returns only the confirmation-required calls.
   - `execute_agent_tools` reloads history and requires `history.last()` to be the assistant with `tool_calls`.
   - In a mixed call set, the persisted tail is already a `tool` message, not the assistant. `validate_and_fix_messages` can also insert dummy responses for the unexecuted confirmation calls, leaving the tail as `tool` as well.
   - Result: user confirmation can fail with `Last message was not an assistant tool call`; the follow-up model request may also contain dummy/skipped tool results instead of the real confirmed result.

3. Duplicate `ai-tool-call` emits are real.
   - `run_ai_turn` emits `ai-tool-call-*` before returning `Ok(Some(confirm_calls))`.
   - The command wrapper (`send_chat_message`, `regenerate_ai_response`, or `execute_agent_tools`) emits the same event again when it receives `Some(calls)`.
   - For confirm-only calls this mostly resets/duplicates frontend pending state. For mixed calls, the earlier auto-executed tool message can arrive before the duplicate tool-call event; frontend `appendToolCalls` may then create a synthetic assistant message after a tool row, diverging UI state from DB state. This is a flow/state bug, and it amplifies the mixed-tool failure above.

4. Provider/model-aware stripping is not implemented in the request path.
   - Current `to_genai_messages` sends any persisted non-empty `reasoning_content` to the OpenAI-compatible adapter for all channels/models.
   - This is necessary for DeepSeek thinking tool-call round-trip, but can be risky for providers/models that reject input `reasoning_content`.
   - Phase 2 should avoid a global strip: DeepSeek-like thinking providers need preservation, explicitly incompatible providers need removal.

## Phase 2 Fix Direction

Smallest useful changes, in order:

1. Capture reasoning fallback from stream end.
   - Enable genai `capture_reasoning_content` or otherwise use `StreamEnd.captured_reasoning_content` as a fallback when `full_reasoning` is empty.
   - Do not rely on `captured_content.first_reasoning_content()`; current genai `captured_content` does not carry reasoning content.

2. Fix mixed tool execution before changing validator behavior.
   - Either execute no tools until all required confirmations are resolved, or make `execute_agent_tools` locate the latest assistant with pending requested `tool_call_ids` instead of requiring `history.last()`.
   - Avoid inserting dummy tool responses for still-pending confirmation calls.

3. Remove the duplicate emit source.
   - Prefer one owner for `ai-tool-call-*`: either `run_ai_turn` emits and wrappers only return, or `run_ai_turn` returns and wrappers emit. One event is enough.

4. Add provider/model-aware reasoning serialization.
   - Preserve reasoning for DeepSeek thinking/tool-call turns.
   - Strip only for providers/models known to reject input `reasoning_content`.
