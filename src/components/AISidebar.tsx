import React, { useState, useEffect, useRef, useMemo, useCallback, useOptimistic } from 'react';
import { useAIStore } from '../stores/useAIStore';
import { useConfig } from '../hooks/useConfig';
import { aiService } from '../services/aiService';
import { useTranslation } from '../i18n';
import ReactMarkdown, { type Components } from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { useVirtualizer } from '@tanstack/react-virtual';
import { X, Send, Lock, LockOpen, Plus, History, Bot, Copy, Terminal, Check, AlertTriangle, Clock, Sliders, Sparkles, MessageSquare, Trash2, ChevronDown, ChevronRight, BrainCircuit, Square, RotateCcw } from 'lucide-react';
import { listen } from '@tauri-apps/api/event';
import { ToolCall, ChatMessage } from '../types/ai';
import { ConfirmationModal } from './ConfirmationModal';
import { CustomSelect } from './CustomSelect';
import { EmojiText } from './EmojiText';

const SFTP_PATH_MIME_TYPE = 'application/x-resh-sftp-path'
const SFTP_ENTRY_MIME_TYPE = 'application/x-resh-sftp-entry'

interface SftpDragEntry {
  path: string
  isDir: boolean
}

interface AISidebarProps {
  isOpen: boolean;
  onClose: () => void;
  isLocked: boolean;
  onToggleLock: () => void;
  onShowToast?: (message: string, type?: 'success' | 'error' | 'info' | 'warning', duration?: number) => void;
  currentServerId?: string;
  currentTabId?: string;
  zIndex?: number;
}

interface OptimisticMessageInput {
  sessionId: string;
  message: ChatMessage;
}

interface RenderableMessage {
  msg: ChatMessage;
  sourceIndex: number;
  isPending: boolean;
  modelName: string | null;
}

const MARKDOWN_REMARK_PLUGINS = [remarkGfm]
const VIRTUAL_MESSAGE_GAP_PX = 16
const MESSAGE_VIRTUALIZATION_THRESHOLD = 40

const MESSAGE_BUBBLE_PERF_STYLE: React.CSSProperties = {
  contentVisibility: 'auto',
  containIntrinsicSize: '180px'
}

const MARKDOWN_COMPONENTS: Components = {
  p({ className, children, ...props }) {
    return (
      <p
        {...props}
        className={`my-2 leading-6 whitespace-pre-wrap ${className ?? ''}`}
      >
        {children}
      </p>
    )
  },
  h1({ className, children, ...props }) {
    return (
      <h1
        {...props}
        className={`mt-3 mb-2 text-[18px] leading-[1.3] font-semibold ${className ?? ''}`}
      >
        {children}
      </h1>
    )
  },
  h2({ className, children, ...props }) {
    return (
      <h2
        {...props}
        className={`mt-3 mb-2 text-[16px] leading-[1.35] font-semibold ${className ?? ''}`}
      >
        {children}
      </h2>
    )
  },
  h3({ className, children, ...props }) {
    return (
      <h3
        {...props}
        className={`mt-2.5 mb-1.5 text-[14px] leading-[1.4] font-semibold ${className ?? ''}`}
      >
        {children}
      </h3>
    )
  },
  ul({ className, children, ...props }) {
    return (
      <ul
        {...props}
        className={`my-2 ml-5 list-disc space-y-1 ${className ?? ''}`}
      >
        {children}
      </ul>
    )
  },
  ol({ className, children, ...props }) {
    return (
      <ol
        {...props}
        className={`my-2 ml-5 list-decimal space-y-1 ${className ?? ''}`}
      >
        {children}
      </ol>
    )
  },
  li({ className, children, ...props }) {
    return (
      <li
        {...props}
        className={`leading-6 ${className ?? ''}`}
      >
        {children}
      </li>
    )
  },
  blockquote({ className, children, ...props }) {
    return (
      <blockquote
        {...props}
        className={`my-2 border-l-[3px] border-[var(--accent-primary)] bg-black/10 px-3 py-2 italic text-[var(--text-secondary)] ${className ?? ''}`}
      >
        {children}
      </blockquote>
    )
  },
  a({ className, children, ...props }) {
    return (
      <a
        {...props}
        className={`underline underline-offset-2 text-[var(--accent-primary)] hover:opacity-85 transition-opacity ${className ?? ''}`}
      >
        {children}
      </a>
    )
  },
  hr({ className, ...props }) {
    return (
      <hr
        {...props}
        className={`my-3 border-0 border-t border-[var(--glass-border)] ${className ?? ''}`}
      />
    )
  },
  strong({ className, children, ...props }) {
    return (
      <strong
        {...props}
        className={`font-semibold ${className ?? ''}`}
      >
        {children}
      </strong>
    )
  },
  em({ className, children, ...props }) {
    return (
      <em
        {...props}
        className={`italic ${className ?? ''}`}
      >
        {children}
      </em>
    )
  },
  pre({ children }: { children?: React.ReactNode }) {
    return <>{children}</>
  },
  code({ className, children, ...props }) {
    const markdownCodeProps = props as React.ComponentPropsWithoutRef<'code'> & { inline?: boolean }
    const { inline, ...codeProps } = markdownCodeProps
    const codeContent = (() => {
      if (typeof children === 'string') return children
      if (Array.isArray(children)) return children.join('')
      return String(children ?? '')
    })()
    const hasLanguageClass = !!className?.includes('language-')
    const shouldRenderBlock = (inline === false) || hasLanguageClass || codeContent.includes('\n')

    if (shouldRenderBlock) {
      return (
        <CodeBlock className={className}>
          {children}
        </CodeBlock>
      )
    }

    return (
      <code
        {...codeProps}
        className={`bg-transparent text-[var(--text-primary)] font-inherit text-inherit p-0 px-1 italic opacity-90 rounded border border-[var(--glass-border)] ${className ?? ''}`}
      >
        {children}
      </code>
    )
  },
  table({ className, children, ...props }: React.ComponentPropsWithoutRef<'table'>) {
    return (
      <div className="my-2 w-full overflow-x-auto">
        <table
          {...props}
          className={`w-full min-w-max border-collapse border border-[var(--glass-border)] text-[12.5px] ${className ?? ''}`}
        >
          {children}
        </table>
      </div>
    )
  },
  thead({ className, children, ...props }: React.ComponentPropsWithoutRef<'thead'>) {
    return (
      <thead
        {...props}
        className={`bg-black/20 ${className ?? ''}`}
      >
        {children}
      </thead>
    )
  },
  tr({ className, children, ...props }: React.ComponentPropsWithoutRef<'tr'>) {
    return (
      <tr
        {...props}
        className={`${className ?? ''}`}
      >
        {children}
      </tr>
    )
  },
  th({ className, children, ...props }: React.ComponentPropsWithoutRef<'th'>) {
    return (
      <th
        {...props}
        className={`border border-[var(--glass-border)] px-2.5 py-1.5 text-left align-top font-semibold ${className ?? ''}`}
      >
        {children}
      </th>
    )
  },
  td({ className, children, ...props }: React.ComponentPropsWithoutRef<'td'>) {
    return (
      <td
        {...props}
        className={`border border-[var(--glass-border)] px-2.5 py-1.5 align-top ${className ?? ''}`}
      >
        {children}
      </td>
    )
  }
}

