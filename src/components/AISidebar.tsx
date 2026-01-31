import React, { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { useAIStore } from '../stores/useAIStore';
import { useConfig } from '../hooks/useConfig';
import { aiService } from '../services/aiService';
import { useTranslation } from '../i18n';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { X, Send, Lock, LockOpen, Plus, History, Bot, Copy, Terminal, Check, AlertTriangle, Clock, Sliders, Sparkles, MessageSquare, Trash2, ChevronDown, ChevronRight, BrainCircuit } from 'lucide-react';
import { listen } from '@tauri-apps/api/event';
import { ToolCall, ChatMessage } from '../types/ai';
import { ConfirmationModal } from './ConfirmationModal';
import { CustomSelect } from './CustomSelect';
import './AISidebar.css';

interface AISidebarProps {
  isOpen: boolean;
  onClose: () => void;
  isLocked: boolean;
  onToggleLock: () => void;
  currentServerId?: string;
  currentTabId?: string;
}

const CodeBlock = ({ children, className }: { children: React.ReactNode, className?: string }) => {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  
  const codeContent = useMemo(() => {
    if (typeof children === 'string') return children;
    if (Array.isArray(children)) return children.join('');
    return String(children);
  }, [children]).replace(/\n$/, '');

  const language = className ? className.replace('language-', '') : 'text';

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(codeContent);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  };

  const handleInsert = () => {
    window.dispatchEvent(new CustomEvent('paste-snippet', { detail: codeContent }));
  };

  return (
    <div className="ai-code-block">
      <div className="ai-code-header">
        <span className="ai-code-lang">{language}</span>
        <div className="ai-code-actions">
          <button 
            type="button"
            className="ai-code-btn" 
            onClick={handleCopy} 
            title={t.ai.tool.copyCode}
          >
            {copied ? <Check size={14} className="text-green-500" /> : <Copy size={14} />}
          </button>
          <button 
            type="button"
            className="ai-code-btn" 
            onClick={handleInsert} 
            title={t.ai.tool.insertToTerminal}
          >
            <Terminal size={14} />
          </button>
        </div>
      </div>
      <pre className="ai-code-content">
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
            
            console.log('[AI ToolConfirm] Original command:', originalCommand);
            console.log('[AI ToolConfirm] Cleaned command:', cleanCommand);
            
            // Check for always dangerous commands
            if (alwaysDangerous.test(cleanCommand)) {
              console.log('[AI ToolConfirm] Detected always-dangerous command');
              sensitive = true;
            } 
            // Check for potentially dangerous commands
            else if (potentiallyDangerous.test(cleanCommand)) {
              console.log('[AI ToolConfirm] Detected potentially-dangerous command');
              sensitive = true;
            }
            // Check for curl/wget piped to shell
            else if (dangerousWhenPiped.test(cleanCommand)) {
              console.log('[AI ToolConfirm] Detected curl/wget piped to shell');
              sensitive = true;
            }
            else {
              console.log('[AI ToolConfirm] Command is safe');
            }
          }
        } catch (e) {
          console.error('[AI ToolConfirm] Failed to parse command arguments:', e);
          sensitive = true;
        }
      }
    });

    setIsSensitive(sensitive);
    
    // Auto-execute if NOT sensitive
    if (!sensitive) {
      console.log('[AI ToolConfirm] Starting 5s countdown for auto-execution');
      setCountdown(5);
    } else {
      console.log('[AI ToolConfirm] Sensitive command detected, requiring manual confirmation');
    }
  }, [toolCalls]);

  useEffect(() => {
    if (countdown === null || confirmedRef.current) return;
    
    if (countdown <= 0) {
      console.log('[AI] Auto-executing tool calls...');
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
    <div className={`ai-tool-confirm ${isSensitive ? 'sensitive' : ''}`}>
      <div className="ai-tool-header">
        {isSensitive ? (
          <><AlertTriangle size={16} className="text-red-500" /> {t.ai.tool.confirmExecution}</>
        ) : (
          <><Clock size={16} /> {t.ai.tool.autoExecute.replace('{seconds}', String(countdown))}</>
        )}
      </div>
      <div className="ai-tool-list">
        {toolCalls.map(call => {
          let displayArgs = call.function.arguments;
          if (call.function.name === 'run_in_terminal') {
             try {
               const args = JSON.parse(call.function.arguments);
               displayArgs = args.command || displayArgs;
             } catch {}
          }
          return (
            <div key={call.id} className="ai-tool-item">
              <span className="font-mono text-xs opacity-70">
                {call.function.name === 'run_in_terminal' ? t.ai.tool.executeCommand : call.function.name}
              </span>
              <code className="block mt-1 text-sm bg-black/20 p-1 rounded">{displayArgs}</code>
            </div>
          );
        })}
      </div>
      <div className="ai-tool-actions">
        <button type="button" className="ai-btn-secondary" onClick={onCancel}>{t.ai.tool.cancel}</button>
        <button 
          type="button"
          className={`ai-btn-primary ${isSensitive ? 'bg-red-600 hover:bg-red-700' : ''}`} 
          onClick={onConfirm}
        >
          {isSensitive ? t.ai.tool.confirmRun : t.ai.tool.runNow.replace('{seconds}', String(countdown))}
        </button>
      </div>
    </div>
  );
};

const MessageBubble = ({ msg, t, isPending, isLast, isLoading }: { msg: ChatMessage, t: any, isPending?: boolean, isLast?: boolean, isLoading?: boolean }) => {
  const [copied, setCopied] = useState(false);
  const [showReasoning, setShowReasoning] = useState(true);

  const handleCopy = async () => {
    if (!msg.content) return;
    try {
      await navigator.clipboard.writeText(msg.content);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy message:', err);
    }
  };

  const visibleToolCalls = useMemo(() => {
    if (!msg.tool_calls || isPending) return [];
    return msg.tool_calls.filter(call => call.function.name !== 'get_terminal_output');
  }, [msg.tool_calls, isPending]);

  return (
    <div className={`ai-message ${msg.role}`}>
      <div className="ai-message-wrapper">
        <div className="ai-message-content">
          {msg.reasoning_content && (
            <div className="ai-reasoning-section mb-2">
              <button 
                type="button" 
                className="ai-reasoning-toggle"
                onClick={() => setShowReasoning(!showReasoning)}
              >
                {showReasoning ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                <BrainCircuit size={14} className="ml-1 mr-1 text-blue-400" />
                <span>{t.ai.thinkingProcess}</span>
              </button>
              {showReasoning && (
                <div className="ai-reasoning-content">
                  {msg.reasoning_content}
                  {isLast && isLoading && !msg.content && <span className="ai-streaming-cursor">|</span>}
                </div>
              )}
            </div>
          )}
          {visibleToolCalls.length > 0 ? (
            <div className="ai-tool-confirm mb-2">
              <div className="ai-tool-header">
                <Check size={16} className="text-green-500" />
                <span>{t.ai.tool.executeCommand}</span>
              </div>
              <div className="ai-tool-list">
                {visibleToolCalls.map((call: ToolCall) => {
                  let displayArgs = call.function.arguments;
                  if (call.function.name === 'run_in_terminal') {
                     try {
                       const args = JSON.parse(call.function.arguments);
                       displayArgs = args.command || displayArgs;
                     } catch {}
                  }
                  return (
                    <div key={call.id} className="ai-tool-item">
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
            <ReactMarkdown 
              remarkPlugins={[remarkGfm]}
              components={{
                pre({children}) {
                  return <>{children}</>;
                },
                code({className, children}: any) {
                  if (className && className.startsWith('language-')) {
                    return (
                      <CodeBlock className={className}>
                        {children}
                      </CodeBlock>
                    );
                  }
                  return (
                    <code className="ai-inline-reference">
                      {children}
                    </code>
                  );
                }
              }}
            >
              {msg.content}
            </ReactMarkdown>
          )}
        </div>
        <button 
          type="button"
          className={`ai-message-copy-btn ${copied ? 'copied' : ''}`}
          onClick={handleCopy}
          title={t.ai.copyMessage}
        >
          {copied ? <Check size={14} className="text-green-500" /> : <Copy size={14} />}
        </button>
      </div>
    </div>
  );
};

export const AISidebar: React.FC<AISidebarProps> = ({
  isOpen,
  onClose,
  isLocked,
  onToggleLock,
  currentServerId,
  currentTabId
}) => {
  const { t } = useTranslation();
  const { config, saveConfig } = useConfig();
  const { 
    sessions, 
    activeSessionId, 
    messages, 
    isLoading,
    loadSessions,
    createSession,
    selectSession,
    addMessage,
    newAssistantMessage,
    appendResponse,
    appendReasoning,
    appendToolCalls,
    setLoading,
    deleteSession,
    clearSessions
  } = useAIStore();

  const [width, setWidth] = useState(350);
  const [isResizing, setIsResizing] = useState(false);
  const [inputValue, setInputValue] = useState('');
  const [showHistory, setShowHistory] = useState(false);
  const [mode, setMode] = useState<'ask' | 'agent'>(config?.general.aiMode as 'ask' | 'agent' || 'ask');
  const [selectedModelId, setSelectedModelId] = useState<string>('');
  const [pendingToolCalls, setPendingToolCalls] = useState<ToolCall[] | null>(null);
  const [sessionToDelete, setSessionToDelete] = useState<string | null>(null);
  const [isClearingHistory, setIsClearingHistory] = useState(false);

  const sidebarRef = useRef<HTMLDivElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Load mode & model from config
  useEffect(() => {
    if (config?.general.aiMode) {
      setMode(config.general.aiMode as 'ask' | 'agent');
    }
    if (config?.general.aiModelId) {
      setSelectedModelId(config.general.aiModelId);
    }
  }, [config?.general.aiMode, config?.general.aiModelId]);

  // Set default model
  useEffect(() => {
    if (config?.aiModels && config.aiModels.length > 0 && !selectedModelId) {
      const firstEnabledModel = config.aiModels.find(model => {
        const channel = config.aiChannels?.find(c => c.id === model.channelId);
        return model.enabled && channel?.isActive;
      });
      if (firstEnabledModel) {
        setSelectedModelId(firstEnabledModel.id);
      }
    }
  }, [config?.aiModels, config?.aiChannels, selectedModelId]);

  // Load sessions when server changes
  useEffect(() => {
    if (currentServerId && isOpen) {
      loadSessions(currentServerId);
    }
  }, [currentServerId, isOpen, loadSessions]);

  // Clear active session when switching servers
  useEffect(() => {
    // console.log('[AISidebar] Server context changed to:', currentServerId);
    if (currentServerId || currentServerId === undefined) {
      selectSession(null);
    }
  }, [currentServerId, selectSession]);

  // Scroll to bottom on new messages
  const currentMessages = useMemo(() => 
    activeSessionId ? messages[activeSessionId] || [] : [], 
    [activeSessionId, messages]
  );

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [currentMessages.length, isLoading, pendingToolCalls]);

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
        console.error('Failed to delete session:', err);
      }
    }
  };

  const handleClearHistory = async () => {
    if (currentServerId) {
      try {
        await clearSessions(currentServerId);
        setIsClearingHistory(false);
      } catch (err) {
        console.error('Failed to clear history:', err);
      }
    }
  };

  // Listen for streaming responses & tool calls
  useEffect(() => {
    if (!activeSessionId) return;

    const startedListener = listen<string>(`ai-started-${activeSessionId}`, () => {
      console.log('[AI] Request started');
      setPendingToolCalls(null);
      newAssistantMessage(activeSessionId);
    });

    const responseListener = listen<string>(`ai-response-${activeSessionId}`, (event) => {
      console.log('[AI] Response chunk received:', event.payload.length, 'chars');
      setLoading(false);
      appendResponse(activeSessionId, event.payload);
    });

    const reasoningListener = listen<string>(`ai-reasoning-${activeSessionId}`, (event) => {
      console.log('[AI] Reasoning chunk received:', event.payload.length, 'chars');
      setLoading(false);
      appendReasoning(activeSessionId, event.payload);
    });

    const toolCallListener = listen<ToolCall[]>(`ai-tool-call-${activeSessionId}`, (event) => {
      console.log('[AI] Tool calls received:', event.payload);
      const calls = event.payload;

      // Update store with tool calls so they appear in the message bubble
      appendToolCalls(activeSessionId, calls);

      // Filter: if ALL calls are get_terminal_output, auto-execute immediately WITHOUT UI
      const isAllSafe = calls.every(c => c.function.name === 'get_terminal_output');

      if (isAllSafe) {
        console.log('[AI] All tool calls are safe (terminal read). Auto-executing immediately.');
        
        // Ensure we are using valid IDs
        const model = config?.aiModels.find(m => m.id === selectedModelId);
        if (!model) {
             console.error('Model not found for auto-execution');
             setLoading(false);
             return;
        }
        const channelId = model.channelId || '';

        aiService.executeAgentTools(
          activeSessionId,
          selectedModelId,
          channelId,
          mode,
          currentTabId,
          calls.map(c => c.id)
        ).catch(err => console.error('Failed to auto-execute safe tools:', err));
        
        // Do NOT set pendingToolCalls
        setLoading(true); 
      } else {
        // Here we have run_in_terminal calls. 
        // We set pendingToolCalls to show the UI.
        // The UI (ToolConfirmation) handles the countdown for non-sensitive commands.
        setLoading(false);
        setPendingToolCalls(calls);
      }
    });

    const errorListener = listen<string>(`ai-error-${activeSessionId}`, (event) => {
      console.error('[AI] Error received:', event.payload);
      setLoading(false);
      setPendingToolCalls(null);
      // TODO: Show error in UI
    });

    const doneListener = listen<string>(`ai-done-${activeSessionId}`, async () => {
      console.log('[AI] Response complete');
      setLoading(false);

      // Auto-generate title for new sessions after first response
      const currentSession = sessions.find(s => s.id === activeSessionId);
      if (currentSession && currentSession.title === 'New Chat') {
        console.log('[AI] Generating title for new session...');
        try {
          const model = config?.aiModels.find(m => m.id === selectedModelId);
          const channelId = model?.channelId || '';
          
          await aiService.generateTitle(activeSessionId, selectedModelId, channelId);
          console.log('[AI] Title generated successfully');
          
          // Reload sessions to get the updated title
          if (currentServerId) {
            await loadSessions(currentServerId);
          }
        } catch (err) {
          console.error('[AI] Failed to generate title:', err);
          // Don't show error to user, it's not critical
        }
      }
    });

    return () => {
      startedListener.then(unlisten => unlisten());
      responseListener.then(unlisten => unlisten());
      reasoningListener.then(unlisten => unlisten());
      toolCallListener.then(unlisten => unlisten());
      errorListener.then(unlisten => unlisten());
      doneListener.then(unlisten => unlisten());
    };
  }, [activeSessionId, appendResponse, appendReasoning, appendToolCalls, newAssistantMessage, setLoading, config, selectedModelId, mode, currentTabId, sessions, currentServerId, loadSessions]);

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
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
    }

    return () => {
      window.removeEventListener('mousemove', resize);
      window.removeEventListener('mouseup', stopResizing);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
  }, [isResizing]);

  const handleCreateSession = useCallback(async () => {
    if (currentServerId) {
      try {
        await createSession(currentServerId, selectedModelId);
        setShowHistory(false);
      } catch (err) {
        console.error('Failed to create session:', err);
      }
    }
  }, [currentServerId, selectedModelId, createSession]);

  const handleSendMessage = useCallback(async () => {
    if (!inputValue.trim() || isLoading || !!pendingToolCalls) return;

    let sessionId = activeSessionId;

    if (!sessionId) {
      if (!currentServerId) return;
      try {
        sessionId = await createSession(currentServerId, selectedModelId);
      } catch (err) {
        console.error('Failed to create session:', err);
        return;
      }
    }

    if (!sessionId) return;

    const content = inputValue;
    setInputValue('');
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
      textareaRef.current.focus();
    }

    // Optimistic update
    addMessage(sessionId, { role: 'user', content });
    setLoading(true);

    try {
      const model = config?.aiModels.find(m => m.id === selectedModelId);
      const channelId = model?.channelId || '';
      
      console.log('[AI] Sending message:', {
        sessionId,
        modelId: selectedModelId,
        channelId,
        mode,
        currentTabId,
        content: content.substring(0, 50)
      });
      
      await aiService.sendMessage(
        sessionId, 
        content, 
        selectedModelId, 
        channelId,
        mode,
        currentTabId
      );
      
      console.log('[AI] Message sent successfully');
    } catch (err) {
      console.error('[AI] Failed to send message:', err);
      setLoading(false);
    }
  }, [inputValue, activeSessionId, currentServerId, selectedModelId, createSession, addMessage, setLoading, config, mode, currentTabId, isLoading, pendingToolCalls]);

  const handleConfirmTools = useCallback(async () => {
    if (!activeSessionId || !pendingToolCalls) return;

    console.log('[AI] Confirming tool execution for session:', activeSessionId);
    setLoading(true);
    const callsToExecute = pendingToolCalls.map(c => c.id);
    setPendingToolCalls(null); // Hide confirmation

    try {
      const model = config?.aiModels.find(m => m.id === selectedModelId);
      const channelId = model?.channelId || '';

      console.log('[AI] Invoking execute_agent_tools...');
      await aiService.executeAgentTools(
        activeSessionId,
        selectedModelId,
        channelId,
        mode,
        currentTabId,
        callsToExecute
      );
      console.log('[AI] Tool execution command sent successfully');
    } catch (err) {
      console.error('Failed to execute tools:', err);
      setLoading(false);
    }
  }, [activeSessionId, pendingToolCalls, config, selectedModelId, mode, currentTabId, setLoading]);

  const handleCancelTools = useCallback(() => {
    setPendingToolCalls(null);
    setLoading(false);
    // Optionally insert a "Cancelled" system message
  }, [setLoading]);

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
        console.error('Failed to save AI mode:', err);
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
        console.error('Failed to save AI model:', err);
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
      className={`ai-sidebar-panel ${isOpen ? 'open' : ''} ${isResizing ? 'resizing' : ''} ${isLocked ? 'locked' : ''}`}
      style={{ width: isOpen ? `${width}px` : '0px' }}
      aria-hidden={!isOpen}
    >
      <div 
        className="ai-resizer" 
        onMouseDown={startResizing}
        role="separator"
        aria-orientation="vertical"
        aria-valuenow={width}
        aria-valuemin={250}
        aria-valuemax={800}
        aria-label="Resize Sidebar"
        tabIndex={0}
      />
      
      <div className="ai-header">
        <h3 className="ai-title">
          <Bot size={16} /> {t.ai.sidebarTitle}
        </h3>
        <div className="ai-actions">
           <button 
            type="button"
            className={`ai-action-btn ${showHistory ? 'active' : ''}`}
            onClick={() => setShowHistory(!showHistory)}
            title={t.ai.history}
          >
            <History size={16} />
          </button>
          <button 
            type="button"
            className="ai-action-btn"
            onClick={handleCreateSession}
            title={t.ai.newChat}
            disabled={!currentServerId}
          >
            <Plus size={16} />
          </button>
          <button
            type="button"
            className={`ai-action-btn ${isLocked ? 'active' : ''}`}
            onClick={onToggleLock}
            title={isLocked ? "Unlock Sidebar" : "Lock Sidebar"}
          >
            {isLocked ? <Lock size={16} /> : <LockOpen size={16} />}
          </button>
          <button 
            type="button"
            className="ai-action-btn"
            onClick={onClose}
            title="Close"
          >
            <X size={16} />
          </button>
        </div>
      </div>

      {showHistory ? (
        <div className="ai-messages">
          {sortedSessions.length === 0 ? (
            <div className="ai-empty-history">
              <History size={48} className="ai-empty-history-icon" />
              <p>{t.ai.noHistory}</p>
            </div>
          ) : (
              <div className="ai-history-list">
                <div className="ai-history-header">
                  <span>{t.ai.history}</span>
                  <button 
                    type="button" 
                    className="ai-clear-btn"
                    onClick={() => setIsClearingHistory(true)}
                  >
                    <Trash2 size={12} /> {t.ai.clearHistory}
                  </button>
                </div>
                {sortedSessions.map(session => (

                 <button
                   type="button"
                   key={session.id}
                   className={`ai-history-item ${activeSessionId === session.id ? 'active' : ''}`}
                   onClick={() => {
                     selectSession(session.id);
                     setShowHistory(false);
                   }}
                 >
                   <div className="ai-history-icon">
                     <MessageSquare size={16} />
                   </div>
                   <div className="ai-history-info">
                     <div className="ai-history-title" title={session.title || t.ai.newChat}>
                       {session.title || t.ai.newChat}
                     </div>
                     <div className="ai-history-meta">
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
          <div className="ai-messages">
            {!activeSessionId && (
              <div className="ai-empty-state">
                <Bot size={48} className="opacity-20 mb-4" />
                <p>{currentServerId ? t.ai.typeMessage : t.ai.selectServer}</p>
              </div>
            )}
            
            {currentMessages.map((msg, idx) => {
              const isPending = !!(pendingToolCalls && msg.tool_calls && pendingToolCalls.length > 0 && 
                  msg.tool_calls.some(tc => pendingToolCalls.some(ptc => ptc.id === tc.id)));
              
              if (msg.role === 'assistant') {
                const hasContent = msg.content && msg.content.trim().length > 0;
                const hasReasoning = msg.reasoning_content && msg.reasoning_content.trim().length > 0;
                const hasVisibleTools = msg.tool_calls && msg.tool_calls.some(tc => 
                  tc.function.name !== 'get_terminal_output'
                );
                
                // Keep the message if it has content, tools, or reasoning process
                if (!hasContent && !hasVisibleTools && !hasReasoning) {
                  return null;
                }
              }

              return <MessageBubble 
                key={`${activeSessionId}-${idx}`} 
                msg={msg} 
                t={t} 
                isPending={isPending} 
                isLast={idx === currentMessages.length - 1}
                isLoading={isLoading}
              />;
            })}
            
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
              <div className="ai-message assistant">
                <div className="ai-message-content">
                  <div className="ai-typing-indicator">
                    <div className="ai-typing-dot"></div>
                    <div className="ai-typing-dot"></div>
                    <div className="ai-typing-dot"></div>
                  </div>
                </div>
              </div>
            )}
            <div ref={messagesEndRef} />
          </div>

          <div className="ai-input-area">
            <div className="ai-controls">
              <div className="ai-select-wrapper">
                <Sliders size={14} className="ai-select-icon" />
                <CustomSelect
                  value={mode}
                  onChange={(val) => handleModeChange(val as 'ask' | 'agent')}
                  disabled={isLoading || !!pendingToolCalls}
                  placement="top"
                  options={[
                    { value: 'ask', label: 'Ask' },
                    { value: 'agent', label: 'Agent' }
                  ]}
                />
              </div>
              <div className="ai-select-wrapper ai-select-wrapper-model">
                <Sparkles size={14} className="ai-select-icon" />
                <CustomSelect
                  value={selectedModelId}
                  onChange={(val) => handleModelChange(val)}
                  disabled={isLoading || !!pendingToolCalls}
                  placement="top"
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
            <div className="ai-input-container">
              <textarea
                ref={textareaRef}
                className="ai-input"
                placeholder={t.ai.typeMessage}
                value={inputValue}
                onChange={(e) => setInputValue(e.target.value)}
                onKeyDown={handleKeyDown}
                disabled={!!pendingToolCalls}
                rows={1}
              />
              <button 
                type="button"
                className="ai-send-btn"
                onClick={handleSendMessage}
                disabled={!inputValue.trim() || isLoading || !selectedModelId || !!pendingToolCalls}
              >
                <Send size={14} />
              </button>
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
