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

/** One flattened models.dev catalog entry (wire view). */
export interface CatalogEntry {
  provider: string
  id: string
  name?: string | null
  context?: number | null
  output?: number | null
}

/** Result of one catalog lookup: best match + runner-up candidates. */
export interface CatalogLookup {
  best: CatalogEntry | null
  candidates: CatalogEntry[]
}

/** Catalog cache status shown on the AI settings page. */
export interface CatalogStatus {
  entries: number
  providers: number
  /** Epoch ms the cache was last fetched (0 = never). */
  fetchedAt: number
  fresh: boolean
  ttlDays: number
  /** Epoch ms of the next scheduled refresh. */
  nextAutoRefreshAt: number
}
