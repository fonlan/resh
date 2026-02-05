import type { Server, Authentication, ProxyConfig, Snippet, AIChannel, AIModel, GeneralSettings } from './index';

export interface Config {
  version: string;
  servers: Server[];
  authentications: Authentication[];
  proxies: ProxyConfig[];
  snippets: Snippet[];
  aiChannels: AIChannel[];
  aiModels: AIModel[];
  additionalPrompt?: string | null;
  additionalPromptUpdatedAt?: string | null;
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
  rememberMe?: boolean;
}
