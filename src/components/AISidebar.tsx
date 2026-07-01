import React, {
  useState,
  useEffect,
  useRef,
  useMemo,
  useCallback,
  useLayoutEffect,
  useOptimistic,
} from "react"
import { useAIStore } from "../stores/useAIStore"
import { useConfig } from "../hooks/useConfig"
import { aiService } from "../services/aiService"
import { useTranslation } from "../i18n"
import { useVirtualizer } from "@tanstack/react-virtual"
import {
  X,
  Send,
  Lock,
  LockOpen,
  Plus,
  History,
  Bot,
  Clock,
  Sliders,
  Sparkles,
  Brain,
  MessageSquare,
  Trash2,
  Square,
} from "lucide-react"
import { listen } from "@tauri-apps/api/event"
import { ToolCall, ChatMessage } from "../types/ai"
import { AIThinkingLevel, EditorAIContext } from "../types"
import { ConfirmationModal } from "./ConfirmationModal"
import { CustomSelect } from "./CustomSelect"
import { EmojiText } from "./EmojiText"
import { MessageBubble } from "./ai/MessageBubble"
import { ToolConfirmation } from "./ai/ToolConfirmation"
import {
  buildMessageWithEditorContext,
  clampAiToolConfirmationCountdown,
  collectAssistantToolOutputs,
  hasSensitiveToolCall,
  HIDDEN_TOOL_CALL_NAMES,
  MAX_EDITOR_CONTEXT_CHARS,
  normalizeAiErrorMessage,
  SFTP_ENTRY_MIME_TYPE,
  SFTP_PATH_MIME_TYPE,
  type SftpDragEntry,
  VIRTUAL_MESSAGE_GAP_PX,
} from "./ai/helpers"

interface AISidebarProps {
  isOpen: boolean
  onClose: () => void
  isLocked: boolean
  onToggleLock: () => void
  onShowToast?: (
    message: string,
    type?: "success" | "error" | "info" | "warning",
    duration?: number,
  ) => void
  currentServerId?: string
  currentTabId?: string
  currentSshSessionId?: string
  editorContextByTabId?: Record<string, EditorAIContext>
  zIndex?: number
}

const EMPTY_MESSAGES: ChatMessage[] = []

interface RenderableMessage {
  msg: ChatMessage
  sourceIndex: number
  isPending: boolean
  modelName: string | null
  toolOutputsByCallId?: Record<string, string>
}

type RenderableListItem =
  | {
      kind: "message"
      key: string
      message: RenderableMessage
      messageIndex: number
    }
  | {
      kind: "pending-tools"
      key: string
    }
  | {
      kind: "typing"
      key: string
    }

type VirtualScrollBehavior = "auto" | "smooth"

const AI_THINKING_OPTIONS: { value: AIThinkingLevel; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "max", label: "Max" },
]

const DEFAULT_AI_THINKING_LEVEL: AIThinkingLevel = "off"

const getAIThinkingLevel = (value?: string): AIThinkingLevel =>
  AI_THINKING_OPTIONS.some((option) => option.value === value)
    ? (value as AIThinkingLevel)
    : DEFAULT_AI_THINKING_LEVEL

