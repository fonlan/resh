export interface EditorFileRef {
  serverId: string
  sessionId: string
  remotePath: string
  localPath: string
}

export interface EditorDocument extends EditorFileRef {
  content: string
  language: string
  encoding: string
  isDirty: boolean
}

export interface EditorAIContext {
  tabId: string
  remotePath: string
  language: string
  content: string
}
