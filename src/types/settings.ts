export interface TerminalSettings {
  fontFamily: string;
  fontSize: number;
  cursorStyle: 'block' | 'underline' | 'bar';
  scrollback: number;
}

export interface WebDAVSettings {
  url: string;
  username: string;
  password: string;
  enabled: boolean;
  proxyId?: string | null;
}

export interface EditorRule {
  id: string;
  pattern: string;
  editor: string;
}

export interface SftpSettings {
  defaultDownloadPath: string;
  editors: EditorRule[];
}

export type Theme = 'light' | 'dark' | 'orange' | 'green' | 'system';
export type Language = 'en' | 'zh-CN';
export type AIMode = 'ask' | 'agent';
export type RecordingMode = 'raw' | 'text';

export interface GeneralSettings {
  theme: Theme;
  language: Language;
  terminal: TerminalSettings;
  webdav: WebDAVSettings;
  confirmCloseTab: boolean;
  confirmExitApp: boolean;
  debugEnabled: boolean;
  snippetsSidebarLocked: boolean;
  aiSidebarLocked: boolean;
  sftpSidebarLocked: boolean;
  sftp: SftpSettings;
  aiMode: AIMode;
  aiMaxHistory: number;
  aiTimeout: number;
  maxRecentServers: number;
  recentServerIds: string[];
  recordingMode: RecordingMode;
  aiModelId?: string;
}
