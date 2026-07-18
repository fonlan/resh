import { useEffect, useRef, useState } from "react"
import { AlertTriangle, Clock } from "lucide-react"
import type { ToolCall } from "../../types/ai"
import { useTranslation } from "../../i18n"
import {
  COMMAND_EXECUTION_TOOL_NAMES,
  getToolDisplayName,
  hasSensitiveToolCall,
  parseCommandExecutionToolArgs,
} from "./helpers"

interface ToolConfirmationProps {
  toolCalls: ToolCall[]
  autoConfirmDelaySeconds: number
  onConfirm: () => void
  onCancel: () => void
}

export const ToolConfirmation = ({
  toolCalls,
  autoConfirmDelaySeconds,
  onConfirm,
  onCancel,
}: ToolConfirmationProps) => {
  const { t } = useTranslation()
  const [countdown, setCountdown] = useState<number | null>(null)
  const [isSensitive, setIsSensitive] = useState(false)
  const confirmedRef = useRef(false)

  useEffect(() => {
    const requiresExplicitApproval = toolCalls.some(
      (call) => call.approval_policy === "AlwaysAsk",
    )
    const sensitive = requiresExplicitApproval || hasSensitiveToolCall(toolCalls)
    const allowsCountdown = toolCalls.every(
      (call) => call.approval_policy === "Countdown",
    )

    setIsSensitive(sensitive)
    confirmedRef.current = false
    setCountdown(allowsCountdown ? autoConfirmDelaySeconds : null)
  }, [autoConfirmDelaySeconds, toolCalls])

  useEffect(() => {
    if (countdown === null || confirmedRef.current) return

    if (countdown <= 0) {
      // Ensure we only call onConfirm once
      confirmedRef.current = true
      onConfirm()
      return
    }

    const timer = setInterval(() => {
      setCountdown((c) => {
        if (c === null) return null
        if (c <= 1) return 0
        return c - 1
      })
    }, 1000)

    return () => clearInterval(timer)
  }, [countdown, onConfirm])

  return (
    <div
      className={`flex flex-col gap-3 bg-black/20 border rounded-lg p-3 w-full box-border ${isSensitive ? "border-red-500 bg-red-500/10" : "border-[var(--border-color)]"}`}
    >
      <div className="flex items-center gap-1.5 font-semibold text-[13px] text-[var(--text-primary)]">
        {isSensitive ? (
          <>
            <AlertTriangle size={16} className="text-red-500" />{" "}
            {t.ai.tool.confirmExecution}
          </>
        ) : (
          <>
            <Clock size={16} />{" "}
            {t.ai.tool.autoExecute.replace("{seconds}", String(countdown))}
          </>
        )}
      </div>
      <div className="flex flex-col gap-2">
        {toolCalls.map((call) => {
          let displayArgs = call.function.arguments
          let timeoutSeconds: number | null = null
          let waitFinish: boolean | null = null
          if (COMMAND_EXECUTION_TOOL_NAMES.has(call.function.name)) {
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
              <span className="font-mono text-xs opacity-70">
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
            </div>
          )
        })}
      </div>
      <div className="flex justify-end gap-2 mt-1">
        <button
          type="button"
          className="px-3 py-1.5 rounded text-[12px] font-medium cursor-pointer border-0 transition-all duration-200 bg-white/10 text-[var(--text-primary)] hover:bg-white/20"
          onClick={onCancel}
        >
          {t.ai.tool.cancel}
        </button>
        <button
          type="button"
          className={`px-3 py-1.5 rounded text-[12px] font-medium cursor-pointer border-0 transition-all duration-200 ${isSensitive ? "bg-red-600 hover:bg-red-700" : "bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-hover)]"}`}
          onClick={onConfirm}
        >
          {isSensitive
            ? t.ai.tool.confirmRun
            : t.ai.tool.runNow.replace("{seconds}", String(countdown))}
        </button>
      </div>
    </div>
  )
}
