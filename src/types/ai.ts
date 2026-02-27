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
