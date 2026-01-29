// src/types/config.ts

export interface Server {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  authId: string | null;
  proxyId: string | null;
  jumphostId: string | null;
  portForwards: PortForward[];
  keepAlive: number;
  autoExecCommands: string[];
  envVars: Record<string, string>;
  snippets?: Snippet[];
  synced: boolean;
  updatedAt: string;
}

export interface PortForward {
  local: number;
  remote: number;
}

export interface Authentication {
  id: string;
  name: string;
  type: 'key' | 'password';
  // For SSH key
  keyContent?: string;
  passphrase?: string;
  // For password
  password?: string;
  synced: boolean;
  updatedAt: string;
}

export interface Snippet {
  id: string;
  name: string;
  content: string;
  description?: string;
  group?: string;
  synced: boolean;
  updatedAt: string;
}

export interface ProxyConfig {
  id: string;
  name: string;
  type: 'http' | 'socks5';
  host: string;
  port: number;
  username?: string;
  password?: string;
  synced: boolean;
  updatedAt: string;
}

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

export interface GeneralSettings {
  theme: 'light' | 'dark' | 'system';
  language: 'en' | 'zh-CN';
  terminal: TerminalSettings;
  webdav: WebDAVSettings;
  confirmCloseTab: boolean;
  confirmExitApp: boolean;
  debugEnabled: boolean;
  snippetsSidebarLocked: boolean;
  maxRecentServers: number;
  recentServerIds: string[];
  recordingMode: 'raw' | 'text';
}

export interface AIChannel {
  id: string;
  name: string;
  type: 'openai' | 'copilot';
  endpoint?: string;
  apiKey?: string;
  isActive: boolean;
  synced: boolean;
  updatedAt: string;
}

export interface AIModel {
  id: string;
  name: string;
  channelId: string;
  synced: boolean;
  updatedAt: string;
}

export interface Config {
  version: string;
  servers: Server[];
  authentications: Authentication[];
  proxies: ProxyConfig[];
  snippets: Snippet[];
  aiChannels: AIChannel[];
  aiModels: AIModel[];
  general: GeneralSettings;
}

export interface AppState {
  syncConfig: Config;
  localConfig: Config;
  mergedConfig: Config;
  masterPasswordSet: boolean;
}

export interface ManualAuthCredentials {
  username: string;
  password?: string;
  privateKey?: string;
  passphrase?: string;
}
