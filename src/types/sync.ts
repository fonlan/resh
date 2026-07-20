import type { Config } from "./config"

export interface SyncConflict {
  entityType: string
  id: string
  displayName: string
  kind: string
  resolutionToken: string
}

export type SyncOutcome =
  | { status: "applied"; changedEntityCount: number }
  | { status: "conflicts"; conflicts: SyncConflict[] }
  | { status: "concurrentRemoteChange"; message: string }
  | {
      status: "failed"
      error: {
        kind: string
        message: string
      }
    }

export interface TriggerSyncResult {
  config: Config | null
  outcome: SyncOutcome
}
