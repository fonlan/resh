import { invoke } from '@tauri-apps/api/core';
import { ChatMessage, AISession } from '../types/ai';

export const aiService = {
  createSession: (serverId: string, modelId?: string) => 
    invoke<string>('create_ai_session', { serverId, modelId }),
    
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

  executeAgentTools: (
    sessionId: string,
    modelId: string,
    channelId: string,
    sshSessionId: string | undefined,
    toolCallIds: string[]
  ) =>
    invoke('execute_agent_tools', {
      sessionId,
      modelId,
      channelId,
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
};
