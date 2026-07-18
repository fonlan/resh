export interface ChatMessage {
  role: "user" | "assistant" | "system" | "tool"
  content: string | null
  reasoning_content?: string | null
  tool_calls?: ToolCall[]
  tool_call_id?: string
  created_at?: string
  model_id?: string
}

export interface ToolCall {
  id: string
  type: string
  function: {
    name: string
    arguments: string
  }
  /** Backend-authoritative approval policy attached only while the call awaits user action. */
  approval_policy?: "Auto" | "Countdown" | "AlwaysAsk"
  /** Durable id for the persisted tool-invocation item. */
  approval_item_id?: string
  approval_id?: string
  /** Request identity of the persisted run; used to cancel the run rather than only its tools. */
  approval_request_id?: string
  approval_run_id?: string
  approval_turn_index?: number
  /** Lifecycle state projected from the backend, never inferred from message order. */
  approval_status?: "awaitingApproval" | "resolving"
}

export interface AISession {
  id: string
  title: string
  createdAt: string
  modelId: string | null
  sshSessionId?: string | null
}

/** Identifies one AI run for a session (send / regenerate / tool continue). */
export type AiRequestId = string

export type AiRunStatus = "running" | "awaitingApproval"

/** Latest non-terminal backend run for one session, used to restore UI after session switches. */
export interface AiRunSnapshot {
  run_id: string
  request_id: string
  status: AiRunStatus
  turn_index: number
}

/** Stream text chunk (response / reasoning). Matches Rust snake_case serde. */
export interface AiStreamTextPayload {
  request_id: string
  content: string
}

export type ToolApprovalPolicy = "Auto" | "Countdown" | "AlwaysAsk"

export interface AiToolCallEventPayload {
  request_id: string
  run_id: string
  turn_index: number
  /** Persisted item ids aligned with tool_calls. */
  item_ids: string[]
  status: "awaitingApproval"
  tool_calls: ToolCall[]
  approval_ids: string[]
  /** Backend-authoritative policy for each pending approval, aligned with tool_calls. */
  approval_policies: ToolApprovalPolicy[]
}

export interface AiMessageBatchPayload {
  request_id: string
  /** Explicit background task update; independent from the active Agent request. */
  background_task_id?: string | null
  messages: ChatMessage[]
}

export interface AiErrorEventPayload {
  request_id: string
  error: string
}
