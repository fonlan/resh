import { invoke } from '@tauri-apps/api/core';
import { ChatMessage, AISession } from '../types/ai';

export const aiService = {
  createSession: (serverId: string, modelId?: string, sshSessionId?: string) => 
    invoke<string>('create_ai_session', { serverId, modelId, sshSessionId }),
    
  getSessions: (serverId: string) => 
    invoke<AISession[]>('get_ai_sessions', { serverId }),
    
  getMessages: (sessionId: string) => 
    invoke<ChatMessage[]>('get_ai_messages', { sessionId }),
    
  sendMessage: (
    sessionId: string, 
    content: string, 
    modelId: string, 
    channelId: string,
    mode?: string,
    sshSessionId?: string
  ) => 
    invoke('send_chat_message', { 
      sessionId, 
      content, 
      modelId, 
      channelId,
      mode,
      sshSessionId
    }),

  cancelMessage: (sessionId: string) =>
    invoke('cancel_ai_chat', { sessionId }),

  executeAgentTools: (
    sessionId: String,
    modelId: String,
    channelId: String,
    mode: string | undefined,
    sshSessionId: string | undefined,
    toolCallIds: string[]
  ) =>
    invoke('execute_agent_tools', {
      sessionId,
      modelId,
      channelId,
      mode,
      sshSessionId,
      toolCallIds
    }),


  generateTitle: (
    sessionId: string,
    modelId: string,
    channelId: string
  ) =>
    invoke<string>('generate_session_title', {
      sessionId,
      modelId,
      channelId
    }),

  deleteSession: (sessionId: string) =>
    invoke<void>('delete_ai_session', { sessionId }),

  deleteAllSessions: (serverId: string) =>
    invoke<void>('delete_all_ai_sessions', { serverId }),
};
