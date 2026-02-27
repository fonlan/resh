import { create } from "zustand"
import { ChatMessage, AISession, ToolCall } from "../types/ai"
import { aiService } from "../services/aiService"

interface AIState {
  sessions: AISession[]
  activeSessionId: string | null
  activeSessionIdByServer: Record<string, string | null>
  activeSessionIdBySshSession: Record<string, string | null>
  messages: Record<string, ChatMessage[]>
  isLoading: boolean
  isGenerating: Record<string, boolean>
  pendingToolCalls: Record<string, ToolCall[] | null>
  stoppedSessions: Set<string> // Track sessions that were manually stopped

  loadSessions: (serverId: string) => Promise<void>
  createSession: (
    serverId: string,
    modelId?: string,
    sshSessionId?: string,
  ) => Promise<string>
  selectSession: (
    sessionId: string | null,
    serverId?: string,
    sshSessionId?: string,
  ) => Promise<void>
  addMessage: (sessionId: string, message: ChatMessage) => void
  newAssistantMessage: (sessionId: string, modelId?: string) => void
  appendResponse: (sessionId: string, content: string) => void
  appendReasoning: (sessionId: string, reasoning: string) => void
  appendToolCalls: (sessionId: string, toolCalls: ToolCall[]) => void
  setLoading: (loading: boolean) => void
  setGenerating: (sessionId: string, generating: boolean) => void
  setPendingToolCalls: (sessionId: string, toolCalls: ToolCall[] | null) => void
  markSessionStopped: (sessionId: string) => void
  clearSessionStopped: (sessionId: string) => void
  deleteSession: (serverId: string, sessionId: string) => Promise<void>
  clearSessions: (serverId: string) => Promise<void>
  addCompleteMessage: (sessionId: string, message: ChatMessage) => void
  removeLatestAssistantMessage: (sessionId: string) => void
}