const HIDDEN_TOOL_CALL_NAMES = new Set([
  'get_terminal_output',
  'get_selected_terminal_output',
  'read_file'
])

const normalizeAiErrorMessage = (error: unknown): string => {
  const rawMessage =
    typeof error === 'string'
      ? error
      : error instanceof Error
        ? error.message
        : error === null || error === undefined
          ? ''
          : String(error)

  return rawMessage
    .replace(/^Error:\s*/i, '')
    .replace(/^Error invoking remote method '[^']+':\s*/i, '')
    .trim()
}

const CodeBlock = ({ children, className }: { children: React.ReactNode, className?: string }) => {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  
  const codeContent = (() => {
    if (typeof children === 'string') return children;
    if (Array.isArray(children)) return children.join('');
    return String(children);
  })().replace(/\n$/, '');

  const language = className ? className.replace('language-', '') : 'text';

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(codeContent);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      // Failed to copy
    }
  };

  const handleInsert = () => {
    window.dispatchEvent(new CustomEvent('paste-snippet', { detail: codeContent }));
  };

  return (
    <div className="my-2 rounded-md overflow-hidden bg-black/30 border border-[var(--glass-border)]">
      <div className="flex justify-between items-center px-3 py-1.5 bg-white/5 border-b border-[var(--glass-border)]">
        <span className="text-[11px] text-[var(--text-muted)] uppercase font-mono">{language}</span>
        <div className="flex gap-1">
          <button
            type="button"
            className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded flex items-center justify-center transition-all duration-200 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
            onClick={handleCopy}
            title={t.ai.tool.copyCode}
          >
            {copied ? <Check size={14} className="text-green-500" /> : <Copy size={14} />}
          </button>
          <button
            type="button"
            className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded flex items-center justify-center transition-all duration-200 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
            onClick={handleInsert}
            title={t.ai.tool.insertToTerminal}
          >
            <Terminal size={14} />
          </button>
        </div>
      </div>
      <pre className="m-0 p-3 overflow-x-auto font-mono text-[12px]">
        <code className={className}>{children}</code>
      </pre>
    </div>
  );
};

const ToolConfirmation = ({ 
  toolCalls, 
  onConfirm, 
  onCancel 
}: { 
  toolCalls: ToolCall[], 
  onConfirm: () => void, 
  onCancel: () => void 
}) => {
  const { t } = useTranslation();
  const [countdown, setCountdown] = useState<number | null>(null);
  const [isSensitive, setIsSensitive] = useState(false);
  const confirmedRef = useRef(false);
  
  useEffect(() => {
    // Dangerous commands that should always require confirmation
    const alwaysDangerous = /\b(rm|dd|mkfs|fdisk|reboot|shutdown|halt|poweroff|init)\b/;
    
    // Potentially dangerous commands (chmod, kill, etc.)
    const potentiallyDangerous = /\b(mv|chmod|chown|chgrp|systemctl|service|kill|pkill|killall)\b/;
    
    // Commands that are dangerous when piped to shell (curl xxx | bash)
    const dangerousWhenPiped = /\b(curl|wget)\b.*\|.*\b(bash|sh|zsh|fish|python|perl|ruby)\b/;
    
    let sensitive = false;

    toolCalls.forEach(call => {
      if (call.function.name === 'run_in_terminal') {
        try {
          const args = JSON.parse(call.function.arguments);
          if (args.command) {
            const originalCommand = args.command;
            
            // Remove safe redirections: 2>/dev/null, >/dev/null, &>/dev/null, 2>&1, 1>&2, etc.
            let cleanCommand = originalCommand.replace(/(?:[0-9&]+)?>>?\s*\/dev\/null/g, ' ');
            cleanCommand = cleanCommand.replace(/[0-9]+>&[0-9]+/g, ' ');
            
            // Check for always dangerous commands
            if (alwaysDangerous.test(cleanCommand)) {
              sensitive = true;
            } 
            // Check for potentially dangerous commands
            else if (potentiallyDangerous.test(cleanCommand)) {
              sensitive = true;
            }
            // Check for curl/wget piped to shell
            else if (dangerousWhenPiped.test(cleanCommand)) {
              sensitive = true;
            }
          }
        } catch (e) {
          sensitive = true;
        }
      }
    });

    setIsSensitive(sensitive);
    
    // Auto-execute if NOT sensitive
    if (!sensitive) {
      setCountdown(5);
    }
  }, [toolCalls]);

  useEffect(() => {
    if (countdown === null || confirmedRef.current) return;
    
    if (countdown <= 0) {
      // Ensure we only call onConfirm once
      confirmedRef.current = true;
      onConfirm();
      return;
    }

    const timer = setInterval(() => {
      setCountdown(c => {
        if (c === null) return null;
        if (c <= 1) return 0;
        return c - 1;
      });
    }, 1000);
    
    return () => clearInterval(timer);
  }, [countdown, onConfirm]); // Added onConfirm dependency

  return (
    <div className={`flex flex-col gap-3 bg-black/20 border rounded-lg p-3 w-full box-border ${isSensitive ? 'border-red-500 bg-red-500/10' : 'border-[var(--border-color)]'}`}>
      <div className="flex items-center gap-1.5 font-semibold text-[13px] text-[var(--text-primary)]">
        {isSensitive ? (
          <><AlertTriangle size={16} className="text-red-500" /> {t.ai.tool.confirmExecution}</>
        ) : (
          <><Clock size={16} /> {t.ai.tool.autoExecute.replace('{seconds}', String(countdown))}</>
        )}
      </div>
      <div className="flex flex-col gap-2">
        {toolCalls.map(call => {
          let displayArgs = call.function.arguments;
          if (call.function.name === 'run_in_terminal') {
             try {
               const args = JSON.parse(call.function.arguments);
               displayArgs = args.command || displayArgs;
             } catch {}
          }
          return (
            <div key={call.id} className="bg-black/20 p-2 rounded-md border border-white/10 overflow-hidden">
              <span className="font-mono text-xs opacity-70">
                {call.function.name === 'run_in_terminal' ? t.ai.tool.executeCommand : call.function.name}
              </span>
              <code className="block mt-1 text-sm bg-black/20 p-1 rounded">{displayArgs}</code>
            </div>
          );
        })}
      </div>
      <div className="flex justify-end gap-2 mt-1">
        <button type="button" className="px-3 py-1.5 rounded text-[12px] font-medium cursor-pointer border-0 transition-all duration-200 bg-white/10 text-[var(--text-primary)] hover:bg-white/20" onClick={onCancel}>{t.ai.tool.cancel}</button>
        <button
          type="button"
          className={`px-3 py-1.5 rounded text-[12px] font-medium cursor-pointer border-0 transition-all duration-200 ${isSensitive ? 'bg-red-600 hover:bg-red-700' : 'bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-hover)]'}`}
          onClick={onConfirm}
        >
          {isSensitive ? t.ai.tool.confirmRun : t.ai.tool.runNow.replace('{seconds}', String(countdown))}
        </button>
      </div>
    </div>
  );
};

