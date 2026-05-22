import React, { useEffect, useMemo, useRef, useState } from "react"
import ReactMarkdown from "react-markdown"
import {
  BrainCircuit,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  RotateCcw,
} from "lucide-react"
import type { ChatMessage, ToolCall } from "../../types/ai"
import { MARKDOWN_COMPONENTS, MARKDOWN_REMARK_PLUGINS } from "./markdown"
import {
  COMMAND_EXECUTION_TOOL_NAMES,
  COMMAND_OUTPUT_PREVIEW_MAX_LINES,
  getToolDisplayName,
  HIDDEN_TOOL_CALL_NAMES,
  parseCommandExecutionToolArgs,
} from "./helpers"

interface MessageBubbleProps {
  msg: ChatMessage
  t: any
  isPending?: boolean
  isLast?: boolean
  isStreaming?: boolean
  modelName: string | null
  toolOutputsByCallId?: Record<string, string>
  canRegenerate?: boolean
  onRegenerate?: () => void
}

export const MessageBubble = React.memo(
  ({
    msg,
    t,
    isPending,
    isLast,
    isStreaming,
    modelName,
    toolOutputsByCallId,
    canRegenerate,
    onRegenerate,
  }: MessageBubbleProps) => {
    const [copied, setCopied] = useState(false)
    const [showReasoning, setShowReasoning] = useState(true)
    const reasoningContentRef = useRef<HTMLDivElement>(null)
    const prevReasoningLength = useRef(msg.reasoning_content?.length || 0)

    const timeString = useMemo(() => {
      if (!msg.created_at) return ""
      try {
        // SQLite CURRENT_TIMESTAMP is UTC "YYYY-MM-DD HH:MM:SS"
        // We append 'Z' to ensure it's treated as UTC
        const dateStr = msg.created_at.endsWith("Z")
          ? msg.created_at
          : msg.created_at.replace(" ", "T") + "Z"
        const date = new Date(dateStr)
        return date.toLocaleString([], {
          year: "numeric",
          month: "2-digit",
          day: "2-digit",
          hour: "2-digit",
          minute: "2-digit",
        })
      } catch {
        return ""
      }
    }, [msg.created_at])

    useEffect(() => {
      if (showReasoning && reasoningContentRef.current) {
        const element = reasoningContentRef.current
        const currentLength = msg.reasoning_content?.length || 0

        if (currentLength >= prevReasoningLength.current) {
          requestAnimationFrame(() => {
            element.scrollTop = element.scrollHeight
          })
        }

        prevReasoningLength.current = currentLength
      }
    }, [msg.reasoning_content, showReasoning])

    const handleCopy = async () => {
      if (!msg.content) return
      try {
        await navigator.clipboard.writeText(msg.content)
        setCopied(true)
        setTimeout(() => setCopied(false), 2000)
      } catch (err) {
        // Failed to copy
      }
    }

    const visibleToolCalls = useMemo(() => {
      if (!msg.tool_calls || isPending) return []
      return msg.tool_calls.filter(
        (call) => !HIDDEN_TOOL_CALL_NAMES.has(call.function.name),
      )
    }, [isPending, msg.tool_calls])

    const hasContentToCopy = !!(msg.content && msg.content.trim().length > 0)
    const renderPlainStreamingContent = !!(
      isStreaming && msg.role === "assistant"
    )
    const canTriggerRegenerate = msg.role === "assistant" && !!canRegenerate
    const sideOffsetClass = msg.role === "user" ? "-left-8" : "-right-8"

    return (
      <div
        className={`flex flex-col gap-1 max-w-full ${msg.role === "user" ? "items-end" : "items-start"}`}
      >
        <div
          className={`relative max-w-[90%] flex flex-col group ${msg.role === "user" ? "items-end" : "items-start"}`}
        >
          <div
            className={`p-2 px-3 rounded-lg text-[13px] leading-[1.5] w-full break-words overflow-x-auto ${msg.role === "user" ? "ai-user-message-bubble bg-[var(--accent-primary)] text-white rounded-tr-sm selection:bg-white/55" : "bg-[var(--bg-tertiary)] text-[var(--text-primary)] rounded-tl-sm"}`}
          >
            {msg.reasoning_content && (
              <div className="w-full mb-2">
                <button
                  type="button"
                  className="flex items-center bg-transparent border-0 text-[var(--text-muted)] text-[12px] cursor-pointer p-1 transition-colors duration-200 hover:text-[var(--accent-primary)]"
                  onClick={() => setShowReasoning(!showReasoning)}
                >
                  {showReasoning ? (
                    <ChevronDown size={14} />
                  ) : (
                    <ChevronRight size={14} />
                  )}
                  <BrainCircuit size={14} className="ml-1 mr-1 text-blue-400" />
                  <span>{t.ai.thinkingProcess}</span>
                </button>
                {showReasoning && (
                  <div
                    className="leading-[1.6] max-h-[400px] overflow-y-auto font-serif italic bg-black/15 border-l-3 border-[var(--accent-primary)] rounded px-3.5 py-2.5 mt-1.5 text-[var(--text-muted)] text-[12.5px] shadow-inset relative whitespace-pre-wrap"
                    ref={reasoningContentRef}
                  >
                    {msg.reasoning_content}
                    {isLast && isStreaming && !msg.content && (
                      <span className="inline-block w-[2px] ml-0.5 text-[var(--accent-primary)] animate-cursor-blink vertical-align-middle font-bold">
                        |
                      </span>
                    )}
                  </div>
                )}
              </div>
            )}
            {visibleToolCalls.length > 0 ? (
              <div className="flex flex-col gap-3 bg-black/20 border rounded-lg p-3 w-full box-border mb-2">
                <div className="flex items-center gap-1.5 font-semibold text-[13px] text-[var(--text-primary)]">
                  <Check size={16} className="text-green-500" />
                  <span>{t.ai.tool.executeCommand}</span>
                </div>
                <div className="flex flex-col gap-2">
                  {visibleToolCalls.map((call: ToolCall) => {
                    let displayArgs = call.function.arguments
                    let timeoutSeconds: number | null = null
                    let waitFinish: boolean | null = null
                    const isCommandExecutionTool =
                      COMMAND_EXECUTION_TOOL_NAMES.has(call.function.name)
                    const commandOutput = isCommandExecutionTool
                      ? toolOutputsByCallId?.[call.id] || ""
                      : ""
                    if (isCommandExecutionTool) {
                      const parsedArgs = parseCommandExecutionToolArgs(
                        call.function.arguments,
                        call.function.name,
                      )
                      displayArgs = parsedArgs.displayCommand
                      timeoutSeconds = parsedArgs.timeoutSeconds
                      waitFinish = parsedArgs.waitFinish
                    }
                    return (
                      <div
                        key={call.id}
                        className="bg-black/20 p-2 rounded-md border border-white/10"
                      >
                        <span className="font-mono text-xs opacity-70 block">
                          {getToolDisplayName(call.function.name, t)}
                        </span>
                        <code className="block mt-1 w-full max-w-full text-sm bg-black/20 p-1 rounded font-mono whitespace-pre overflow-x-auto overflow-y-hidden">
                          {displayArgs}
                        </code>
                        {timeoutSeconds !== null && (
                          <span className="mt-1 block text-[11px] text-[var(--text-muted)]">
                            {t.ai.tool.timeoutSeconds.replace(
                              "{seconds}",
                              String(timeoutSeconds),
                            )}
                          </span>
                        )}
                        {waitFinish !== null && (
                          <span className="mt-1 block text-[11px] text-[var(--text-muted)]">
                            {t.ai.tool.waitFinish.replace(
                              "{value}",
                              waitFinish
                                ? t.ai.tool.waitFinishOn
                                : t.ai.tool.waitFinishOff,
                            )}
                          </span>
                        )}
                        {isCommandExecutionTool && (
                          <div className="mt-2">
                            <span className="font-mono text-xs opacity-70 block">
                              {t.ai.tool.commandOutput}
                            </span>
                            <pre
                              className="block mt-1 w-full max-w-full bg-black/30 p-2 rounded font-mono whitespace-pre overflow-x-auto overflow-y-auto leading-5"
                              style={{
                                maxHeight: `${COMMAND_OUTPUT_PREVIEW_MAX_LINES * 20}px`,
                              }}
                            >
                              {commandOutput || t.ai.tool.noCommandOutput}
                            </pre>
                          </div>
                        )}
                      </div>
                    )
                  })}
                </div>
              </div>
            ) : null}
            {msg.content &&
              (renderPlainStreamingContent ? (
                <div className="whitespace-pre-wrap break-words">
                  {msg.content}
                </div>
              ) : (
                <ReactMarkdown
                  remarkPlugins={MARKDOWN_REMARK_PLUGINS}
                  components={MARKDOWN_COMPONENTS}
                >
                  {msg.content}
                </ReactMarkdown>
              ))}
            <div
              className={`flex items-center gap-2 mt-1 text-[10px] select-none ${msg.role === "user" ? "text-white/60 justify-end" : "text-[var(--text-muted)] justify-between"}`}
            >
              {modelName && <span>{modelName}</span>}
              {timeString && <span>{timeString}</span>}
            </div>
          </div>
          <div
            className={`absolute top-0 bottom-0 flex flex-col justify-between z-10 ${sideOffsetClass}`}
          >
            <button
              type="button"
              disabled={!hasContentToCopy}
              className={`bg-[var(--bg-secondary)] border border-[var(--glass-border)] text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center shadow-[0_2px_4px_rgba(0,0,0,0.1)] ${copied ? "opacity-100 text-[var(--accent-primary)]" : hasContentToCopy ? "opacity-45 group-hover:opacity-100 hover:opacity-100 hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)]" : "opacity-30"} ${!hasContentToCopy ? "disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-[var(--text-muted)]" : ""}`}
              onClick={handleCopy}
              title={t.ai.copyMessage}
            >
              {copied ? (
                <Check size={14} className="text-green-500" />
              ) : (
                <Copy size={14} />
              )}
            </button>
            {msg.role === "assistant" && (
              <button
                type="button"
                disabled={!canTriggerRegenerate}
                className={`bg-[var(--bg-secondary)] border border-[var(--glass-border)] text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center shadow-[0_2px_4px_rgba(0,0,0,0.1)] ${canTriggerRegenerate ? "opacity-45 group-hover:opacity-100 hover:opacity-100 hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)]" : "opacity-30 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-[var(--text-muted)]"}`}
                onClick={onRegenerate}
                title={t.ai.regenerateMessage}
              >
                <RotateCcw size={14} />
              </button>
            )}
          </div>
        </div>
      </div>
    )
  },
)

MessageBubble.displayName = "MessageBubble"
