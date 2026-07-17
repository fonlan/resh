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

export interface AiToolCallEventPayload {
  request_id: string
  tool_calls: ToolCall[]
}

export interface AiMessageBatchPayload {
  request_id: string
  messages: ChatMessage[]
}

export interface AiErrorEventPayload {
  request_id: string
  error: string
}

