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
  /** Optional provider/model context capacity in tokens. */
  contextWindow?: number
  /** Tokens reserved for the next model response. */
  responseReserve?: number
  enabled: boolean
  synced: boolean
  updatedAt: string
}
