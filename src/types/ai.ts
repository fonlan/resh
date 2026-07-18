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
  approval_id?: string
  approval_run_id?: string
  approval_turn_index?: number
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
  tool_calls: ToolCall[]
  approval_ids: string[]
  /** Backend-authoritative policy for each pending approval, aligned with tool_calls. */
  approval_policies: ToolApprovalPolicy[]
}

export interface AiMessageBatchPayload {
  request_id: string
  messages: ChatMessage[]
}

export interface AiErrorEventPayload {
  request_id: string
  error: string
}

