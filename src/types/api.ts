// src/types/api.ts

import { Config } from './config';

export interface ConnectRequest {
  serverId: string;
}

export interface ConnectResponse {
  sessionId: string;
}

export interface TerminalOutputEvent {
  sessionId: string;
  data: string;
}

export interface ConnectionClosedEvent {
  sessionId: string;
  reason: string;
}

export interface ConnectionErrorEvent {
  sessionId: string;
  error: string;
}

export interface SendCommandRequest {
  sessionId: string;
  input: string;
}

export interface CloseSessionRequest {
  sessionId: string;
}

export interface CloneSessionRequest {
  sessionId: string;
}

export interface SaveConfigRequest {
  syncPart: Config;
  localPart: Config;
}

export interface SyncWebDAVRequest {
  // Empty - uses stored WebDAV config
}
