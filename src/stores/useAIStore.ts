import { create } from "zustand"
import {
  ChatMessage,
  AISession,
  ToolCall,
  AiRequestId,
  AiRunSnapshot,
  AiToolCallEventPayload,
} from "../types/ai"
import { aiService } from "../services/aiService"

/** Convert one backend approval snapshot into the UI's display model. */
const projectPendingToolCalls = (
  snapshot: AiToolCallEventPayload | null,
): ToolCall[] | null => {
  if (
    !snapshot ||
    snapshot.status !== "awaitingApproval" ||
    snapshot.item_ids.length !== snapshot.tool_calls.length ||
    snapshot.approval_ids.length !== snapshot.tool_calls.length ||
    snapshot.approval_policies.length !== snapshot.tool_calls.length
  ) {
    return null
  }

  return snapshot.tool_calls.map((call, index) => ({
    ...call,
    approval_item_id: snapshot.item_ids[index],
    approval_policy: snapshot.approval_policies[index],
    approval_id: snapshot.approval_ids[index],
    approval_request_id: snapshot.request_id,
    approval_run_id: snapshot.run_id,
    approval_turn_index: snapshot.turn_index,
    approval_status: snapshot.status,
  }))
}

interface AIState {
  sessions: AISession[]
  activeSessionId: string | null
  activeSessionIdByServer: Record<string, string | null>
  activeSessionIdBySshSession: Record<string, string | null>
  messages: Record<string, ChatMessage[]>
  isLoading: boolean
  isGenerating: Record<string, boolean>
  /** Current AI run id per session; used to ignore late events from older runs. */
  activeRequestId: Record<string, AiRequestId | null>
  /** Latest non-terminal lifecycle snapshot returned by the backend for each session. */
  runSnapshots: Record<string, AiRunSnapshot | null>
  pendingToolCalls: Record<string, ToolCall[] | null>
  stoppedSessions: Set<string> // Track sessions that were manually stopped

