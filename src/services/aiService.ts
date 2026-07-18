import { invoke } from "@tauri-apps/api/core"
import {
  ChatMessage,
  AISession,
  AiToolCallEventPayload,
  ToolCall,
} from "../types/ai"

export const aiService = {
  createSession: (serverId: string, modelId?: string, sshSessionId?: string) =>
    invoke<string>("create_ai_session", { serverId, modelId, sshSessionId }),

  getSessions: (serverId: string) =>
    invoke<AISession[]>("get_ai_sessions", { serverId }),

  getMessages: (sessionId: string) =>
    invoke<ChatMessage[]>("get_ai_messages", { sessionId }),

  sendMessage: (
    sessionId: string,
    content: string,
    modelId: string,
    channelId: string,
    mode: string | undefined,
    sshSessionId: string | undefined,
    thinkingLevel: string | undefined,
    requestId: string,
  ) =>
    invoke("send_chat_message", {
      sessionId,
      content,
      modelId,
      channelId,
      mode,
      sshSessionId,
      thinkingLevel,
      requestId,
    }),

  regenerateResponse: (
    sessionId: string,
    modelId: string,
    channelId: string,
    mode: string | undefined,
    sshSessionId: string | undefined,
    thinkingLevel: string | undefined,
    requestId: string,
  ) =>
    invoke("regenerate_ai_response", {
      sessionId,
      modelId,
      channelId,
      mode,
      sshSessionId,
      thinkingLevel,
      requestId,
    }),

  cancelMessage: (sessionId: string, requestId: string) =>
    invoke("cancel_ai_chat", { sessionId, requestId }),

  executeAgentTools: (
    sessionId: string,
    modelId: string,
    channelId: string,
    mode: string | undefined,
    sshSessionId: string | undefined,
    approvalCalls: ToolCall[],
    approvalAction: "accept" | "acceptForSession" | "decline" | "cancel",
    thinkingLevel: string | undefined,
    requestId: string,
  ) => {
    const firstCall = approvalCalls[0]
    if (
      !firstCall?.approval_run_id ||
      firstCall.approval_turn_index === undefined ||
      approvalCalls.some(
        (call) =>
          !call.approval_id ||
          call.approval_run_id !== firstCall.approval_run_id ||
          call.approval_turn_index !== firstCall.approval_turn_index,
      )
    ) {
      return Promise.reject(new Error("Missing durable tool approval identity"))
    }
    return invoke("execute_agent_tools", {
      sessionId,
      modelId,
      channelId,
      mode,
      sshSessionId,
      runId: firstCall.approval_run_id,
      turnIndex: firstCall.approval_turn_index,
      toolCallIds: approvalCalls.map((call) => call.id),
      approvalIds: approvalCalls.map((call) => call.approval_id),
      approvalAction,
      thinkingLevel,
      requestId,
    })
  },

  getPendingToolApprovals: (sessionId: string) =>
    invoke<AiToolCallEventPayload | null>("get_pending_tool_approvals", { sessionId }),

  generateTitle: (sessionId: string, modelId: string, channelId: string) =>
    invoke<string>("generate_session_title", {
      sessionId,
      modelId,
      channelId,
    }),

  deleteSession: (sessionId: string) =>
    invoke<void>("delete_ai_session", { sessionId }),

  deleteAllSessions: (serverId: string) =>
    invoke<void>("delete_all_ai_sessions", { serverId }),
}
