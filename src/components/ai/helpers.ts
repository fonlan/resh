import type { ChatMessage } from "../../types/ai"
import type { EditorAIContext } from "../../types"

export const SFTP_PATH_MIME_TYPE = "application/x-resh-sftp-path"
export const SFTP_ENTRY_MIME_TYPE = "application/x-resh-sftp-entry"

export interface SftpDragEntry {
  path: string
  isDir: boolean
}

export const VIRTUAL_MESSAGE_GAP_PX = 16
export const DEFAULT_RUN_IN_TERMINAL_TIMEOUT_SECONDS = 30
export const COMMAND_OUTPUT_PREVIEW_MAX_LINES = 5
export const MAX_EDITOR_CONTEXT_CHARS = 20000

export const HIDDEN_TOOL_CALL_NAMES = new Set([
  "get_terminal_output",
  "get_selected_terminal_output",
  "read_file",
])

export const COMMAND_EXECUTION_TOOL_NAMES = new Set([
  "run_in_terminal",
  "run_in_background",
])

export const getToolDisplayName = (toolName: string, t: any): string => {
  const displayNameMap: Record<string, string> = {
    get_terminal_output: t.ai.tool.getTerminalOutput,
    get_selected_terminal_output: t.ai.tool.getSelectedTerminalOutput,
    read_file: t.ai.tool.readFile,
    run_in_terminal: t.ai.tool.executeCommand,
    run_in_background: t.ai.tool.executeBackgroundCommand,
    send_interrupt: t.ai.tool.sendInterrupt,
    send_terminal_input: t.ai.tool.sendTerminalInput,
    sftp_download: t.ai.tool.sftpDownload,
    sftp_upload: t.ai.tool.sftpUpload,
  }
  return displayNameMap[toolName] || toolName
}

export const normalizeAiErrorMessage = (error: unknown): string => {
  const rawMessage =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : error === null || error === undefined
          ? ""
          : String(error)

  return rawMessage
    .replace(/^Error:\s*/i, "")
    .replace(/^Error invoking remote method '[^']+':\s*/i, "")
    .trim()
}

export const buildMessageWithEditorContext = (
  userContent: string,
  editorContext: EditorAIContext,
): string => {
  const normalizedLanguage = editorContext.language.trim() || "plaintext"
  return `${userContent}

[Current editor file context - read only]
Path: ${editorContext.remotePath}
Language: ${normalizedLanguage}
Content:
<<<RESH_EDITOR_FILE_START
${editorContext.content}
RESH_EDITOR_FILE_END>>>

Please use this file context for analysis first. Do not assume write-back changes unless explicitly requested.`
}

interface CommandExecutionToolArgs {
  command?: unknown
  timeoutSeconds?: unknown
  wait_finish?: unknown
}

export const parseCommandExecutionToolArgs = (
  rawArguments: string,
  toolName: string,
) => {
  let displayCommand = rawArguments
  let timeoutSeconds: number | null =
    toolName === "run_in_terminal" || toolName === "run_in_background"
      ? DEFAULT_RUN_IN_TERMINAL_TIMEOUT_SECONDS
      : null
  let waitFinish: boolean | null = toolName === "run_in_terminal" ? true : null

  try {
    const args = JSON.parse(rawArguments) as CommandExecutionToolArgs

    if (typeof args.command === "string" && args.command.length > 0) {
      displayCommand = args.command
    }

    if (
      timeoutSeconds !== null &&
      typeof args.timeoutSeconds === "number" &&
      Number.isFinite(args.timeoutSeconds) &&
      args.timeoutSeconds > 0
    ) {
      timeoutSeconds = Math.floor(args.timeoutSeconds)
    }

    if (
      toolName === "run_in_terminal" &&
      typeof args.wait_finish === "boolean"
    ) {
      waitFinish = args.wait_finish
    }
  } catch {}

  if (toolName === "run_in_terminal" && waitFinish === false) {
    timeoutSeconds = null
  }

  return {
    displayCommand,
    timeoutSeconds,
    waitFinish,
  }
}

export const collectAssistantToolOutputs = (
  messages: ChatMessage[],
  assistantIndex: number,
) => {
  const assistantMessage = messages[assistantIndex]
  const toolCalls = assistantMessage.tool_calls || []
  if (assistantMessage.role !== "assistant" || toolCalls.length === 0) {
    return {
      toolOutputsByCallId: {} as Record<string, string>,
      consumedToolMessageIndexes: [] as number[],
    }
  }
  const toolCallIdSet = new Set(toolCalls.map((call) => call.id))
  const commandToolCallIdSet = new Set(
    toolCalls
      .filter((call) => COMMAND_EXECUTION_TOOL_NAMES.has(call.function.name))
      .map((call) => call.id),
  )
  if (commandToolCallIdSet.size === 0) {
    return {
      toolOutputsByCallId: {} as Record<string, string>,
      consumedToolMessageIndexes: [] as number[],
    }
  }
  const outputChunksByToolCallId = new Map<string, string[]>()
  const consumedToolMessageIndexes: number[] = []
  for (let i = assistantIndex + 1; i < messages.length; i += 1) {
    const candidate = messages[i]
    if (candidate.role !== "tool") {
      continue
    }
    const toolCallId = candidate.tool_call_id?.trim()
    if (!toolCallId || !toolCallIdSet.has(toolCallId)) {
      continue
    }
    if (!commandToolCallIdSet.has(toolCallId)) {
      continue
    }
    consumedToolMessageIndexes.push(i)
    const outputText = candidate.content || ""
    if (outputText.length > 0) {
      const chunks = outputChunksByToolCallId.get(toolCallId) || []
      chunks.push(outputText)
      outputChunksByToolCallId.set(toolCallId, chunks)
    }
  }
  const toolOutputsByCallId: Record<string, string> = {}
  outputChunksByToolCallId.forEach((chunks, toolCallId) => {
    toolOutputsByCallId[toolCallId] = chunks.join("\n\n").trimEnd()
  })
  return {
    toolOutputsByCallId,
    consumedToolMessageIndexes,
  }
}
