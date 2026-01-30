import { create } from 'zustand';
import { ChatMessage, AISession } from '../types/ai';
import { aiService } from '../services/aiService';

interface AIState {
  sessions: AISession[];
  activeSessionId: string | null;
  messages: Record<string, ChatMessage[]>;
  isLoading: boolean;
  
  loadSessions: (serverId: string) => Promise<void>;
  createSession: (serverId: string, modelId?: string) => Promise<string>;
  selectSession: (sessionId: string | null) => Promise<void>;
  addMessage: (sessionId: string, message: ChatMessage) => void;
  appendResponse: (sessionId: string, content: string) => void;
  setLoading: (loading: boolean) => void;
  deleteSession: (serverId: string, sessionId: string) => Promise<void>;
  clearSessions: (serverId: string) => Promise<void>;
}

export const useAIStore = create<AIState>((set, get) => ({
  sessions: [],
  activeSessionId: null,
  messages: {},
  isLoading: false,

  setLoading: (loading) => set({ isLoading: loading }),

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
    await get().selectSession(id);
    return id;
  },

  selectSession: async (sessionId) => {
    set({ activeSessionId: sessionId });
    if (sessionId) {
      const msgs = await aiService.getMessages(sessionId);
      set(state => ({
        messages: { ...state.messages, [sessionId]: msgs }
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

  appendResponse: (sessionId, content) => {
    set(state => {
      const current = state.messages[sessionId] || [];
      const last = current[current.length - 1];
      
      if (last && last.role === 'assistant') {
        // Update last message
        const updated = [...current];
        updated[updated.length - 1] = { ...last, content: last.content + content };
        return { messages: { ...state.messages, [sessionId]: updated } };
      } else {
        // Create new assistant message
        return {
          messages: { ...state.messages, [sessionId]: [...current, { role: 'assistant', content }] }
        };
      }
    });
  },

  deleteSession: async (serverId, sessionId) => {
    await aiService.deleteSession(sessionId);
    if (get().activeSessionId === sessionId) {
      set({ activeSessionId: null });
    }
    await get().loadSessions(serverId);
  },

  clearSessions: async (serverId) => {
    await aiService.deleteAllSessions(serverId);
    set({ activeSessionId: null, sessions: [] });
    await get().loadSessions(serverId);
  },
}));
