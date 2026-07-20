import type { Config } from "./config"

export type SyncResolutionChoice = "keepLocal" | "useRemote"

export interface SyncEntitySummary {
  displayName: string
  details: string
  present: boolean
}

export interface SyncConflict {
  entityType: string
  id: string
  displayName: string
  kind: string
  local: SyncEntitySummary
  remote: SyncEntitySummary
  resolutionToken: string
}

export interface SyncConflictAttempt {
  conflicts: SyncConflict[]
  attemptToken: string
}

export interface SyncResolution {
  entityType: string
  id: string
  choice: SyncResolutionChoice
  resolutionToken: string
}

export type SyncOutcome =
  | { status: "applied"; changedEntityCount: number }
  | {
      status: "conflicts"
      conflicts: SyncConflict[]
      attemptToken: string
    }
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
