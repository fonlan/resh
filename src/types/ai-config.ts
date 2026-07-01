export interface AIChannel {
  id: string
  name: string
  type: "openai" | "copilot" | "anthropic"
  endpoint?: string
  apiKey?: string
  proxyId?: string | null
  isActive: boolean
  synced: boolean
  updatedAt: string
}

export interface AIModel {
  id: string
  name: string
  channelId: string
  enabled: boolean
  synced: boolean
  updatedAt: string
}
