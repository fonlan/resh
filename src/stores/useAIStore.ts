import { create } from 'zustand';
import { ChatMessage, AISession, ToolCall } from '../types/ai';
import { aiService } from '../services/aiService';

interface AIState {
  sessions: AISession[];
  activeSessionId: string | null;
  activeSessionIdByServer: Record<string, string | null>;
  messages: Record<string, ChatMessage[]>;
  isLoading: boolean;
  isGenerating: Record<string, boolean>;
  pendingToolCalls: Record<string, ToolCall[] | null>;
  stoppedSessions: Set<string>; // Track sessions that were manually stopped
  
  loadSessions: (serverId: string) => Promise<void>;
  createSession: (serverId: string, modelId?: string) => Promise<string>;
  selectSession: (sessionId: string | null, serverId?: string) => Promise<void>;
  addMessage: (sessionId: string, message: ChatMessage) => void;
  newAssistantMessage: (sessionId: string) => void;
  appendResponse: (sessionId: string, content: string) => void;
  appendReasoning: (sessionId: string, reasoning: string) => void;
  appendToolCalls: (sessionId: string, toolCalls: ToolCall[]) => void;
  setLoading: (loading: boolean) => void;
  setGenerating: (sessionId: string, generating: boolean) => void;
  setPendingToolCalls: (sessionId: string, toolCalls: ToolCall[] | null) => void;
  markSessionStopped: (sessionId: string) => void;
  clearSessionStopped: (sessionId: string) => void;
  deleteSession: (serverId: string, sessionId: string) => Promise<void>;
  clearSessions: (serverId: string) => Promise<void>;
}

