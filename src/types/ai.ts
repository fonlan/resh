export interface ChatMessage {
  role: 'user' | 'assistant' | 'system';
  content: string;
}

export interface AISession {
  id: string;
  title: string;
  createdAt: string;
  modelId: string | null;
}