const MessageBubble = React.memo(({
  msg,
  t,
  isPending,
  isLast,
  isStreaming,
  modelName,
  canRegenerate,
  onRegenerate
}: {
  msg: ChatMessage,
  t: any,
  isPending?: boolean,
  isLast?: boolean,
  isStreaming?: boolean,
  modelName: string | null,
  canRegenerate?: boolean,
  onRegenerate?: () => void
}) => {
  const [copied, setCopied] = useState(false);
  const [showReasoning, setShowReasoning] = useState(true);
  const reasoningContentRef = useRef<HTMLDivElement>(null);
  const prevReasoningLength = useRef(msg.reasoning_content?.length || 0);

  const timeString = useMemo(() => {
    if (!msg.created_at) return '';
    try {
      // SQLite CURRENT_TIMESTAMP is UTC "YYYY-MM-DD HH:MM:SS"
      // We append 'Z' to ensure it's treated as UTC
      const dateStr = msg.created_at.endsWith('Z') ? msg.created_at : msg.created_at.replace(' ', 'T') + 'Z';
      const date = new Date(dateStr);
      return date.toLocaleString([], { 
        year: 'numeric', 
        month: '2-digit', 
        day: '2-digit', 
        hour: '2-digit', 
        minute: '2-digit' 
      });
    } catch {
      return '';
    }
  }, [msg.created_at]);

  useEffect(() => {
    if (showReasoning && reasoningContentRef.current) {
      const element = reasoningContentRef.current;
      const currentLength = msg.reasoning_content?.length || 0;

      if (currentLength >= prevReasoningLength.current) {
        requestAnimationFrame(() => {
          element.scrollTop = element.scrollHeight;
        });
      }

      prevReasoningLength.current = currentLength;
    }
  }, [msg.reasoning_content, showReasoning]);

  const handleCopy = async () => {
    if (!msg.content) return;
    try {
      await navigator.clipboard.writeText(msg.content);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      // Failed to copy
    }
  };

  const visibleToolCalls = useMemo(() => {
    if (!msg.tool_calls || isPending) return [];
    return msg.tool_calls.filter(call => !HIDDEN_TOOL_CALL_NAMES.has(call.function.name))
  }, [isPending, msg.tool_calls]);

  const hasContentToCopy = !!(msg.content && msg.content.trim().length > 0)
  const renderPlainStreamingContent = !!(isStreaming && msg.role === 'assistant')
  const canTriggerRegenerate = msg.role === 'assistant' && !!canRegenerate
  const sideOffsetClass = msg.role === 'user' ? '-left-8' : '-right-8'

  return (
    <div
      className={`flex flex-col gap-1 max-w-full ${msg.role === 'user' ? 'items-end' : 'items-start'}`}
      style={MESSAGE_BUBBLE_PERF_STYLE}
    >
      <div className={`relative max-w-[90%] flex flex-col group ${msg.role === 'user' ? 'items-end' : 'items-start'}`}>
        <div className={`p-2 px-3 rounded-lg text-[13px] leading-[1.5] w-full break-words overflow-x-auto ${msg.role === 'user' ? 'ai-user-message-bubble bg-[var(--accent-primary)] text-white rounded-tr-sm selection:bg-white/55' : 'bg-[var(--bg-tertiary)] text-[var(--text-primary)] rounded-tl-sm'}`}>
          {msg.reasoning_content && (
            <div className="w-full mb-2">
              <button
                type="button"
                className="flex items-center bg-transparent border-0 text-[var(--text-muted)] text-[12px] cursor-pointer p-1 transition-colors duration-200 hover:text-[var(--accent-primary)]"
                onClick={() => setShowReasoning(!showReasoning)}
              >
                {showReasoning ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                <BrainCircuit size={14} className="ml-1 mr-1 text-blue-400" />
                <span>{t.ai.thinkingProcess}</span>
              </button>
              {showReasoning && (
                <div className="leading-[1.6] max-h-[400px] overflow-y-auto font-serif italic bg-black/15 border-l-3 border-[var(--accent-primary)] rounded px-3.5 py-2.5 mt-1.5 text-[var(--text-muted)] text-[12.5px] shadow-inset relative whitespace-pre-wrap" ref={reasoningContentRef}>
                  {msg.reasoning_content}
                  {isLast && isStreaming && !msg.content && <span className="inline-block w-[2px] ml-0.5 text-[var(--accent-primary)] animate-cursor-blink vertical-align-middle font-bold">|</span>}
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
                  let displayArgs = call.function.arguments;
                  if (call.function.name === 'run_in_terminal') {
                     try {
                       const args = JSON.parse(call.function.arguments);
                       displayArgs = args.command || displayArgs;
                     } catch {}
                  }
                   return (
                     <div key={call.id} className="bg-black/20 p-2 rounded-md border border-white/10 overflow-hidden">
                       <span className="font-mono text-xs opacity-70 block">
                        {call.function.name === 'run_in_terminal' ? t.ai.tool.executeCommand : call.function.name}
                      </span>
                      <code className="block mt-1 text-sm bg-black/20 p-1 rounded break-all">{displayArgs}</code>
                    </div>
                  );
                })}
              </div>
            </div>
          ) : null}
          {msg.content && (
            renderPlainStreamingContent ? (
              <div className="whitespace-pre-wrap break-words">{msg.content}</div>
            ) : (
              <ReactMarkdown
                remarkPlugins={MARKDOWN_REMARK_PLUGINS}
                components={MARKDOWN_COMPONENTS}
              >
                {msg.content}
              </ReactMarkdown>
            )
          )}
          <div className={`flex items-center gap-2 mt-1 text-[10px] select-none ${msg.role === 'user' ? 'text-white/60 justify-end' : 'text-[var(--text-muted)] justify-between'}`}>
            {modelName && <span>{modelName}</span>}
            {timeString && <span>{timeString}</span>}
          </div>
        </div>
        <div className={`absolute top-0 bottom-0 flex flex-col justify-between z-10 ${sideOffsetClass}`}>
          <button
            type="button"
            disabled={!hasContentToCopy}
            className={`bg-[var(--bg-secondary)] border border-[var(--glass-border)] text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center shadow-[0_2px_4px_rgba(0,0,0,0.1)] ${copied ? 'opacity-100 text-[var(--accent-primary)]' : hasContentToCopy ? 'opacity-45 group-hover:opacity-100 hover:opacity-100 hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)]' : 'opacity-30'} ${!hasContentToCopy ? 'disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-[var(--text-muted)]' : ''}`}
            onClick={handleCopy}
            title={t.ai.copyMessage}
          >
            {copied ? <Check size={14} className="text-green-500" /> : <Copy size={14} />}
          </button>
          {msg.role === 'assistant' && (
            <button
              type="button"
              disabled={!canTriggerRegenerate}
              className={`bg-[var(--bg-secondary)] border border-[var(--glass-border)] text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center shadow-[0_2px_4px_rgba(0,0,0,0.1)] ${canTriggerRegenerate ? 'opacity-45 group-hover:opacity-100 hover:opacity-100 hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)]' : 'opacity-30 disabled:cursor-not-allowed disabled:hover:bg-transparent disabled:hover:text-[var(--text-muted)]'}`}
              onClick={onRegenerate}
              title={t.ai.regenerateMessage}
            >
              <RotateCcw size={14} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
})

MessageBubble.displayName = 'MessageBubble'

export const AISidebar: React.FC<AISidebarProps> = ({
  isOpen,
  onClose,
  isLocked,
  onToggleLock,
  onShowToast,
  currentServerId,
  currentTabId,
  zIndex
}) => {
  const { t } = useTranslation();
  const { config, saveConfig } = useConfig();
  const sessions = useAIStore(state => state.sessions)
  const activeSessionId = useAIStore(state => state.activeSessionId)
  const activeSessionIdByServer = useAIStore(state => state.activeSessionIdByServer)
  const activeSessionIdBySshSession = useAIStore(state => state.activeSessionIdBySshSession)
  const messages = useAIStore(state => state.messages)
  const isGenerating = useAIStore(state => state.isGenerating)
  const pendingToolCallsMap = useAIStore(state => state.pendingToolCalls)
  const loadSessions = useAIStore(state => state.loadSessions)
  const createSession = useAIStore(state => state.createSession)
  const selectSession = useAIStore(state => state.selectSession)
  const addMessage = useAIStore(state => state.addMessage)
  const newAssistantMessage = useAIStore(state => state.newAssistantMessage)
  const appendResponse = useAIStore(state => state.appendResponse)
  const appendReasoning = useAIStore(state => state.appendReasoning)
  const appendToolCalls = useAIStore(state => state.appendToolCalls)
  const setGenerating = useAIStore(state => state.setGenerating)
  const storeSetPendingToolCalls = useAIStore(state => state.setPendingToolCalls)
  const markSessionStopped = useAIStore(state => state.markSessionStopped)
  const clearSessionStopped = useAIStore(state => state.clearSessionStopped)
  const deleteSession = useAIStore(state => state.deleteSession)
  const clearSessions = useAIStore(state => state.clearSessions)
  const addCompleteMessage = useAIStore(state => state.addCompleteMessage)
  const removeLatestAssistantMessage = useAIStore(state => state.removeLatestAssistantMessage)

  const [width, setWidth] = useState(350);
  const [isResizing, setIsResizing] = useState(false);
  const [inputValue, setInputValue] = useState('');
  const [isInputDragOver, setIsInputDragOver] = useState(false)
  const [showHistory, setShowHistory] = useState(false);
  const [mode, setMode] = useState<'ask' | 'agent'>(config?.general.aiMode as 'ask' | 'agent' || 'ask');
  const [selectedModelId, setSelectedModelId] = useState<string>('');
  const [sessionToDelete, setSessionToDelete] = useState<string | null>(null);
  const [isClearingHistory, setIsClearingHistory] = useState(false);

  const [optimisticMessagesBySession, addOptimisticMessage] = useOptimistic(
    messages,
    (currentMessagesBySession, payload: OptimisticMessageInput) => {
      const currentSessionMessages = currentMessagesBySession[payload.sessionId] || [];
      return {
        ...currentMessagesBySession,
        [payload.sessionId]: [...currentSessionMessages, payload.message]
      };
    }
  );

  const isLoading = activeSessionId ? isGenerating[activeSessionId] || false : false;
  const pendingToolCalls = activeSessionId ? pendingToolCallsMap[activeSessionId] || null : null;
  const currentSession = sessions.find(s => s.id === activeSessionId);
  const boundSshSessionId = currentSession?.sshSessionId || currentTabId;

  const sidebarRef = useRef<HTMLDivElement>(null);
  const messagesContainerRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const isAtBottomRef = useRef(true);
  const responseChunkBufferRef = useRef('')
  const reasoningChunkBufferRef = useRef('')
  const streamFlushTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const lastErrorToastRef = useRef<{ message: string, at: number } | null>(null)

  const showAiError = useCallback((error: unknown) => {
    const normalizedError = normalizeAiErrorMessage(error)
    const fallbackMessage = t.ai.unknownError || 'Unknown error'
    const detailMessage = normalizedError || fallbackMessage
    const template = t.ai.requestFailed || 'AI request failed: {error}'
    const finalMessage = template.includes('{error}')
      ? template.replace('{error}', detailMessage)
      : `${template} ${detailMessage}`

    const now = Date.now()
    const previous = lastErrorToastRef.current
    if (previous && previous.message === finalMessage && now - previous.at < 1500) {
      return
    }

    lastErrorToastRef.current = { message: finalMessage, at: now }
    onShowToast?.(finalMessage, 'error')
  }, [onShowToast, t])

  // Load mode & model from config, with fallback to default model
  useEffect(() => {
    if (config?.general.aiMode) {
      setMode(config.general.aiMode as 'ask' | 'agent');
    }
    if (config?.general.aiModelId) {
      setSelectedModelId(config.general.aiModelId);
    } else if (config?.aiModels && config.aiModels.length > 0 && !selectedModelId) {
      const firstEnabledModel = config.aiModels.find(model => {
        const channel = config.aiChannels?.find(c => c.id === model.channelId);
        return model.enabled && channel?.isActive;
      });
      if (firstEnabledModel) {
        setSelectedModelId(firstEnabledModel.id);
      }
    }
  }, [config?.general.aiMode, config?.general.aiModelId, config?.aiModels, config?.aiChannels, selectedModelId]);

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
      ? (
          Object.prototype.hasOwnProperty.call(activeSessionIdBySshSession, currentTabId)
            ? (activeSessionIdBySshSession[currentTabId] ?? null)
            : (sessions.find(session => session.sshSessionId === currentTabId)?.id ?? null)
        )
      : null

    const targetSessionId = currentTabId ? sessionIdFromCurrentTab : fallbackSessionId
    if (targetSessionId !== activeSessionId) {
      if (currentTabId) {
        void selectSession(targetSessionId, targetSessionId ? currentServerId : undefined, currentTabId)
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

  const currentMessages = activeSessionId ? optimisticMessagesBySession[activeSessionId] || [] : [];
  const modelNameById = useMemo(() => {
    if (!config?.aiModels?.length) {
      return {} as Record<string, string>
    }

    return config.aiModels.reduce((acc, model) => {
      acc[model.id] = model.name
      return acc
    }, {} as Record<string, string>)
  }, [config?.aiModels])

  const pendingToolCallIdSet = useMemo(() => {
    if (!pendingToolCalls?.length) {
      return new Set<string>()
    }

    return new Set(pendingToolCalls.map(call => call.id))
  }, [pendingToolCalls])

  const renderableMessages = useMemo<RenderableMessage[]>(() => {
    if (currentMessages.length === 0) {
      return []
    }

    const nextMessages: RenderableMessage[] = []

    currentMessages.forEach((msg, sourceIndex) => {
      if (msg.role === 'assistant') {
        const hasContent = !!(msg.content && msg.content.trim().length > 0)
        const hasReasoning = !!(msg.reasoning_content && msg.reasoning_content.trim().length > 0)
        const hasVisibleTools = !!msg.tool_calls?.some(tc => !HIDDEN_TOOL_CALL_NAMES.has(tc.function.name))

        if (!hasContent && !hasVisibleTools && !hasReasoning) {
          return
        }
      }

      const isPending = !!(
        pendingToolCallIdSet.size &&
        msg.tool_calls?.some(tc => pendingToolCallIdSet.has(tc.id))
      )

      const modelName = msg.role === 'assistant' && msg.model_id
        ? (modelNameById[msg.model_id] || msg.model_id)
        : null

      nextMessages.push({
        msg,
        sourceIndex,
        isPending,
        modelName
      })
    })

    return nextMessages
  }, [currentMessages, modelNameById, pendingToolCallIdSet])
  const latestAssistantMessageSourceIndex = useMemo(() => {
    for (let i = renderableMessages.length - 1; i >= 0; i -= 1) {
      const candidate = renderableMessages[i]
      if (candidate.msg.role === 'assistant') {
        return candidate.sourceIndex
      }
    }

    return null
  }, [renderableMessages])
  const canRegenerateLatestAssistant = !!activeSessionId && !isLoading && !pendingToolCalls && latestAssistantMessageSourceIndex !== null
  const shouldUseVirtualizedMessages = renderableMessages.length > MESSAGE_VIRTUALIZATION_THRESHOLD
  const messageVirtualizer = useVirtualizer({
    enabled: shouldUseVirtualizedMessages,
    count: renderableMessages.length,
    getScrollElement: () => messagesContainerRef.current,
    estimateSize: () => 188,
    overscan: 8
  })
  const virtualMessageItems = messageVirtualizer.getVirtualItems()
  const virtualizedTotalHeight = messageVirtualizer.getTotalSize()

  const scrollToBottom = useCallback((behavior: ScrollBehavior = 'auto') => {
    if (messagesContainerRef.current) {
      const container = messagesContainerRef.current;
      if (behavior === 'smooth') {
        container.scrollTo({ top: container.scrollHeight, behavior: 'smooth' });
      } else {
        container.scrollTop = container.scrollHeight;
      }
    }
  }, []);

  const handleScroll = useCallback(() => {
    if (messagesContainerRef.current) {
      const { scrollTop, scrollHeight, clientHeight } = messagesContainerRef.current;
      const atBottom = scrollHeight - scrollTop - clientHeight < 50;
      isAtBottomRef.current = atBottom;
    }
  }, []);

  const lastMessageContentLength = useMemo(() => {
    const lastMsg = currentMessages[currentMessages.length - 1];
    if (lastMsg?.role === 'assistant') {
      const contentLength = (lastMsg.content?.length || 0) + (lastMsg.reasoning_content?.length || 0);
      const toolCallsCount = lastMsg.tool_calls?.length || 0;
      // Include tool calls count to trigger scroll when tool call cards appear
      return contentLength + toolCallsCount;
    }
    return 0;
  }, [currentMessages]);

  useEffect(() => {
    const shouldScroll = isAtBottomRef.current || isLoading;
    if (shouldScroll && lastMessageContentLength >= 0) {
      const rafId = requestAnimationFrame(() => {
        scrollToBottom(isLoading ? 'auto' : 'smooth');
      });
      return () => cancelAnimationFrame(rafId);
    }
  }, [lastMessageContentLength, isLoading, scrollToBottom]);

  // Focus textarea when sidebar opens or when tools are resolved
  useEffect(() => {
    if (isOpen && !pendingToolCalls) {
      textareaRef.current?.focus();
    }
  }, [isOpen, pendingToolCalls]);

  // Auto-resize textarea
  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
      if (inputValue) {
        textareaRef.current.style.height = Math.min(textareaRef.current.scrollHeight, 150) + 'px';
      } else {
        textareaRef.current.style.height = '28px';
      }
    }
  }, [inputValue]);
  
  const handleDeleteSession = async (sessionId: string) => {
    if (currentServerId) {
      try {
        await deleteSession(currentServerId, sessionId);
        setSessionToDelete(null);
      } catch (err) {
        // Failed to delete session
      }
    }
  };

  const handleClearHistory = async () => {
    if (currentServerId) {
      try {
        await clearSessions(currentServerId);
        setIsClearingHistory(false);
      } catch (err) {
        // Failed to clear history
      }
    }
  };

  // Listen for streaming responses & tool calls
  useEffect(() => {
    if (!activeSessionId) return;

    const flushStreamBuffers = () => {
      if (responseChunkBufferRef.current) {
        appendResponse(activeSessionId, responseChunkBufferRef.current)
        responseChunkBufferRef.current = ''
      }

      if (reasoningChunkBufferRef.current) {
        appendReasoning(activeSessionId, reasoningChunkBufferRef.current)
        reasoningChunkBufferRef.current = ''
      }
    }

    const scheduleStreamFlush = () => {
      if (streamFlushTimerRef.current) return
      streamFlushTimerRef.current = setTimeout(() => {
        flushStreamBuffers()
        streamFlushTimerRef.current = null
      }, 33)
    }

    const startedListener = listen<string>(`ai-started-${activeSessionId}`, () => {
      storeSetPendingToolCalls(activeSessionId, null);
      newAssistantMessage(activeSessionId, selectedModelId);
    });

    // Handle MessageBatch events (minimax-style multiple messages in one response)
    const messageBatchListener = listen<ChatMessage[]>(`ai-message-batch-${activeSessionId}`, (event) => {
      const messages = event.payload;
      // Add each message as a separate bubble
      messages.forEach(msg => {
        addCompleteMessage(activeSessionId, msg);
      });
    });

    const responseListener = listen<string>(`ai-response-${activeSessionId}`, (event) => {
      responseChunkBufferRef.current += event.payload
      scheduleStreamFlush()
    });

    const reasoningListener = listen<string>(`ai-reasoning-${activeSessionId}`, (event) => {
      reasoningChunkBufferRef.current += event.payload
      scheduleStreamFlush()
    });

    const toolCallListener = listen<ToolCall[]>(`ai-tool-call-${activeSessionId}`, (event) => {
      const calls = event.payload;

      // Update store with tool calls so they appear in the message bubble
      appendToolCalls(activeSessionId, calls);

      // Filter: if ALL calls are read-only tools, auto-execute immediately WITHOUT UI
      const isAllSafe = calls.every(
        c =>
          c.function.name === 'get_terminal_output' ||
          c.function.name === 'get_selected_terminal_output' ||
          c.function.name === 'read_file'
      )

      if (isAllSafe) {
        // Ensure we are using valid IDs
        const model = config?.aiModels.find(m => m.id === selectedModelId);
        if (!model) {
             setGenerating(activeSessionId, false);
             return;
        }
        const channelId = model.channelId || '';

        aiService.executeAgentTools(
          activeSessionId,
          selectedModelId,
          channelId,
          mode,
          boundSshSessionId,
          calls.map(c => c.id)
        ).catch((err) => {
          setGenerating(activeSessionId, false);
          showAiError(err);
        });
        
        // Do NOT set pendingToolCalls
        setGenerating(activeSessionId, true); 
      } else {
        // Here we have tool calls that require confirmation.
        // We set pendingToolCalls to show the UI.
        // The UI (ToolConfirmation) handles the countdown for non-sensitive commands.
        setGenerating(activeSessionId, false);
        storeSetPendingToolCalls(activeSessionId, calls);
      }
    });

    const errorListener = listen<string>(`ai-error-${activeSessionId}`, (event) => {
      flushStreamBuffers()
      setGenerating(activeSessionId, false);
      storeSetPendingToolCalls(activeSessionId, null);
      showAiError(event.payload);
    });

    const doneListener = listen<string>(`ai-done-${activeSessionId}`, async () => {
      flushStreamBuffers()
      setGenerating(activeSessionId, false);

      // Auto-generate title for new sessions after first response
      const currentSession = sessions.find(s => s.id === activeSessionId);
      if (currentSession && currentSession.title === 'New Chat') {
        try {
          const model = config?.aiModels.find(m => m.id === selectedModelId);
          const channelId = model?.channelId || '';
          
          await aiService.generateTitle(activeSessionId, selectedModelId, channelId);
          
          // Reload sessions to get the updated title
          if (currentServerId) {
            await loadSessions(currentServerId);
          }
        } catch (err) {
          // Failed to generate title
        }
      }
    });

    return () => {
      if (streamFlushTimerRef.current) {
        clearTimeout(streamFlushTimerRef.current)
        streamFlushTimerRef.current = null
      }
      flushStreamBuffers()
      startedListener.then(unlisten => unlisten());
      messageBatchListener.then(unlisten => unlisten());
      responseListener.then(unlisten => unlisten());
      reasoningListener.then(unlisten => unlisten());
      toolCallListener.then(unlisten => unlisten());
      errorListener.then(unlisten => unlisten());
      doneListener.then(unlisten => unlisten());
    };
  }, [activeSessionId, addCompleteMessage, appendResponse, appendReasoning, appendToolCalls, newAssistantMessage, setGenerating, storeSetPendingToolCalls, config, selectedModelId, mode, boundSshSessionId, sessions, currentServerId, loadSessions, showAiError]);

  // Resizing logic
  const startResizing = (e: React.MouseEvent) => {
    e.preventDefault();
    setIsResizing(true);
  };

  useEffect(() => {
    const stopResizing = () => setIsResizing(false);
    const resize = (e: MouseEvent) => {
      if (isResizing) {
        const newWidth = window.innerWidth - e.clientX;
        if (newWidth >= 250 && newWidth <= 800) {
          setWidth(newWidth);
        }
      }
    };

    if (isResizing) {
      window.addEventListener('mousemove', resize);
      window.addEventListener('mouseup', stopResizing);
      window.addEventListener('pointerup', stopResizing);
      window.addEventListener('blur', stopResizing);
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
    }

    return () => {
      window.removeEventListener('mousemove', resize);
      window.removeEventListener('mouseup', stopResizing);
      window.removeEventListener('pointerup', stopResizing);
      window.removeEventListener('blur', stopResizing);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
  }, [isResizing]);

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

  const appendPathToInput = useCallback((path: string, useReadFilePrefix: boolean) => {
    const token = `${useReadFilePrefix ? '#' : ''}${path}`
    setInputValue(prev => {
      const normalized = prev.trimEnd()
      const merged = normalized.length > 0 ? `${normalized} ${token}` : token
      return `${merged} `
    })

    if (textareaRef.current) {
      textareaRef.current.focus()
    }
  }, [])

  const handleInputDragOver = useCallback((e: React.DragEvent<HTMLTextAreaElement>) => {
    if (
      !e.dataTransfer.types.includes(SFTP_PATH_MIME_TYPE) &&
      !e.dataTransfer.types.includes(SFTP_ENTRY_MIME_TYPE)
    ) {
      return
    }

    e.preventDefault()
    e.dataTransfer.dropEffect = 'copy'
    setIsInputDragOver(true)
  }, [])

  const handleInputDragLeave = useCallback((e: React.DragEvent<HTMLTextAreaElement>) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
      setIsInputDragOver(false)
    }
  }, [])

  const handleInputDrop = useCallback((e: React.DragEvent<HTMLTextAreaElement>) => {
    const entryRaw = e.dataTransfer.getData(SFTP_ENTRY_MIME_TYPE)
    const pathRaw = e.dataTransfer.getData(SFTP_PATH_MIME_TYPE)
    const fallbackPath = e.dataTransfer.getData('text/plain')
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
      } catch {
      }
    }

    if (droppedPath) {
      appendPathToInput(droppedPath, false)
    }
  }, [appendPathToInput])

  const handleRegenerateResponse = useCallback(async () => {
    if (!activeSessionId || isLoading || !!pendingToolCalls) return
    if (latestAssistantMessageSourceIndex === null) return

    clearSessionStopped(activeSessionId)
    removeLatestAssistantMessage(activeSessionId)
    setGenerating(activeSessionId, true)

    try {
      const model = config?.aiModels.find(m => m.id === selectedModelId)
      const channelId = model?.channelId || ''

      await aiService.regenerateResponse(
        activeSessionId,
        selectedModelId,
        channelId,
        mode,
        boundSshSessionId
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
      } catch {
      }
    }
  }, [activeSessionId, isLoading, pendingToolCalls, latestAssistantMessageSourceIndex, clearSessionStopped, removeLatestAssistantMessage, setGenerating, config, selectedModelId, mode, boundSshSessionId, currentServerId, currentTabId, loadSessions, showAiError, selectSession])

  const handleSendMessage = useCallback(async () => {
    if (!inputValue.trim() || isLoading || !!pendingToolCalls) return;

    let sessionId = activeSessionId;

    if (!sessionId) {
      if (!currentServerId) return;
      try {
        sessionId = await createSession(currentServerId, selectedModelId, currentTabId);
      } catch (err) {
        showAiError(err);
        return;
      }
    }

    if (!sessionId) return;

    // Clear the stopped flag when sending a new message
    clearSessionStopped(sessionId);

    const content = inputValue;
    setInputValue('');
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
      textareaRef.current.focus();
    }

    const optimisticUserMessage: ChatMessage = {
      role: 'user',
      content,
      created_at: new Date().toISOString()
    }

    addOptimisticMessage({
      sessionId,
      message: optimisticUserMessage
    })

    addMessage(sessionId, optimisticUserMessage);
    setGenerating(sessionId, true);

    try {
      const model = config?.aiModels.find(m => m.id === selectedModelId);
      const channelId = model?.channelId || '';
      
      await aiService.sendMessage(
        sessionId, 
        content, 
        selectedModelId, 
        channelId,
        mode,
        boundSshSessionId
      );
      
      if (currentServerId) {
        await loadSessions(currentServerId);
      }
    } catch (err) {
      setGenerating(sessionId, false);
      showAiError(err);
    }
  }, [inputValue, activeSessionId, currentServerId, selectedModelId, createSession, addMessage, setGenerating, config, mode, currentTabId, boundSshSessionId, isLoading, pendingToolCalls, clearSessionStopped, loadSessions, showAiError]);

  const handleConfirmTools = useCallback(async () => {
    if (!activeSessionId || !pendingToolCalls) return;

    setGenerating(activeSessionId, true);
    clearSessionStopped(activeSessionId); // Clear stopped flag when tools are confirmed
    const callsToExecute = pendingToolCalls.map(c => c.id);
    storeSetPendingToolCalls(activeSessionId, null); // Hide confirmation

    try {
      const model = config?.aiModels.find(m => m.id === selectedModelId);
      const channelId = model?.channelId || '';

      await aiService.executeAgentTools(
        activeSessionId,
        selectedModelId,
        channelId,
        mode,
        boundSshSessionId,
        callsToExecute
      );
    } catch (err) {
      setGenerating(activeSessionId, false);
      showAiError(err);
    }
  }, [activeSessionId, pendingToolCalls, config, selectedModelId, mode, boundSshSessionId, setGenerating, storeSetPendingToolCalls, clearSessionStopped, showAiError]);

  const handleCancelTools = useCallback(() => {
    if (activeSessionId) {
      storeSetPendingToolCalls(activeSessionId, null);
      setGenerating(activeSessionId, false);
      markSessionStopped(activeSessionId); // Mark as stopped when tools are cancelled
    }
    // Optionally insert a "Cancelled" system message
  }, [activeSessionId, storeSetPendingToolCalls, setGenerating, markSessionStopped]);

  const handleStopGeneration = useCallback(async () => {
    // 1. Clear frontend pending tools and mark session as stopped
    if (activeSessionId && pendingToolCalls) {
      storeSetPendingToolCalls(activeSessionId, null);
    }

    // 2. Cancel backend processing if active
    if (activeSessionId && isLoading) {
      try {
        await aiService.cancelMessage(activeSessionId);
      } catch (err) {
        // Failed to cancel message
      }
    }

    // 3. Ensure loading is turned off and mark as stopped
    if (activeSessionId) {
      setGenerating(activeSessionId, false);
      markSessionStopped(activeSessionId);

      const currentSession = sessions.find(s => s.id === activeSessionId);
      if (currentSession && currentSession.title === 'New Chat') {
        try {
          const model = config?.aiModels.find(m => m.id === selectedModelId);
          const channelId = model?.channelId || '';

          await aiService.generateTitle(activeSessionId, selectedModelId, channelId);

          if (currentServerId) {
            await loadSessions(currentServerId);
          }
        } catch (err) {
        }
      }
    }
  }, [activeSessionId, isLoading, pendingToolCalls, setGenerating, storeSetPendingToolCalls, markSessionStopped, sessions, selectedModelId, config, currentServerId, loadSessions]);

  const handleModeChange = async (newMode: 'ask' | 'agent') => {
    setMode(newMode);
    if (config) {
      try {
        const newConfig = {
          ...config,
          general: {
            ...config.general,
            aiMode: newMode
          }
        };
        await saveConfig(newConfig);
      } catch (err) {
        // Failed to save AI mode
      }
    }
  };

  const handleModelChange = async (newModelId: string) => {
    setSelectedModelId(newModelId);
    if (config) {
      try {
        const newConfig = {
          ...config,
          general: {
            ...config.general,
            aiModelId: newModelId
          }
        };
        await saveConfig(newConfig);
      } catch (err) {
        // Failed to save AI model
      }
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      if (!isLoading && !pendingToolCalls) {
        handleSendMessage();
      }
    }
  };

  // Close on click outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        isOpen && 
        !isLocked && 
        sidebarRef.current && 
        !sidebarRef.current.contains(event.target as Node)
      ) {
        onClose();
      }
    };

    if (isOpen && !isLocked) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [isOpen, isLocked, onClose]);

  const sortedSessions = useMemo(() => {
    return [...sessions].sort((a, b) => 
      new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime()
    );
  }, [sessions]);

  return (
    <div
      ref={sidebarRef}
      className={`absolute top-0 bottom-0 overflow-hidden bg-[var(--bg-secondary)] border-l flex flex-col transition-all duration-200 shadow-[-2px_0_8px_rgba(0,0,0,0.2)] !right-0 !left-auto ${isOpen ? 'opacity-100 visible border-l-[var(--glass-border)]' : 'opacity-0 invisible border-transparent'} ${isResizing ? 'transition-none' : ''} ${isLocked ? '!relative shadow-none !right-auto !top-auto !bottom-auto h-full' : ''}`}
      style={{ width: isOpen ? `${width}px` : '0px', zIndex }}
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
             className={`bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] disabled:opacity-50 disabled:cursor-not-allowed disabled:text-[var(--text-muted)] ${showHistory ? 'text-[var(--accent-primary)]' : ''}`}
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
             className={`bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] disabled:opacity-50 disabled:cursor-not-allowed disabled:text-[var(--text-muted)] ${isLocked ? 'text-[var(--accent-primary)]' : ''}`}
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
                 {sortedSessions.map(session => (

                 <button
                   type="button"
                   key={session.id}
                   className={`flex items-center gap-3 px-3 py-2.5 rounded-lg bg-[var(--bg-elevated)] border border-transparent cursor-pointer transition-all duration-200 w-full text-left relative overflow-hidden hover:bg-[var(--bg-tertiary)] hover:border-[var(--glass-border)] ${activeSessionId === session.id ? 'bg-[var(--bg-tertiary)] border-[var(--accent-primary)] shadow-[0_2px_8px_rgba(0,0,0,0.1)]' : ''}`}
                   onClick={() => {
                     void selectSession(session.id, currentServerId, currentTabId);
                       setShowHistory(false);
                    }}
                  >
                    <div className="flex items-center justify-center w-8 h-8 rounded-md bg-white/5 text-[var(--text-muted)] flex-shrink-0 transition-all duration-200">
                      <MessageSquare size={16} />
                    </div>
                     <div className="flex-1 min-w-0 flex flex-col gap-0.5">
                       <div className="text-[13px] font-medium text-[var(--text-primary)] whitespace-nowrap overflow-hidden text-ellipsis" title={session.title || t.ai.newChat}>
                        <EmojiText text={session.title || t.ai.newChat} />
                       </div>
                       <div className="text-[11px] text-[var(--text-muted)] flex items-center gap-1">
                       <Clock size={10} />
                       {new Date(session.createdAt).toLocaleString(undefined, {
                         month: 'short',
                         day: 'numeric',
                         hour: '2-digit',
                         minute: '2-digit'
                       })}
                     </div>
                   </div>
                    <button
                      type="button"
                      className="ai-history-delete"
                      onClick={(e) => {
                        e.stopPropagation();
                        setSessionToDelete(session.id);
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
             className={`flex-1 overflow-y-auto px-4 py-4 flex flex-col gap-4 scroll-smooth ${isLoading ? '!scroll-auto' : ''}`}
             ref={messagesContainerRef}
             onScroll={handleScroll}
           >
             {!activeSessionId && (
               <div className="flex-1 flex flex-col items-center justify-center text-[var(--text-muted)] text-center p-8 gap-4 min-h-[200px]">
                <Bot size={48} className="opacity-20 mb-4" />
                <p>{currentServerId ? t.ai.typeMessage : t.ai.selectServer}</p>
              </div>
            )}
            
            {shouldUseVirtualizedMessages ? (
              <div
                className="relative w-full"
                style={{ height: `${virtualizedTotalHeight}px` }}
              >
                {virtualMessageItems.map(virtualItem => {
                  const message = renderableMessages[virtualItem.index]
                  if (!message) {
                    return null
                  }

                  const isLastMessage = virtualItem.index === renderableMessages.length - 1

                  return (
                    <div
                      key={`${activeSessionId}-${message.sourceIndex}-${message.msg.created_at || 'no-date'}`}
                      data-index={virtualItem.index}
                      ref={messageVirtualizer.measureElement}
                      className="absolute left-0 top-0 w-full"
                      style={{
                        transform: `translateY(${virtualItem.start}px)`,
                        paddingBottom: `${VIRTUAL_MESSAGE_GAP_PX}px`
                      }}
                    >
                      <MessageBubble
                        msg={message.msg}
                        t={t}
                        isPending={message.isPending}
                        isLast={isLastMessage}
                        isStreaming={isLoading && isLastMessage}
                        modelName={message.modelName}
                        canRegenerate={canRegenerateLatestAssistant && message.sourceIndex === latestAssistantMessageSourceIndex}
                        onRegenerate={handleRegenerateResponse}
                      />
                    </div>
                  )
                })}
              </div>
            ) : (
              renderableMessages.map((message, idx) => (
                <MessageBubble
                  key={`${activeSessionId}-${message.sourceIndex}-${message.msg.created_at || 'no-date'}`}
                  msg={message.msg}
                  t={t}
                  isPending={message.isPending}
                  isLast={idx === renderableMessages.length - 1}
                  isStreaming={isLoading && idx === renderableMessages.length - 1}
                  modelName={message.modelName}
                  canRegenerate={canRegenerateLatestAssistant && message.sourceIndex === latestAssistantMessageSourceIndex}
                  onRegenerate={handleRegenerateResponse}
                />
              ))
            )}
            
            {pendingToolCalls && (
              <div className="ai-message assistant">
                <div className="ai-message-content p-0 overflow-hidden">
                  <ToolConfirmation 
                    toolCalls={pendingToolCalls} 
                    onConfirm={handleConfirmTools} 
                    onCancel={handleCancelTools} 
                  />
                </div>
              </div>
            )}

             {isLoading && !pendingToolCalls && (
               <div className="flex flex-col gap-1 max-w-full items-start">
                 <div className="relative max-w-[90%] flex flex-col items-start">
                   <div className="p-2 px-3 rounded-lg text-[13px] leading-[1.5] w-full break-words overflow-x-auto bg-[var(--bg-tertiary)] text-[var(--text-primary)] rounded-tl-sm">
                     <div className="flex items-center gap-1 px-2 py-1 h-5">
                       <div className="w-1.5 h-1.5 bg-[var(--text-muted)] rounded-full animate-typing-bounce" style={{animationDelay: '-0.32s'}}></div>
                       <div className="w-1.5 h-1.5 bg-[var(--text-muted)] rounded-full animate-typing-bounce" style={{animationDelay: '-0.16s'}}></div>
                       <div className="w-1.5 h-1.5 bg-[var(--text-muted)] rounded-full animate-typing-bounce"></div>
                     </div>
                   </div>
                 </div>
               </div>
             )}
           </div>

           <div className="p-3 border-t border-[var(--glass-border)] bg-[var(--bg-secondary)] flex flex-col gap-2">
             <div className="flex gap-2">
               <div className="relative flex items-center flex-0-auto min-w-[100px] peer">
                 <Sliders size={14} className="absolute left-2.5 text-[var(--text-muted)] pointer-events-none z-1 transition-colors duration-200 peer-hover:text-[var(--accent-primary)] focus-within:text-[var(--accent-primary)]" />
                <CustomSelect
                  value={mode}
                  onChange={(val) => handleModeChange(val as 'ask' | 'agent')}
                  disabled={isLoading || !!pendingToolCalls}
                  placement="top"
                  triggerClassName="pl-8"
                  options={[
                    { value: 'ask', label: 'Ask' },
                    { value: 'agent', label: 'Agent' }
                  ]}
                />
               </div>
               <div className="relative flex items-center flex-1 min-w-0 peer">
                 <Sparkles size={14} className="absolute left-2.5 text-[var(--text-muted)] pointer-events-none z-1 transition-colors duration-200 peer-hover:text-[var(--accent-primary)] focus-within:text-[var(--accent-primary)]" />
                <CustomSelect
                  value={selectedModelId}
                  onChange={(val) => handleModelChange(val)}
                  disabled={isLoading || !!pendingToolCalls}
                  placement="top"
                  triggerClassName="pl-8"
                  options={(config?.aiModels || [])
                    .filter(model => {
                      const channel = config?.aiChannels?.find(c => c.id === model.channelId);
                      return model.enabled && channel?.isActive;
                    })
                    .map(model => {
                      const channel = config?.aiChannels?.find(c => c.id === model.channelId);
                      const label = channel ? `${channel.name} - ${model.name}` : model.name;
                      return { value: model.id, label };
                    })}
                />
              </div>
             </div>
             <div className="flex gap-2 items-end bg-[var(--bg-elevated)] border border-[var(--border-color)] rounded-[var(--radius-sm)] p-2 transition-colors duration-200 focus-within:border-[var(--accent-primary)]">
              <textarea
                ref={textareaRef}
                className={`flex-1 bg-transparent border-0 text-[var(--text-primary)] font-inherit text-[13px] leading-7 resize-none outline-none max-h-[150px] min-h-7 p-0 block ${isInputDragOver ? 'opacity-80' : ''}`}
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
               {(isLoading || !!pendingToolCalls) ? (
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
                  disabled={!inputValue.trim() || !selectedModelId}
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
        onConfirm={() => sessionToDelete && handleDeleteSession(sessionToDelete)}
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
  );
};