export const useAIStore = create<AIState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  activeSessionIdByServer: {},
  messages: {},
  isLoading: false,
  isGenerating: {},
  pendingToolCalls: {},
  stoppedSessions: new Set<string>(),

  setLoading: (loading) => set({ isLoading: loading }),
  
  setGenerating: (sessionId, generating) => set(state => ({
    isGenerating: { ...state.isGenerating, [sessionId]: generating }
  })),

  setPendingToolCalls: (sessionId, toolCalls) => set(state => ({
    pendingToolCalls: { ...state.pendingToolCalls, [sessionId]: toolCalls }
  })),

  markSessionStopped: (sessionId) => set(state => {
    const newStoppedSessions = new Set(state.stoppedSessions);
    newStoppedSessions.add(sessionId);
    return { stoppedSessions: newStoppedSessions };
  }),

  clearSessionStopped: (sessionId) => set(state => {
    const newStoppedSessions = new Set(state.stoppedSessions);
    newStoppedSessions.delete(sessionId);
    return { stoppedSessions: newStoppedSessions };
  }),

  loadSessions: async (serverId) => {
    set({ isLoading: true });
    try {
      const sessions = await aiService.getSessions(serverId);
      set({ sessions });
    } finally {
      set({ isLoading: false });
    }
  },

  createSession: async (serverId, modelId) => {
    const id = await aiService.createSession(serverId, modelId);
    await get().loadSessions(serverId);
    await get().selectSession(id, serverId);
    return id;
  },

  selectSession: async (sessionId, serverId) => {
    set(state => ({ 
      activeSessionId: sessionId,
      activeSessionIdByServer: serverId ? { ...state.activeSessionIdByServer, [serverId]: sessionId } : state.activeSessionIdByServer
    }));
    if (sessionId) {
      const msgs = await aiService.getMessages(sessionId);
      
      // Check if this session was manually stopped - if so, don't restore pending tool calls
      const isStopped = get().stoppedSessions.has(sessionId);
      
      // Check for pending tool calls in history
      let pending = null;
      if (!isStopped && msgs.length > 0) {
        const lastMsg = msgs[msgs.length - 1];
        if (lastMsg.role === 'assistant' && lastMsg.tool_calls && lastMsg.tool_calls.length > 0) {
          const hasVisibleTools = lastMsg.tool_calls.some(tc => 
            tc.function.name !== 'get_terminal_output'
          );
          if (hasVisibleTools) {
            pending = lastMsg.tool_calls;
          }
        }
      }

      set(state => ({
        messages: { ...state.messages, [sessionId]: msgs },
        pendingToolCalls: { ...state.pendingToolCalls, [sessionId]: pending }
      }));
    }
  },

  addMessage: (sessionId, message) => {
    set(state => {
      const current = state.messages[sessionId] || [];
      return {
        messages: { ...state.messages, [sessionId]: [...current, message] }
      };
    });
  },

  newAssistantMessage: (sessionId) => {
    set(state => {
      const current = state.messages[sessionId] || [];
      // Don't add a new message if the last one is already an empty assistant message
      const last = current[current.length - 1];
      if (last && last.role === 'assistant' && !last.content && !last.tool_calls) {
        return state;
      }
      return {
        messages: { ...state.messages, [sessionId]: [...current, { role: 'assistant', content: '' }] }
      };
    });
  },

  appendResponse: (sessionId, content) => {
    set(state => {
      const current = state.messages[sessionId] || [];
      const last = current[current.length - 1];
      
      if (last && last.role === 'assistant') {
        // Update last message
        const updated = [...current];
        updated[updated.length - 1] = { ...last, content: (last.content || '') + content };
        return { messages: { ...state.messages, [sessionId]: updated } };
      } else {
        // Create new assistant message
        return {
          messages: { ...state.messages, [sessionId]: [...current, { role: 'assistant', content }] }
        };
      }
    });
  },

  appendReasoning: (sessionId, reasoning) => {
    set(state => {
      const current = state.messages[sessionId] || [];
      const last = current[current.length - 1];
      
      if (last && last.role === 'assistant') {
        const updated = [...current];
        updated[updated.length - 1] = { ...last, reasoning_content: (last.reasoning_content || '') + reasoning };
        return { messages: { ...state.messages, [sessionId]: updated } };
      } else {
        return {
          messages: { ...state.messages, [sessionId]: [...current, { role: 'assistant', content: '', reasoning_content: reasoning }] }
        };
      }
    });
  },

  appendToolCalls: (sessionId, toolCalls) => {
    set(state => {
      const current = state.messages[sessionId] || [];
      const last = current[current.length - 1];
      
      if (last && last.role === 'assistant') {
        const updated = [...current];
        updated[updated.length - 1] = { ...last, tool_calls: toolCalls };
        return { messages: { ...state.messages, [sessionId]: updated } };
      } else {
        return {
          messages: { ...state.messages, [sessionId]: [...current, { role: 'assistant', content: '', tool_calls: toolCalls }] }
        };
      }
    });
  },

  deleteSession: async (serverId, sessionId) => {
    await aiService.deleteSession(sessionId);
    const state = get();
    if (state.activeSessionId === sessionId) {
      set({ activeSessionId: null });
    }
    if (state.activeSessionIdByServer[serverId] === sessionId) {
      set(s => ({
        activeSessionIdByServer: { ...s.activeSessionIdByServer, [serverId]: null }
      }));
    }
    
    // Clean up session-specific state
    set(s => {
      const isGenerating = { ...s.isGenerating };
      const pendingToolCalls = { ...s.pendingToolCalls };
      const messages = { ...s.messages };
      const stoppedSessions = new Set(s.stoppedSessions);
      delete isGenerating[sessionId];
      delete pendingToolCalls[sessionId];
      delete messages[sessionId];
      stoppedSessions.delete(sessionId);
      return { isGenerating, pendingToolCalls, messages, stoppedSessions };
    });

    await get().loadSessions(serverId);
  },

  clearSessions: async (serverId) => {
    await aiService.deleteAllSessions(serverId);
    set(state => ({ 
      activeSessionId: null, 
      sessions: [],
      activeSessionIdByServer: { ...state.activeSessionIdByServer, [serverId]: null },
      isGenerating: {},
      pendingToolCalls: {},
      messages: {},
      stoppedSessions: new Set<string>()
    }));
    await get().loadSessions(serverId);
  },
}));