  loadSessions: (serverId: string) => Promise<void>
  createSession: (
    serverId: string,
    modelId?: string,
    sshSessionId?: string,
    sessionScopeId?: string,
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
  /** Begin a request-scoped run: sets activeRequestId + generating, clears stopped. */
  startRun: (sessionId: string, requestId: AiRequestId) => void
  /**
   * End a matching run: clear generating + activeRequestId.
   * Returns false if requestId is not the session's active run.
   */
  finishRun: (sessionId: string, requestId: AiRequestId) => boolean
  /**
   * Local cancel: mark stopped, clear pending tools / generating / activeRequestId
   * when requestId matches (or when requestId is omitted and any run is active).
   */
  cancelRunLocally: (
    sessionId: string,
    requestId?: AiRequestId | null,
  ) => boolean
  /** True when event requestId matches the session's active run. */
  isActiveRequest: (
    sessionId: string,
    requestId: string | null | undefined,
  ) => boolean
  /** Apply a backend snapshot without allowing an older restored run to replace a live request. */
  hydrateRunSnapshot: (sessionId: string, snapshot: AiRunSnapshot | null) => boolean
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
  activeRequestId: {},
  runSnapshots: {},
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

  startRun: (sessionId, requestId) =>
    set((state) => {
      const stoppedSessions = new Set(state.stoppedSessions)
      stoppedSessions.delete(sessionId)
      return {
        activeRequestId: {
          ...state.activeRequestId,
          [sessionId]: requestId,
        },
        runSnapshots: { ...state.runSnapshots, [sessionId]: null },
        isGenerating: { ...state.isGenerating, [sessionId]: true },
        stoppedSessions,
      }
    }),

  finishRun: (sessionId, requestId) => {
    const current = get().activeRequestId[sessionId]
    if (current !== requestId) {
      return false
    }
    set((state) => ({
      activeRequestId: { ...state.activeRequestId, [sessionId]: null },
      runSnapshots: { ...state.runSnapshots, [sessionId]: null },
      isGenerating: { ...state.isGenerating, [sessionId]: false },
    }))
    return true
  },

  cancelRunLocally: (sessionId, requestId) => {
    const current = get().activeRequestId[sessionId]
    if (
      requestId != null &&
      requestId !== "" &&
      current != null &&
      current !== requestId
    ) {
      return false
    }
    set((state) => {
      const stoppedSessions = new Set(state.stoppedSessions)
      stoppedSessions.add(sessionId)
      return {
        activeRequestId: { ...state.activeRequestId, [sessionId]: null },
        runSnapshots: { ...state.runSnapshots, [sessionId]: null },
        isGenerating: { ...state.isGenerating, [sessionId]: false },
        pendingToolCalls: { ...state.pendingToolCalls, [sessionId]: null },
        stoppedSessions,
      }
    })
    return true
  },

  isActiveRequest: (sessionId, requestId) => {
    if (requestId == null || requestId === "") {
      return false
    }
    return get().activeRequestId[sessionId] === requestId
  },

  hydrateRunSnapshot: (sessionId, snapshot) => {
    const currentRequestId = get().activeRequestId[sessionId]
    if (
      currentRequestId != null &&
      (snapshot == null || currentRequestId !== snapshot.request_id)
    ) {
      return false
    }
    if (get().stoppedSessions.has(sessionId)) {
      return false
    }
    set((state) => ({
      runSnapshots: { ...state.runSnapshots, [sessionId]: snapshot },
      activeRequestId: {
        ...state.activeRequestId,
        [sessionId]: snapshot?.status === "running" ? snapshot.request_id : null,
      },
      isGenerating: {
        ...state.isGenerating,
        [sessionId]: snapshot?.status === "running",
      },
    }))
    return true
  },

  loadSessions: async (serverId) => {
    set({ isLoading: true })
    try {
      const sessions = await aiService.getSessions(serverId)
      set({ sessions })
    } finally {
      set({ isLoading: false })
    }
  },

  createSession: async (serverId, modelId, sshSessionId, sessionScopeId) => {
    const id = await aiService.createSession(serverId, modelId, sshSessionId)
    await get().loadSessions(serverId)
    await get().selectSession(id, serverId, sessionScopeId || sshSessionId)
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
      // A load is only a snapshot. If a stream/lifecycle event changes this session while
      // the three requests are in flight, its newer projection wins over this stale result.
      const projectionAtLoad = {
        activeRequestId: get().activeRequestId[sessionId] ?? null,
        pendingToolCalls: get().pendingToolCalls[sessionId] ?? null,
        messages: get().messages[sessionId],
        stopped: get().stoppedSessions.has(sessionId),
      }
      const [msgs, runSnapshot, pendingSnapshot] = await Promise.all([
        aiService.getMessages(sessionId),
        aiService.getRunSnapshot(sessionId),
        aiService.getPendingToolApprovals(sessionId),
      ])

      const stateAfterLoad = get()
      if (
        stateAfterLoad.activeSessionId !== sessionId ||
        (stateAfterLoad.activeRequestId[sessionId] ?? null) !==
          projectionAtLoad.activeRequestId ||
        (stateAfterLoad.pendingToolCalls[sessionId] ?? null) !==
          projectionAtLoad.pendingToolCalls ||
        stateAfterLoad.messages[sessionId] !== projectionAtLoad.messages ||
        stateAfterLoad.stoppedSessions.has(sessionId) !== projectionAtLoad.stopped
      ) {
        return
      }

      // A local stop wins until the backend's cancellation reaches a terminal state.
      const isStopped = get().stoppedSessions.has(sessionId)

      // Pending approvals are restored atomically with the session projection from
      // ai_tool_invocations, never inferred from message order.
      const pending = projectPendingToolCalls(pendingSnapshot)

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
      if (!isStopped) {
        get().hydrateRunSnapshot(sessionId, runSnapshot)
      }
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
      const activeRequestId = { ...s.activeRequestId }
      const runSnapshots = { ...s.runSnapshots }
      const pendingToolCalls = { ...s.pendingToolCalls }
      const messages = { ...s.messages }
      const activeSessionIdBySshSession = { ...s.activeSessionIdBySshSession }
      const stoppedSessions = new Set(s.stoppedSessions)
      delete isGenerating[sessionId]
      delete activeRequestId[sessionId]
      delete runSnapshots[sessionId]
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
        activeRequestId,
        runSnapshots,
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
      activeRequestId: {},
      runSnapshots: {},
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
