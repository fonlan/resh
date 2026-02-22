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

export interface SftpCustomCommand {
  id: string;
  name: string;
  pattern: string;
  command: string;
  synced: boolean;
  updatedAt: string;
}

export interface SftpSettings {
  defaultDownloadPath: string;
  editors: EditorRule[];
  maxConcurrentTransfers: number;
}

export type Theme = 'light' | 'dark' | 'orange' | 'green' | 'system';
export type Language = 'en' | 'zh-CN';
export type AIMode = 'ask' | 'agent';
export type RecordingMode = 'raw' | 'text';
export type TabWidthMode = 'adaptive' | 'fixed';

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
  tabWidthMode: TabWidthMode;
  tabFixedWidth: number;
  aiModelId?: string;
}