export const AISidebar: React.FC<AISidebarProps> = ({
  isOpen,
  onClose,
  isLocked,
  onToggleLock,
  onShowToast,
  currentServerId,
  currentTabId,
  currentSshSessionId,
  editorContextByTabId,
  zIndex,
}) => {
  const { t } = useTranslation()
  const { config, saveConfig } = useConfig()
  const sessions = useAIStore((state) => state.sessions)
  const activeSessionId = useAIStore((state) => state.activeSessionId)
  const activeSessionIdByServer = useAIStore(
    (state) => state.activeSessionIdByServer,
  )
  const activeSessionIdBySshSession = useAIStore(
    (state) => state.activeSessionIdBySshSession,
  )
  const activeSessionMessages = useAIStore((state) => {
    const id = state.activeSessionId
    return id ? state.messages[id] || EMPTY_MESSAGES : EMPTY_MESSAGES
  })
  const isLoading = useAIStore((state) => {
    const id = state.activeSessionId
    return id ? state.isGenerating[id] || false : false
  })
  const pendingToolCalls = useAIStore((state) => {
    const id = state.activeSessionId
    return id ? state.pendingToolCalls[id] || null : null
  })
  const loadSessions = useAIStore((state) => state.loadSessions)
  const createSession = useAIStore((state) => state.createSession)
  const selectSession = useAIStore((state) => state.selectSession)
  const addMessage = useAIStore((state) => state.addMessage)
  const newAssistantMessage = useAIStore((state) => state.newAssistantMessage)
  const appendResponse = useAIStore((state) => state.appendResponse)
  const appendReasoning = useAIStore((state) => state.appendReasoning)
  const appendToolCalls = useAIStore((state) => state.appendToolCalls)
  const setGenerating = useAIStore((state) => state.setGenerating)
  const storeSetPendingToolCalls = useAIStore(
    (state) => state.setPendingToolCalls,
  )
  const markSessionStopped = useAIStore((state) => state.markSessionStopped)
  const clearSessionStopped = useAIStore((state) => state.clearSessionStopped)
  const deleteSession = useAIStore((state) => state.deleteSession)
  const clearSessions = useAIStore((state) => state.clearSessions)
  const addCompleteMessage = useAIStore((state) => state.addCompleteMessage)
  const removeLatestAssistantMessage = useAIStore(
    (state) => state.removeLatestAssistantMessage,
  )

  const [width, setWidth] = useState(350)
  const [isResizing, setIsResizing] = useState(false)
  const [inputValue, setInputValue] = useState("")
  const [isInputDragOver, setIsInputDragOver] = useState(false)
  const [showHistory, setShowHistory] = useState(false)
  const [mode, setMode] = useState<"ask" | "agent">(
    (config?.general.aiMode as "ask" | "agent") || "ask",
  )
  const [selectedModelId, setSelectedModelId] = useState<string>("")
  const [thinkingLevel, setThinkingLevel] = useState<AIThinkingLevel>(
    getAIThinkingLevel(config?.general.aiThinkingLevel),
  )
  const [sessionToDelete, setSessionToDelete] = useState<string | null>(null)
  const [isClearingHistory, setIsClearingHistory] = useState(false)
  const [includeEditorContext, setIncludeEditorContext] = useState(false)

  const [optimisticMessages, addOptimisticMessage] = useOptimistic(
    activeSessionMessages,
    (current, message: ChatMessage) => [...current, message],
  )
  const activeEditorContext = useMemo(() => {
    if (!currentTabId || !editorContextByTabId) {
      return null
    }
    return editorContextByTabId[currentTabId] || null
  }, [currentTabId, editorContextByTabId])
  const activeEditorContextCharCount = activeEditorContext?.content.length || 0
  const isEditorContextTooLarge =
    activeEditorContextCharCount > MAX_EDITOR_CONTEXT_CHARS
  const shouldBlockEditorContextSend =
    includeEditorContext && !!activeEditorContext && isEditorContextTooLarge
  const currentSession = sessions.find((s) => s.id === activeSessionId)
  const boundSshSessionId =
    currentSession?.sshSessionId || currentSshSessionId || currentTabId
  const autoConfirmDelaySeconds = clampAiToolConfirmationCountdown(
    config?.general.aiToolConfirmationCountdown ?? 5,
  )

  const sidebarRef = useRef<HTMLDivElement>(null)
  const messagesContainerRef = useRef<HTMLDivElement>(null)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const isAtBottomRef = useRef(true)
  const responseChunkBufferRef = useRef("")
  const reasoningChunkBufferRef = useRef("")
  const streamFlushTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const lastErrorToastRef = useRef<{ message: string; at: number } | null>(null)
  const scrollPositionsRef = useRef<Record<string, number>>({})
  const pendingScrollRestoreRef = useRef<{
    key: string
    scrollTop: number
  } | null>(null)
  const isRestoringScrollRef = useRef(false)

  const showAiError = useCallback(
    (error: unknown) => {
      const normalizedError = normalizeAiErrorMessage(error)
      const fallbackMessage = t.ai.unknownError || "Unknown error"
      const detailMessage = normalizedError || fallbackMessage
      const template = t.ai.requestFailed || "AI request failed: {error}"
      const finalMessage = template.includes("{error}")
        ? template.replace("{error}", detailMessage)
        : `${template} ${detailMessage}`

      const now = Date.now()
      const previous = lastErrorToastRef.current
      if (
        previous &&
        previous.message === finalMessage &&
        now - previous.at < 1500
      ) {
        return
      }

      lastErrorToastRef.current = { message: finalMessage, at: now }
      onShowToast?.(finalMessage, "error")
    },
    [onShowToast, t],
  )

  // Load mode & model from config, with fallback to default model
  useEffect(() => {
    if (config?.general.aiMode) {
      setMode(config.general.aiMode as "ask" | "agent")
    }
    setThinkingLevel(getAIThinkingLevel(config?.general.aiThinkingLevel))
    if (config?.general.aiModelId) {
      setSelectedModelId(config.general.aiModelId)
    } else if (
      config?.aiModels &&
      config.aiModels.length > 0 &&
      !selectedModelId
    ) {
      const firstEnabledModel = config.aiModels.find((model) => {
        const channel = config.aiChannels?.find((c) => c.id === model.channelId)
        return model.enabled && channel?.isActive
      })
      if (firstEnabledModel) {
        setSelectedModelId(firstEnabledModel.id)
      }
    }
  }, [
    config?.general.aiMode,
    config?.general.aiModelId,
    config?.general.aiThinkingLevel,
    config?.aiModels,
    config?.aiChannels,
    selectedModelId,
  ])

  useEffect(() => {
    if (!activeEditorContext && includeEditorContext) {
      setIncludeEditorContext(false)
    }
  }, [activeEditorContext, includeEditorContext])
  // Load sessions when sidebar opens or server changes
  useEffect(() => {
    if (currentServerId && isOpen) {
      void loadSessions(currentServerId)
    } else if (!currentServerId) {
      void selectSession(null)
      setShowHistory(false)
    }
  }, [currentServerId, isOpen, loadSessions, selectSession])

  // Keep AI sessions isolated by SSH tab/session and restore on tab switch
  useEffect(() => {
    if (!isOpen || !currentServerId) {
      return
    }

    const fallbackSessionId = activeSessionIdByServer[currentServerId] || null
    const sessionIdFromCurrentTab = currentTabId
      ? Object.prototype.hasOwnProperty.call(
          activeSessionIdBySshSession,
          currentTabId,
        )
        ? (activeSessionIdBySshSession[currentTabId] ?? null)
        : (sessions.find((session) => session.sshSessionId === currentTabId)
            ?.id ?? null)
      : null

    const targetSessionId = currentTabId
      ? sessionIdFromCurrentTab
      : fallbackSessionId
    if (targetSessionId !== activeSessionId) {
      if (currentTabId) {
        void selectSession(
          targetSessionId,
          targetSessionId ? currentServerId : undefined,
          currentTabId,
        )
      } else {
        void selectSession(targetSessionId, currentServerId)
      }
    }
  }, [
    isOpen,
    currentServerId,
    currentTabId,
    activeSessionId,
    sessions,
    activeSessionIdByServer,
    activeSessionIdBySshSession,
    selectSession,
  ])

  const currentMessages = optimisticMessages
  const modelNameById = useMemo(() => {
    if (!config?.aiModels?.length) {
      return {} as Record<string, string>
    }

    return config.aiModels.reduce(
      (acc, model) => {
        acc[model.id] = model.name
        return acc
      },
      {} as Record<string, string>,
    )
  }, [config?.aiModels])

  const pendingToolCallIdSet = useMemo(() => {
    if (!pendingToolCalls?.length) {
      return new Set<string>()
    }

    return new Set(pendingToolCalls.map((call) => call.id))
  }, [pendingToolCalls])

  const renderableMessages = useMemo<RenderableMessage[]>(() => {
    if (currentMessages.length === 0) {
      return []
    }

    const nextMessages: RenderableMessage[] = []
    const consumedToolMessageIndexes = new Set<number>()

    for (
      let sourceIndex = 0;
      sourceIndex < currentMessages.length;
      sourceIndex += 1
    ) {
      if (consumedToolMessageIndexes.has(sourceIndex)) {
        continue
      }
      const msg = currentMessages[sourceIndex]
      let toolOutputsByCallId: Record<string, string> | undefined
      if (msg.role === "assistant" && msg.tool_calls?.length) {
        const outputCollection = collectAssistantToolOutputs(
          currentMessages,
          sourceIndex,
        )
        outputCollection.consumedToolMessageIndexes.forEach((index) => {
          consumedToolMessageIndexes.add(index)
        })
        if (outputCollection.consumedToolMessageIndexes.length > 0) {
          toolOutputsByCallId = outputCollection.toolOutputsByCallId
        }
      }
      if (msg.role === "tool") {
        continue
      }
      if (msg.role === "assistant") {
        const hasContent = !!(msg.content && msg.content.trim().length > 0)
        const hasReasoning = !!(
          msg.reasoning_content && msg.reasoning_content.trim().length > 0
        )
        const hasVisibleTools = !!msg.tool_calls?.some(
          (tc) => !HIDDEN_TOOL_CALL_NAMES.has(tc.function.name),
        )

        if (!hasContent && !hasVisibleTools && !hasReasoning) {
          continue
        }
      }

      const isPending = !!(
        pendingToolCallIdSet.size &&
        msg.tool_calls?.some((tc) => pendingToolCallIdSet.has(tc.id))
      )

      const modelName =
        msg.role === "assistant" && msg.model_id
          ? modelNameById[msg.model_id] || msg.model_id
          : null

      nextMessages.push({
        msg,
        sourceIndex,
        isPending,
        modelName,
        toolOutputsByCallId,
      })
    }

    return nextMessages
  }, [currentMessages, modelNameById, pendingToolCallIdSet])
  const latestAssistantMessageSourceIndex = useMemo(() => {
    for (let i = renderableMessages.length - 1; i >= 0; i -= 1) {
      const candidate = renderableMessages[i]
      if (candidate.msg.role === "assistant") {
        return candidate.sourceIndex
      }
    }

    return null
  }, [renderableMessages])
  const canRegenerateLatestAssistant =
    !!activeSessionId &&
    !isLoading &&
    !pendingToolCalls &&
    latestAssistantMessageSourceIndex !== null
  const scrollPositionKey = useMemo(() => {
    const scope = currentTabId
      ? `tab:${currentTabId}`
      : currentServerId
        ? `server:${currentServerId}`
        : "global"
    return `${scope}:session:${activeSessionId || "none"}`
  }, [activeSessionId, currentServerId, currentTabId])

  const renderableListItems = useMemo<RenderableListItem[]>(() => {
    const sessionKey = activeSessionId || "no-session"
    const items: RenderableListItem[] = renderableMessages.map(
      (message, messageIndex) => ({
        kind: "message",
        key: `${sessionKey}-message-${message.sourceIndex}-${message.msg.created_at || "no-date"}`,
        message,
        messageIndex,
      }),
    )

    if (pendingToolCalls) {
      items.push({
        kind: "pending-tools",
        key: `${sessionKey}-pending-tools`,
      })
    } else if (isLoading) {
      items.push({
        kind: "typing",
        key: `${sessionKey}-typing`,
      })
    }

    return items
  }, [activeSessionId, renderableMessages, pendingToolCalls, isLoading])
  const messageVirtualizer = useVirtualizer({
    count: renderableListItems.length,
    getScrollElement: () => messagesContainerRef.current,
    getItemKey: (index) => renderableListItems[index]?.key || index,
    estimateSize: () => 188,
    overscan: 8,
  })

  messageVirtualizer.shouldAdjustScrollPositionOnItemSizeChange = (
    item,
    _delta,
    instance,
  ) => {
    if (instance.scrollDirection === "backward") {
      return false
    }

    return item.start < (instance.scrollOffset ?? 0)
  }

  const virtualMessageItems = messageVirtualizer.getVirtualItems()
  const virtualizedTotalHeight = messageVirtualizer.getTotalSize()

  const scrollToBottom = useCallback(
    (behavior: VirtualScrollBehavior = "auto") => {
      const container = messagesContainerRef.current
      if (!container) {
        return
      }

      isAtBottomRef.current = true

      if (renderableListItems.length > 0) {
        messageVirtualizer.scrollToIndex(renderableListItems.length - 1, {
          align: "end",
          behavior,
        })
      }

      if (behavior === "smooth") {
        container.scrollTo({ top: container.scrollHeight, behavior: "smooth" })
      } else {
        container.scrollTop = container.scrollHeight
      }
    },
    [messageVirtualizer, renderableListItems.length],
  )

  const handleScroll = useCallback(() => {
    if (messagesContainerRef.current) {
      const { scrollTop, scrollHeight, clientHeight } =
        messagesContainerRef.current
      scrollPositionsRef.current[scrollPositionKey] = scrollTop
      const atBottom = scrollHeight - scrollTop - clientHeight < 50
      isAtBottomRef.current = atBottom
    }
  }, [scrollPositionKey])

  useLayoutEffect(() => {
    return () => {
      const container = messagesContainerRef.current
      if (!container) {
        return
      }

      scrollPositionsRef.current[scrollPositionKey] = container.scrollTop
    }
  }, [isOpen, scrollPositionKey, showHistory])

  useLayoutEffect(() => {
    if (!isOpen || showHistory) {
      pendingScrollRestoreRef.current = null
      isRestoringScrollRef.current = false
      return
    }

    const savedScrollTop = scrollPositionsRef.current[scrollPositionKey]
    if (savedScrollTop === undefined) {
      pendingScrollRestoreRef.current = null
      isRestoringScrollRef.current = false
      isAtBottomRef.current = true
      return
    }

    pendingScrollRestoreRef.current = {
      key: scrollPositionKey,
      scrollTop: savedScrollTop,
    }
    isRestoringScrollRef.current = true
  }, [isOpen, scrollPositionKey, showHistory])

  useLayoutEffect(() => {
    const pendingRestore = pendingScrollRestoreRef.current
    const container = messagesContainerRef.current

    if (
      !isOpen ||
      showHistory ||
      !container ||
      !pendingRestore ||
      pendingRestore.key !== scrollPositionKey
    ) {
      return
    }

    const restoreScrollTop = () => {
      const maxScrollTop = Math.max(
        0,
        container.scrollHeight - container.clientHeight,
      )
      const nextScrollTop = Math.min(pendingRestore.scrollTop, maxScrollTop)
      container.scrollTop = nextScrollTop
      isAtBottomRef.current = maxScrollTop - nextScrollTop < 50
    }

    restoreScrollTop()

    if (renderableListItems.length === 0 && pendingRestore.scrollTop > 0) {
      return
    }

    const rafId = requestAnimationFrame(() => {
      restoreScrollTop()
      if (pendingScrollRestoreRef.current?.key === scrollPositionKey) {
        pendingScrollRestoreRef.current = null
        isRestoringScrollRef.current = false
      }
    })

    return () => cancelAnimationFrame(rafId)
  }, [
    isOpen,
    renderableListItems.length,
    scrollPositionKey,
    showHistory,
    virtualizedTotalHeight,
  ])

  const lastMessageRenderSignature = useMemo(() => {
    const lastMsg = currentMessages[currentMessages.length - 1]
    if (lastMsg?.role === "assistant") {
      const contentLength =
        (lastMsg.content?.length || 0) +
        (lastMsg.reasoning_content?.length || 0)
      const toolCallsCount = lastMsg.tool_calls?.length || 0
      return `${currentMessages.length}:${contentLength + toolCallsCount}`
    }
    return `${currentMessages.length}:0`
  }, [currentMessages])

  useEffect(() => {
    if (isRestoringScrollRef.current) {
      return
    }

    if (!isAtBottomRef.current) {
      return
    }

    const rafId = requestAnimationFrame(() => {
      if (!isAtBottomRef.current) {
        return
      }
      scrollToBottom(isLoading ? "auto" : "smooth")
    })
    return () => cancelAnimationFrame(rafId)
  }, [
    activeSessionId,
    lastMessageRenderSignature,
    renderableListItems.length,
    isLoading,
    scrollToBottom,
  ])

  // Focus textarea when sidebar opens or when tools are resolved
  useEffect(() => {
    if (isOpen && !pendingToolCalls) {
      textareaRef.current?.focus()
    }
  }, [isOpen, pendingToolCalls])

  // Auto-resize textarea
  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto"
      if (inputValue) {
        textareaRef.current.style.height =
          Math.min(textareaRef.current.scrollHeight, 150) + "px"
      } else {
        textareaRef.current.style.height = "28px"
      }
    }
  }, [inputValue])

  const handleDeleteSession = async (sessionId: string) => {
    if (currentServerId) {
      try {
        await deleteSession(currentServerId, sessionId)
        setSessionToDelete(null)
      } catch (err) {
        // Failed to delete session
      }
    }
  }

  const handleClearHistory = async () => {
    if (currentServerId) {
      try {
        await clearSessions(currentServerId)
        setIsClearingHistory(false)
      } catch (err) {
        // Failed to clear history
      }
    }
  }

  const executeToolCalls = useCallback(
    async (calls: ToolCall[]) => {
      if (!activeSessionId) return

      isAtBottomRef.current = true
      setGenerating(activeSessionId, true)
      clearSessionStopped(activeSessionId)

      try {
        const model = config?.aiModels.find((m) => m.id === selectedModelId)
        const channelId = model?.channelId || ""

        await aiService.executeAgentTools(
          activeSessionId,
          selectedModelId,
          channelId,
          mode,
          boundSshSessionId,
          calls.map((c) => c.id),
          thinkingLevel,
        )
        await selectSession(activeSessionId)
      } catch (err) {
        setGenerating(activeSessionId, false)
        showAiError(err)
      }
    },
    [
      activeSessionId,
      boundSshSessionId,
      clearSessionStopped,
      config,
      mode,
      selectSession,
      selectedModelId,
      setGenerating,
      showAiError,
      thinkingLevel,
    ],
  )

  // Listen for streaming responses & tool calls
  useEffect(() => {
    if (!activeSessionId) return

    const flushStreamBuffers = () => {
      if (responseChunkBufferRef.current) {
        appendResponse(activeSessionId, responseChunkBufferRef.current)
        responseChunkBufferRef.current = ""
      }

      if (reasoningChunkBufferRef.current) {
        appendReasoning(activeSessionId, reasoningChunkBufferRef.current)
        reasoningChunkBufferRef.current = ""
      }
    }

    const scheduleStreamFlush = () => {
      if (streamFlushTimerRef.current) return
      streamFlushTimerRef.current = setTimeout(() => {
        flushStreamBuffers()
        streamFlushTimerRef.current = null
      }, 33)
    }

    const startedListener = listen<string>(
      `ai-started-${activeSessionId}`,
      () => {
        storeSetPendingToolCalls(activeSessionId, null)
        newAssistantMessage(activeSessionId, selectedModelId)
      },
    )

    // Handle MessageBatch events (minimax-style multiple messages in one response)
    const messageBatchListener = listen<ChatMessage[]>(
      `ai-message-batch-${activeSessionId}`,
      (event) => {
        const messages = event.payload
        // Add each message as a separate bubble
        messages.forEach((msg) => {
          addCompleteMessage(activeSessionId, msg)
        })
      },
    )

    const responseListener = listen<string>(
      `ai-response-${activeSessionId}`,
      (event) => {
        responseChunkBufferRef.current += event.payload
        scheduleStreamFlush()
      },
    )

    const reasoningListener = listen<string>(
      `ai-reasoning-${activeSessionId}`,
      (event) => {
        reasoningChunkBufferRef.current += event.payload
        scheduleStreamFlush()
      },
    )

    const toolCallListener = listen<ToolCall[]>(
      `ai-tool-call-${activeSessionId}`,
      (event) => {
        const calls = event.payload

        // Update store with tool calls so they appear in the message bubble
        appendToolCalls(activeSessionId, calls)

        // Filter: if ALL calls are read-only tools, auto-execute immediately WITHOUT UI
        const isAllSafe = calls.every(
          (c) =>
            c.function.name === "get_terminal_output" ||
            c.function.name === "get_selected_terminal_output" ||
            c.function.name === "read_file",
        )

        const shouldExecuteWithoutConfirmation =
          isAllSafe ||
          (autoConfirmDelaySeconds === 0 && !hasSensitiveToolCall(calls))

        if (shouldExecuteWithoutConfirmation) {
          void executeToolCalls(calls)
        } else {
          setGenerating(activeSessionId, false)
          storeSetPendingToolCalls(activeSessionId, calls)
        }
      },
    )

    const errorListener = listen<string>(
      `ai-error-${activeSessionId}`,
      (event) => {
        flushStreamBuffers()
        setGenerating(activeSessionId, false)
        storeSetPendingToolCalls(activeSessionId, null)
        showAiError(event.payload)
      },
    )

    const doneListener = listen<string>(
      `ai-done-${activeSessionId}`,
      async () => {
        flushStreamBuffers()
        setGenerating(activeSessionId, false)

        // Auto-generate title for new sessions after first response
        const currentSession = sessions.find((s) => s.id === activeSessionId)
        if (currentSession && currentSession.title === "New Chat") {
          try {
            const model = config?.aiModels.find((m) => m.id === selectedModelId)
            const channelId = model?.channelId || ""

            await aiService.generateTitle(
              activeSessionId,
              selectedModelId,
              channelId,
            )

            // Reload sessions to get the updated title
            if (currentServerId) {
              await loadSessions(currentServerId)
            }
          } catch (err) {
            // Failed to generate title
          }
        }
      },
    )

    return () => {
      if (streamFlushTimerRef.current) {
        clearTimeout(streamFlushTimerRef.current)
        streamFlushTimerRef.current = null
      }
      flushStreamBuffers()
      startedListener.then((unlisten) => unlisten())
      messageBatchListener.then((unlisten) => unlisten())
      responseListener.then((unlisten) => unlisten())
      reasoningListener.then((unlisten) => unlisten())
      toolCallListener.then((unlisten) => unlisten())
      errorListener.then((unlisten) => unlisten())
      doneListener.then((unlisten) => unlisten())
    }
  }, [
    activeSessionId,
    addCompleteMessage,
    appendResponse,
    appendReasoning,
    appendToolCalls,
    newAssistantMessage,
    setGenerating,
    storeSetPendingToolCalls,
    autoConfirmDelaySeconds,
    executeToolCalls,
    config,
    selectedModelId,
    sessions,
    currentServerId,
    loadSessions,
    showAiError,
  ])

  // Resizing logic
  const startResizing = (e: React.MouseEvent) => {
    e.preventDefault()
    setIsResizing(true)
  }

  useEffect(() => {
    const stopResizing = () => setIsResizing(false)
    const resize = (e: MouseEvent) => {
      if (isResizing) {
        const newWidth = window.innerWidth - e.clientX
        if (newWidth >= 250 && newWidth <= 800) {
          setWidth(newWidth)
        }
      }
    }

    if (isResizing) {
      window.addEventListener("mousemove", resize)
      window.addEventListener("mouseup", stopResizing)
      window.addEventListener("pointerup", stopResizing)
      window.addEventListener("blur", stopResizing)
      document.body.style.cursor = "col-resize"
      document.body.style.userSelect = "none"
    }

    return () => {
      window.removeEventListener("mousemove", resize)
      window.removeEventListener("mouseup", stopResizing)
      window.removeEventListener("pointerup", stopResizing)
      window.removeEventListener("blur", stopResizing)
      document.body.style.cursor = ""
      document.body.style.userSelect = ""
    }
  }, [isResizing])

  const handleCreateSession = useCallback(() => {
    if (currentServerId) {
      if (currentTabId) {
        void selectSession(null, undefined, currentTabId)
      } else {
        void selectSession(null, currentServerId)
      }
      setShowHistory(false)
    }
  }, [currentServerId, currentTabId, selectSession])

  const appendPathToInput = useCallback(
    (path: string, useReadFilePrefix: boolean) => {
      const token = `${useReadFilePrefix ? "#" : ""}${path}`
      setInputValue((prev) => {
        const normalized = prev.trimEnd()
        const merged = normalized.length > 0 ? `${normalized} ${token}` : token
        return `${merged} `
      })

      if (textareaRef.current) {
        textareaRef.current.focus()
      }
    },
    [],
  )

  const handleInputDragOver = useCallback(
    (e: React.DragEvent<HTMLTextAreaElement>) => {
      if (
        !e.dataTransfer.types.includes(SFTP_PATH_MIME_TYPE) &&
        !e.dataTransfer.types.includes(SFTP_ENTRY_MIME_TYPE)
      ) {
        return
      }

      e.preventDefault()
      e.dataTransfer.dropEffect = "copy"
      setIsInputDragOver(true)
    },
    [],
  )

  const handleInputDragLeave = useCallback(
    (e: React.DragEvent<HTMLTextAreaElement>) => {
      if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
        setIsInputDragOver(false)
      }
    },
    [],
  )

  const handleInputDrop = useCallback(
    (e: React.DragEvent<HTMLTextAreaElement>) => {
      const entryRaw = e.dataTransfer.getData(SFTP_ENTRY_MIME_TYPE)
      const pathRaw = e.dataTransfer.getData(SFTP_PATH_MIME_TYPE)
      const fallbackPath = e.dataTransfer.getData("text/plain")
      const droppedPath = pathRaw || fallbackPath

      if (!entryRaw && !droppedPath) {
        return
      }

      e.preventDefault()
      setIsInputDragOver(false)

      if (entryRaw) {
        try {
          const entry = JSON.parse(entryRaw) as SftpDragEntry
          if (entry.path) {
            appendPathToInput(entry.path, !entry.isDir)
            return
          }
        } catch {}
      }

      if (droppedPath) {
        appendPathToInput(droppedPath, false)
      }
    },
    [appendPathToInput],
  )

  const handleRegenerateResponse = useCallback(async () => {
    if (!activeSessionId || isLoading || !!pendingToolCalls) return
    if (latestAssistantMessageSourceIndex === null) return

    clearSessionStopped(activeSessionId)
    isAtBottomRef.current = true
    removeLatestAssistantMessage(activeSessionId)
    setGenerating(activeSessionId, true)

    try {
      const model = config?.aiModels.find((m) => m.id === selectedModelId)
      const channelId = model?.channelId || ""

      await aiService.regenerateResponse(
        activeSessionId,
        selectedModelId,
        channelId,
        mode,
        boundSshSessionId,
        thinkingLevel,
      )

      if (currentServerId) {
        await loadSessions(currentServerId)
      }
    } catch (err) {
      setGenerating(activeSessionId, false)
      showAiError(err)
      try {
        if (currentTabId) {
          await selectSession(activeSessionId, currentServerId, currentTabId)
        } else {
          await selectSession(activeSessionId, currentServerId)
        }
      } catch {}
    }
  }, [
    activeSessionId,
    isLoading,
    pendingToolCalls,
    latestAssistantMessageSourceIndex,
    clearSessionStopped,
    removeLatestAssistantMessage,
    setGenerating,
    config,
    selectedModelId,
    thinkingLevel,
    mode,
    boundSshSessionId,
    currentServerId,
    currentTabId,
    loadSessions,
    showAiError,
    selectSession,
  ])

  const handleSendMessage = useCallback(async () => {
    if (!inputValue.trim() || isLoading || !!pendingToolCalls) return

    if (shouldBlockEditorContextSend) {
      const template =
        t.ai.editorContext.tooLarge ||
        "Current file length {count} exceeds the limit {max}."
      showAiError(
        template
          .replace("{count}", String(activeEditorContextCharCount))
          .replace("{max}", String(MAX_EDITOR_CONTEXT_CHARS)),
      )
      return
    }
    const content = inputValue
    const requestContent =
      includeEditorContext && activeEditorContext
        ? buildMessageWithEditorContext(content, activeEditorContext)
        : content
    const sessionBindingId = currentSshSessionId || currentTabId
    const sessionScopeId = currentTabId || currentSshSessionId
    let sessionId = activeSessionId

    if (!sessionId) {
      if (!currentServerId) return
      try {
        sessionId = await createSession(
          currentServerId,
          selectedModelId,
          sessionBindingId,
          sessionScopeId,
        )
      } catch (err) {
        showAiError(err)
        return
      }
    }

    if (!sessionId) return

    // Clear the stopped flag when sending a new message
    clearSessionStopped(sessionId)
    isAtBottomRef.current = true
    setInputValue("")
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto"
      textareaRef.current.focus()
    }

    const optimisticUserMessage: ChatMessage = {
      role: "user",
      content,
      created_at: new Date().toISOString(),
    }

    addOptimisticMessage(optimisticUserMessage)

    addMessage(sessionId, optimisticUserMessage)
    setGenerating(sessionId, true)

    try {
      const model = config?.aiModels.find((m) => m.id === selectedModelId)
      const channelId = model?.channelId || ""

      await aiService.sendMessage(
        sessionId,
        requestContent,
        selectedModelId,
        channelId,
        mode,
        boundSshSessionId,
        thinkingLevel,
      )

      if (currentServerId) {
        await loadSessions(currentServerId)
      }
    } catch (err) {
      setGenerating(sessionId, false)
      showAiError(err)
    }
  }, [
    inputValue,
    activeSessionId,
    currentServerId,
    selectedModelId,
    currentSshSessionId,
    createSession,
    addMessage,
    setGenerating,
    config,
    mode,
    thinkingLevel,
    currentTabId,
    boundSshSessionId,
    isLoading,
    pendingToolCalls,
    includeEditorContext,
    activeEditorContext,
    activeEditorContextCharCount,
    shouldBlockEditorContextSend,
    clearSessionStopped,
    loadSessions,
    t.ai.editorContext.tooLarge,
    showAiError,
  ])

  const handleConfirmTools = useCallback(async () => {
    if (!activeSessionId || !pendingToolCalls) return

    const callsToExecute = pendingToolCalls
    storeSetPendingToolCalls(activeSessionId, null) // Hide confirmation
    await executeToolCalls(callsToExecute)
  }, [
    activeSessionId,
    executeToolCalls,
    pendingToolCalls,
    storeSetPendingToolCalls,
  ])

  const handleCancelTools = useCallback(() => {
    if (activeSessionId) {
      storeSetPendingToolCalls(activeSessionId, null)
      setGenerating(activeSessionId, false)
      markSessionStopped(activeSessionId) // Mark as stopped when tools are cancelled
    }
    // Optionally insert a "Cancelled" system message
  }, [
    activeSessionId,
    storeSetPendingToolCalls,
    setGenerating,
    markSessionStopped,
  ])

  const handleStopGeneration = useCallback(async () => {
    // 1. Clear frontend pending tools and mark session as stopped
    if (activeSessionId && pendingToolCalls) {
      storeSetPendingToolCalls(activeSessionId, null)
    }

    // 2. Cancel backend processing if active
    if (activeSessionId && isLoading) {
      try {
        await aiService.cancelMessage(activeSessionId)
      } catch (err) {
        // Failed to cancel message
      }
    }

    // 3. Ensure loading is turned off and mark as stopped
    if (activeSessionId) {
      setGenerating(activeSessionId, false)
      markSessionStopped(activeSessionId)

      const currentSession = sessions.find((s) => s.id === activeSessionId)
      if (currentSession && currentSession.title === "New Chat") {
        try {
          const model = config?.aiModels.find((m) => m.id === selectedModelId)
          const channelId = model?.channelId || ""

          await aiService.generateTitle(
            activeSessionId,
            selectedModelId,
            channelId,
          )

          if (currentServerId) {
            await loadSessions(currentServerId)
          }
        } catch (err) {}
      }
    }
  }, [
    activeSessionId,
    isLoading,
    pendingToolCalls,
    setGenerating,
    storeSetPendingToolCalls,
    markSessionStopped,
    sessions,
    selectedModelId,
    config,
    currentServerId,
    loadSessions,
  ])

  const handleModeChange = async (newMode: "ask" | "agent") => {
    setMode(newMode)
    if (config) {
      try {
        const newConfig = {
          ...config,
          general: {
            ...config.general,
            aiMode: newMode,
          },
        }
        await saveConfig(newConfig)
      } catch (err) {
        // Failed to save AI mode
      }
    }
  }

  const handleModelChange = async (newModelId: string) => {
    setSelectedModelId(newModelId)
    if (config) {
      try {
        const newConfig = {
          ...config,
          general: {
            ...config.general,
            aiModelId: newModelId,
          },
        }
        await saveConfig(newConfig)
      } catch (err) {
        // Failed to save AI model
      }
    }
  }

  const handleThinkingLevelChange = async (newLevel: string) => {
    const nextLevel = getAIThinkingLevel(newLevel)
    setThinkingLevel(nextLevel)
    if (config) {
      try {
        const newConfig = {
          ...config,
          general: {
            ...config.general,
            aiThinkingLevel: nextLevel,
          },
        }
        await saveConfig(newConfig)
      } catch (err) {
        // Failed to save AI thinking level
      }
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault()
      if (!isLoading && !pendingToolCalls) {
        handleSendMessage()
      }
    }
  }

  // Close on click outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        isOpen &&
        !isLocked &&
        sidebarRef.current &&
        !sidebarRef.current.contains(event.target as Node)
      ) {
        onClose()
      }
    }

    if (isOpen && !isLocked) {
      document.addEventListener("mousedown", handleClickOutside)
    }

    return () => {
      document.removeEventListener("mousedown", handleClickOutside)
    }
  }, [isOpen, isLocked, onClose])

  const sortedSessions = useMemo(() => {
    return [...sessions].sort(
      (a, b) =>
        new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
    )
  }, [sessions])

  return (
    <div
      ref={sidebarRef}
      className={`absolute top-0 bottom-0 overflow-hidden bg-[var(--bg-secondary)] border-l flex flex-col transition-all duration-200 shadow-[-2px_0_8px_rgba(0,0,0,0.2)] !right-0 !left-auto ${isOpen ? "opacity-100 visible border-l-[var(--glass-border)]" : "opacity-0 invisible border-transparent"} ${isResizing ? "transition-none" : ""} ${isLocked ? "!relative shadow-none !right-auto !top-auto !bottom-auto h-full" : ""}`}
      style={{ width: isOpen ? `${width}px` : "0px", zIndex }}
      aria-hidden={!isOpen}
    >
      <div
        className="absolute top-0 bottom-0 left-0 w-[5px] cursor-col-resize bg-transparent transition-colors duration-200 hover:bg-[var(--accent-primary)] hover:opacity-50"
        onMouseDown={startResizing}
        role="separator"
        aria-orientation="vertical"
        aria-valuenow={width}
        aria-valuemin={250}
        aria-valuemax={800}
        aria-label="Resize Sidebar"
        tabIndex={0}
        style={{ zIndex: zIndex ? zIndex + 1 : undefined }}
      />

      <div className="flex items-center justify-between p-3 pl-4 border-b border-[var(--glass-border)] flex-shrink-0">
        <h3 className="text-[13px] font-semibold text-[var(--text-primary)] flex items-center gap-2 m-0 whitespace-nowrap">
          <Bot size={16} /> {t.ai.sidebarTitle}
        </h3>
        <div className="flex items-center gap-1">
          <button
            type="button"
            className={`bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] disabled:opacity-50 disabled:cursor-not-allowed disabled:text-[var(--text-muted)] ${showHistory ? "text-[var(--accent-primary)]" : ""}`}
            onClick={() => setShowHistory(!showHistory)}
            title={t.ai.history}
            disabled={!currentServerId}
          >
            <History size={16} />
          </button>
          <button
            type="button"
            className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] disabled:opacity-50 disabled:cursor-not-allowed disabled:text-[var(--text-muted)]"
            onClick={handleCreateSession}
            title={t.ai.newChat}
            disabled={!currentServerId}
          >
            <Plus size={16} />
          </button>
          <button
            type="button"
            className={`bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] disabled:opacity-50 disabled:cursor-not-allowed disabled:text-[var(--text-muted)] ${isLocked ? "text-[var(--accent-primary)]" : ""}`}
            onClick={onToggleLock}
            title={isLocked ? "Unlock Sidebar" : "Lock Sidebar"}
          >
            {isLocked ? <Lock size={16} /> : <LockOpen size={16} />}
          </button>
          <button
            type="button"
            className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] disabled:opacity-50 disabled:cursor-not-allowed disabled:text-[var(--text-muted)]"
            onClick={onClose}
            title="Close"
          >
            <X size={16} />
          </button>
        </div>
      </div>

      {showHistory ? (
        <div className="flex-1 overflow-y-auto px-4 py-4 flex flex-col gap-4 scroll-smooth">
          {sortedSessions.length === 0 ? (
            <div className="flex-1 flex flex-col items-center justify-center p-5 text-[var(--text-muted)] text-center gap-3">
              <History size={48} className="opacity-20" />
              <p>{t.ai.noHistory}</p>
            </div>
          ) : (
            <div className="flex flex-col gap-2 p-2">
              <div className="flex justify-between items-center px-3 py-1 mb-1 text-[11px] text-[var(--text-muted)] uppercase font-semibold tracking-wider">
                <span>{t.ai.history}</span>
                <button
                  type="button"
                  className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer px-2 py-1 rounded text-[11px] flex items-center gap-1 transition-all duration-200 font-normal hover:bg-red-500/10 hover:text-red-500"
                  onClick={() => setIsClearingHistory(true)}
                >
                  <Trash2 size={12} /> {t.ai.clearHistory}
                </button>
              </div>
              {sortedSessions.map((session) => (
                <button
                  type="button"
                  key={session.id}
                  className={`flex items-center gap-3 px-3 py-2.5 rounded-lg bg-[var(--bg-elevated)] border border-transparent cursor-pointer transition-all duration-200 w-full text-left relative overflow-hidden hover:bg-[var(--bg-tertiary)] hover:border-[var(--glass-border)] ${activeSessionId === session.id ? "bg-[var(--bg-tertiary)] border-[var(--accent-primary)] shadow-[0_2px_8px_rgba(0,0,0,0.1)]" : ""}`}
                  onClick={() => {
                    void selectSession(
                      session.id,
                      currentServerId,
                      currentTabId,
                    )
                    setShowHistory(false)
                  }}
                >
                  <div className="flex items-center justify-center w-8 h-8 rounded-md bg-white/5 text-[var(--text-muted)] flex-shrink-0 transition-all duration-200">
                    <MessageSquare size={16} />
                  </div>
                  <div className="flex-1 min-w-0 flex flex-col gap-0.5">
                    <div
                      className="text-[13px] font-medium text-[var(--text-primary)] whitespace-nowrap overflow-hidden text-ellipsis"
                      title={session.title || t.ai.newChat}
                    >
                      <EmojiText text={session.title || t.ai.newChat} />
                    </div>
                    <div className="text-[11px] text-[var(--text-muted)] flex items-center gap-1">
                      <Clock size={10} />
                      {new Date(session.createdAt).toLocaleString(undefined, {
                        month: "short",
                        day: "numeric",
                        hour: "2-digit",
                        minute: "2-digit",
                      })}
                    </div>
                  </div>
                  <button
                    type="button"
                    className="ai-history-delete"
                    onClick={(e) => {
                      e.stopPropagation()
                      setSessionToDelete(session.id)
                    }}
                    title={t.common.delete}
                  >
                    <Trash2 size={14} />
                  </button>
                </button>
              ))}
            </div>
          )}
        </div>
      ) : (
        <>
          <div
            className="flex-1 overflow-y-auto px-4 py-4 flex flex-col gap-4"
            ref={messagesContainerRef}
            onScroll={handleScroll}
          >
            {!activeSessionId && (
              <div className="flex-1 flex flex-col items-center justify-center text-[var(--text-muted)] text-center p-8 gap-4 min-h-[200px]">
                <Bot size={48} className="opacity-20 mb-4" />
                <p>{currentServerId ? t.ai.typeMessage : t.ai.selectServer}</p>
              </div>
            )}

            <div
              className="relative w-full shrink-0"
              style={{ height: `${virtualizedTotalHeight}px` }}
            >
              {virtualMessageItems.map((virtualItem) => {
                const item = renderableListItems[virtualItem.index]
                if (!item) {
                  return null
                }

                return (
                  <div
                    key={item.key}
                    data-index={virtualItem.index}
                    ref={messageVirtualizer.measureElement}
                    className="absolute left-0 top-0 w-full"
                    style={{
                      transform: `translateY(${virtualItem.start}px)`,
                      paddingBottom: `${VIRTUAL_MESSAGE_GAP_PX}px`,
                    }}
                  >
                    {item.kind === "message" ? (
                      <MessageBubble
                        msg={item.message.msg}
                        t={t}
                        isPending={item.message.isPending}
                        isLast={
                          item.messageIndex === renderableMessages.length - 1
                        }
                        isStreaming={
                          isLoading &&
                          item.messageIndex === renderableMessages.length - 1
                        }
                        modelName={item.message.modelName}
                        toolOutputsByCallId={item.message.toolOutputsByCallId}
                        canRegenerate={
                          canRegenerateLatestAssistant &&
                          item.message.sourceIndex ===
                            latestAssistantMessageSourceIndex
                        }
                        onRegenerate={handleRegenerateResponse}
                      />
                    ) : item.kind === "pending-tools" ? (
                      <div className="ai-message assistant">
                        <div className="ai-message-content p-0 overflow-hidden">
                          <ToolConfirmation
                            toolCalls={pendingToolCalls || []}
                            autoConfirmDelaySeconds={autoConfirmDelaySeconds}
                            onConfirm={handleConfirmTools}
                            onCancel={handleCancelTools}
                          />
                        </div>
                      </div>
                    ) : (
                      <div className="flex flex-col gap-1 max-w-full items-start">
                        <div className="relative max-w-[90%] flex flex-col items-start">
                          <div className="p-2 px-3 rounded-lg text-[13px] leading-[1.5] w-full break-words overflow-x-auto bg-[var(--bg-tertiary)] text-[var(--text-primary)] rounded-tl-sm">
                            <div className="flex items-center gap-1 px-2 py-1 h-5">
                              <div
                                className="w-1.5 h-1.5 bg-[var(--text-muted)] rounded-full animate-typing-bounce"
                                style={{ animationDelay: "-0.32s" }}
                              ></div>
                              <div
                                className="w-1.5 h-1.5 bg-[var(--text-muted)] rounded-full animate-typing-bounce"
                                style={{ animationDelay: "-0.16s" }}
                              ></div>
                              <div className="w-1.5 h-1.5 bg-[var(--text-muted)] rounded-full animate-typing-bounce"></div>
                            </div>
                          </div>
                        </div>
                      </div>
                    )}
                  </div>
                )
              })}
            </div>
          </div>

          <div className="p-3 border-t border-[var(--glass-border)] bg-[var(--bg-secondary)] flex flex-col gap-2">
            <div className="flex gap-2">
              <div className="relative flex items-center flex-0-auto min-w-[100px] peer">
                <Sliders
                  size={14}
                  className="absolute left-2.5 text-[var(--text-muted)] pointer-events-none z-1 transition-colors duration-200 peer-hover:text-[var(--accent-primary)] focus-within:text-[var(--accent-primary)]"
                />
                <CustomSelect
                  value={mode}
                  onChange={(val) => handleModeChange(val as "ask" | "agent")}
                  disabled={isLoading || !!pendingToolCalls}
                  placement="top"
                  triggerClassName="pl-8"
                  options={[
                    { value: "ask", label: "Ask" },
                    { value: "agent", label: "Agent" },
                  ]}
                />
              </div>
              <div className="relative flex items-center flex-1 min-w-0 peer">
                <Sparkles
                  size={14}
                  className="absolute left-2.5 text-[var(--text-muted)] pointer-events-none z-1 transition-colors duration-200 peer-hover:text-[var(--accent-primary)] focus-within:text-[var(--accent-primary)]"
                />
                <CustomSelect
                  value={selectedModelId}
                  onChange={(val) => handleModelChange(val)}
                  disabled={isLoading || !!pendingToolCalls}
                  placement="top"
                  triggerClassName="pl-8"
                  options={(config?.aiModels || [])
                    .filter((model) => {
                      const channel = config?.aiChannels?.find(
                        (c) => c.id === model.channelId,
                      )
                      return model.enabled && channel?.isActive
                    })
                    .map((model) => {
                      const channel = config?.aiChannels?.find(
                        (c) => c.id === model.channelId,
                      )
                      const label = channel
                        ? `${channel.name} - ${model.name}`
                        : model.name
                      return { value: model.id, label }
                    })}
                />
              </div>
              <div className="relative flex items-center flex-0-auto min-w-[96px] peer">
                <Brain
                  size={14}
                  className="absolute left-2.5 text-[var(--text-muted)] pointer-events-none z-1 transition-colors duration-200 peer-hover:text-[var(--accent-primary)] focus-within:text-[var(--accent-primary)]"
                />
                <CustomSelect
                  value={thinkingLevel}
                  onChange={handleThinkingLevelChange}
                  disabled={isLoading || !!pendingToolCalls}
                  placement="top"
                  triggerClassName="pl-8"
                  options={AI_THINKING_OPTIONS}
                />
              </div>
            </div>
            {activeEditorContext && (
              <div
                className={`flex items-start justify-between gap-2 rounded-[var(--radius-sm)] border px-2 py-1.5 text-[11px] ${includeEditorContext ? "border-[var(--accent-primary)] bg-[var(--accent-primary)]/10" : "border-[var(--glass-border)] bg-[var(--bg-elevated)]"}`}
              >
                <label className="inline-flex items-center gap-1.5 text-[var(--text-primary)] cursor-pointer select-none">
                  <input
                    type="checkbox"
                    className="m-0"
                    checked={includeEditorContext}
                    onChange={(e) => setIncludeEditorContext(e.target.checked)}
                    disabled={isLoading || !!pendingToolCalls}
                  />
                  <span>{t.ai.editorContext.includeCurrentFile}</span>
                </label>
                <div className="min-w-0 text-right leading-5">
                  <div
                    className="truncate text-[var(--text-secondary)]"
                    title={activeEditorContext.remotePath}
                  >
                    {t.ai.editorContext.fileLabel.replace(
                      "{path}",
                      activeEditorContext.remotePath,
                    )}
                  </div>
                  <div
                    className={
                      isEditorContextTooLarge
                        ? "text-[var(--danger)]"
                        : "text-[var(--text-secondary)]"
                    }
                  >
                    {`${t.ai.editorContext.languageLabel.replace(
                      "{language}",
                      activeEditorContext.language || "plaintext",
                    )} 路 ${t.ai.editorContext.charCountLabel.replace(
                      "{count}",
                      String(activeEditorContextCharCount),
                    )} / ${MAX_EDITOR_CONTEXT_CHARS}`}
                  </div>
                  <div className="text-[var(--text-muted)]">
                    {t.ai.editorContext.readOnlyHint}
                  </div>
                </div>
              </div>
            )}
            <div className="flex gap-2 items-end bg-[var(--bg-elevated)] border border-[var(--border-color)] rounded-[var(--radius-sm)] p-2 transition-colors duration-200 focus-within:border-[var(--accent-primary)]">
              <textarea
                ref={textareaRef}
                className={`flex-1 bg-transparent border-0 text-[var(--text-primary)] font-inherit text-[13px] leading-7 resize-none outline-none max-h-[150px] min-h-7 p-0 block ${isInputDragOver ? "opacity-80" : ""}`}
                placeholder={t.ai.typeMessage}
                value={inputValue}
                onChange={(e) => setInputValue(e.target.value)}
                onKeyDown={handleKeyDown}
                onDragOver={handleInputDragOver}
                onDragLeave={handleInputDragLeave}
                onDrop={handleInputDrop}
                disabled={!!pendingToolCalls}
                rows={1}
              />
              {isLoading || !!pendingToolCalls ? (
                <button
                  type="button"
                  className="bg-red-500 text-white border-0 rounded w-7 h-7 flex items-center justify-center cursor-pointer transition-colors duration-200 flex-shrink-0 hover:bg-red-600"
                  onClick={handleStopGeneration}
                  title={t.ai.stopGeneration}
                >
                  <Square size={14} fill="currentColor" />
                </button>
              ) : (
                <button
                  type="button"
                  className="bg-[var(--accent-primary)] text-white border-0 rounded w-7 h-7 flex items-center justify-center cursor-pointer transition-colors duration-200 flex-shrink-0 hover:bg-[var(--accent-hover)] disabled:bg-[var(--bg-tertiary)] disabled:text-[var(--text-muted)] disabled:cursor-not-allowed"
                  onClick={handleSendMessage}
                  disabled={
                    !inputValue.trim() ||
                    !selectedModelId ||
                    shouldBlockEditorContextSend
                  }
                >
                  <Send size={14} />
                </button>
              )}
            </div>
          </div>
        </>
      )}

      <ConfirmationModal
        isOpen={!!sessionToDelete}
        title={t.common.delete}
        message={t.ai.deleteSessionConfirm}
        onConfirm={() =>
          sessionToDelete && handleDeleteSession(sessionToDelete)
        }
        onCancel={() => setSessionToDelete(null)}
        type="danger"
      />

      <ConfirmationModal
        isOpen={isClearingHistory}
        title={t.ai.clearHistory}
        message={t.ai.clearHistoryConfirm}
        onConfirm={handleClearHistory}
        onCancel={() => setIsClearingHistory(false)}
        type="danger"
      />
    </div>
  )
}