export const useAIStore = create<AIState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  activeSessionIdByServer: {},
  activeSessionIdBySshSession: {},
  messages: {},
  isLoading: false,
  isGenerating: {},
  pendingToolCalls: {},
  stoppedSessions: new Set<string>(),

  setLoading: (loading) => set({ isLoading: loading }),

  setGenerating: (sessionId, generating) =>
    set((state) => ({
      isGenerating: { ...state.isGenerating, [sessionId]: generating },
    })),

  setPendingToolCalls: (sessionId, toolCalls) =>
    set((state) => ({
      pendingToolCalls: { ...state.pendingToolCalls, [sessionId]: toolCalls },
    })),

  markSessionStopped: (sessionId) =>
    set((state) => {
      const newStoppedSessions = new Set(state.stoppedSessions)
      newStoppedSessions.add(sessionId)
      return { stoppedSessions: newStoppedSessions }
    }),

  clearSessionStopped: (sessionId) =>
    set((state) => {
      const newStoppedSessions = new Set(state.stoppedSessions)
      newStoppedSessions.delete(sessionId)
      return { stoppedSessions: newStoppedSessions }
    }),

  loadSessions: async (serverId) => {
    set({ isLoading: true })
    try {
      const sessions = await aiService.getSessions(serverId)
      set({ sessions })
    } finally {
      set({ isLoading: false })
    }
  },

  createSession: async (serverId, modelId, sshSessionId) => {
    const id = await aiService.createSession(serverId, modelId, sshSessionId)
    await get().loadSessions(serverId)
    await get().selectSession(id, serverId, sshSessionId)
    return id
  },

  selectSession: async (sessionId, serverId, sshSessionId) => {
    set((state) => ({
      activeSessionId: sessionId,
      activeSessionIdByServer: serverId
        ? { ...state.activeSessionIdByServer, [serverId]: sessionId }
        : state.activeSessionIdByServer,
      activeSessionIdBySshSession: sshSessionId
        ? { ...state.activeSessionIdBySshSession, [sshSessionId]: sessionId }
        : state.activeSessionIdBySshSession,
    }))
    if (sessionId) {
      const msgs = await aiService.getMessages(sessionId)

      // Check if this session was manually stopped - if so, don't restore pending tool calls
      const isStopped = get().stoppedSessions.has(sessionId)

      // Check for pending tool calls in history
      let pending = null
      if (!isStopped && msgs.length > 0) {
        const lastMsg = msgs[msgs.length - 1]
        if (
          lastMsg.role === "assistant" &&
          lastMsg.tool_calls &&
          lastMsg.tool_calls.length > 0
        ) {
          const hasVisibleTools = lastMsg.tool_calls.some(
            (tc) =>
              tc.function.name !== "get_terminal_output" &&
              tc.function.name !== "get_selected_terminal_output" &&
              tc.function.name !== "read_file",
          )
          if (hasVisibleTools) {
            pending = lastMsg.tool_calls
          }
        }
      }

      // Preserve only the latest unsynced user message while backend persistence is in-flight
      const currentState = get()
      const isCurrentSession = currentState.activeSessionId === sessionId
      const isGenerating = currentState.isGenerating[sessionId] ?? false
      const pendingTools = currentState.pendingToolCalls[sessionId]
      const frontendMessages = currentState.messages[sessionId]

      let finalMessages = msgs
      if (
        isCurrentSession &&
        isGenerating &&
        !pendingTools &&
        frontendMessages &&
        frontendMessages.length > msgs.length
      ) {
        const latestFrontendMessage =
          frontendMessages[frontendMessages.length - 1]
        const latestBackendMessage = msgs[msgs.length - 1]
        const latestAlreadyPersisted =
          !!latestBackendMessage &&
          latestBackendMessage.role === "user" &&
          latestFrontendMessage.role === "user" &&
          (latestBackendMessage.content || "") ===
            (latestFrontendMessage.content || "")

        if (latestFrontendMessage.role === "user" && !latestAlreadyPersisted) {
          finalMessages = [...msgs, latestFrontendMessage]
        }
      }

      set((state) => ({
        messages: { ...state.messages, [sessionId]: finalMessages },
        pendingToolCalls: { ...state.pendingToolCalls, [sessionId]: pending },
      }))
    }
  },

  addMessage: (sessionId, message) => {
    set((state) => {
      const current = state.messages[sessionId] || []
      return {
        messages: { ...state.messages, [sessionId]: [...current, message] },
      }
    })
  },

  newAssistantMessage: (sessionId, modelId) => {
    set((state) => {
      const current = state.messages[sessionId] || []
      // Don't add a new message if the last one is already an empty assistant message
      const last = current[current.length - 1]
      if (
        last &&
        last.role === "assistant" &&
        !last.content &&
        !last.tool_calls
      ) {
        return state
      }
      return {
        messages: {
          ...state.messages,
          [sessionId]: [
            ...current,
            {
              role: "assistant",
              content: "",
              created_at: new Date().toISOString(),
              model_id: modelId,
            },
          ],
        },
      }
    })
  },

  appendResponse: (sessionId, content) => {
    set((state) => {
      const current = state.messages[sessionId] || []
      const last = current[current.length - 1]

      if (last && last.role === "assistant") {
        // Update last message
        const updated = [...current]
        updated[updated.length - 1] = {
          ...last,
          content: (last.content || "") + content,
        }
        return { messages: { ...state.messages, [sessionId]: updated } }
      } else {
        // Create new assistant message
        return {
          messages: {
            ...state.messages,
            [sessionId]: [
              ...current,
              {
                role: "assistant",
                content,
                created_at: new Date().toISOString(),
              },
            ],
          },
        }
      }
    })
  },

  appendReasoning: (sessionId, reasoning) => {
    set((state) => {
      const current = state.messages[sessionId] || []
      const last = current[current.length - 1]

      if (last && last.role === "assistant") {
        const updated = [...current]
        updated[updated.length - 1] = {
          ...last,
          reasoning_content: (last.reasoning_content || "") + reasoning,
        }
        return { messages: { ...state.messages, [sessionId]: updated } }
      } else {
        return {
          messages: {
            ...state.messages,
            [sessionId]: [
              ...current,
              {
                role: "assistant",
                content: "",
                reasoning_content: reasoning,
                created_at: new Date().toISOString(),
              },
            ],
          },
        }
      }
    })
  },

  appendToolCalls: (sessionId, toolCalls) => {
    set((state) => {
      const current = state.messages[sessionId] || []
      const last = current[current.length - 1]

      if (last && last.role === "assistant") {
        const updated = [...current]
        updated[updated.length - 1] = { ...last, tool_calls: toolCalls }
        return { messages: { ...state.messages, [sessionId]: updated } }
      } else {
        return {
          messages: {
            ...state.messages,
            [sessionId]: [
              ...current,
              {
                role: "assistant",
                content: "",
                tool_calls: toolCalls,
                created_at: new Date().toISOString(),
              },
            ],
          },
        }
      }
    })
  },

  deleteSession: async (serverId, sessionId) => {
    await aiService.deleteSession(sessionId)
    const state = get()
    if (state.activeSessionId === sessionId) {
      set({ activeSessionId: null })
    }
    if (state.activeSessionIdByServer[serverId] === sessionId) {
      set((s) => ({
        activeSessionIdByServer: {
          ...s.activeSessionIdByServer,
          [serverId]: null,
        },
      }))
    }

    // Clean up session-specific state
    set((s) => {
      const isGenerating = { ...s.isGenerating }
      const pendingToolCalls = { ...s.pendingToolCalls }
      const messages = { ...s.messages }
      const activeSessionIdBySshSession = { ...s.activeSessionIdBySshSession }
      const stoppedSessions = new Set(s.stoppedSessions)
      delete isGenerating[sessionId]
      delete pendingToolCalls[sessionId]
      delete messages[sessionId]
      Object.keys(activeSessionIdBySshSession).forEach((sshSessionId) => {
        if (activeSessionIdBySshSession[sshSessionId] === sessionId) {
          activeSessionIdBySshSession[sshSessionId] = null
        }
      })
      stoppedSessions.delete(sessionId)
      return {
        isGenerating,
        pendingToolCalls,
        messages,
        activeSessionIdBySshSession,
        stoppedSessions,
      }
    })

    await get().loadSessions(serverId)
  },

  clearSessions: async (serverId) => {
    await aiService.deleteAllSessions(serverId)
    set((state) => ({
      activeSessionIdBySshSession: Object.fromEntries(
        Object.entries(state.activeSessionIdBySshSession).map(
          ([sshSessionId, mappedSessionId]) => {
            if (
              mappedSessionId &&
              state.sessions.some((session) => session.id === mappedSessionId)
            ) {
              return [sshSessionId, null]
            }
            return [sshSessionId, mappedSessionId]
          },
        ),
      ),
      activeSessionId: null,
      sessions: [],
      activeSessionIdByServer: {
        ...state.activeSessionIdByServer,
        [serverId]: null,
      },
      isGenerating: {},
      pendingToolCalls: {},
      messages: {},
      stoppedSessions: new Set<string>(),
    }))
    await get().loadSessions(serverId)
  },

  addCompleteMessage: (sessionId: string, message: ChatMessage) => {
    set((state) => {
      const current = state.messages[sessionId] || []
      return {
        messages: { ...state.messages, [sessionId]: [...current, message] },
      }
    })
  },

  removeLatestAssistantMessage: (sessionId: string) => {
    set((state) => {
      const current = state.messages[sessionId] || []
      if (current.length === 0) {
        return state
      }

      const removeIndex = [...current]
        .map((msg, idx) => ({ msg, idx }))
        .reverse()
        .find(({ msg }) => msg.role === "assistant")?.idx

      if (removeIndex === undefined) {
        return state
      }

      const next = [...current]
      next.splice(removeIndex, 1)

      return {
        messages: { ...state.messages, [sessionId]: next },
      }
    })
  },
}))
