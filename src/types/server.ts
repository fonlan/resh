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
  snippets?: import('./snippet').Snippet[];
  additionalPrompt?: string | null;
  synced: boolean;
  updatedAt: string;
}

export interface PortForward {
  local: number;
  remote: number;
}
