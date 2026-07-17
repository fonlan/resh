/** Frontend types for GitHub Releases based app updates. */

export interface UpdateAssetInfo {
  name: string
  browserDownloadUrl: string
  size: number
  digest?: string | null
}

export interface UpdateInfo {
  /** Opaque backend id; download only accepts this (never client URLs). */
  id: string
  tagName: string
  version: string
  name?: string | null
  body?: string | null
  htmlUrl: string
  publishedAt?: string | null
  installAsset: UpdateAssetInfo
  checksumsAsset: UpdateAssetInfo
  currentVersion: string
}

export type CheckUpdateResult =
  | {
      status: "upToDate"
      currentVersion: string
      latestVersion: string
      tagName: string
      fromCache?: boolean
    }
  | {
      status: "updateAvailable"
      update: UpdateInfo
      fromCache?: boolean
    }
  | {
      status: "rateLimited"
      message: string
      retryAfterSecs?: number | null
    }
  | {
      status: "error"
      message: string
      code: string
    }

export interface PreparedUpdate {
  id: string
  version: string
  tagName: string
  assetName: string
  size: number
  sha256: string
  currentVersion: string
}

export interface DownloadProgressEvent {
  downloadId: string
  received: number
  total?: number | null
  phase: string
}

/**
 * High-level UI / lifecycle status for the update flow.
 */
export type UpdateStatus =
  | "idle"
  | "checking"
  | "upToDate"
  | "available"
  | "downloading"
  | "ready"
  | "waitingForSafeRestart"
  | "restarting"
  | "error"

export type UpdateErrorCode =
  | "network"
  | "incomplete"
  | "proxy"
  | "rateLimited"
  | "download"
  | "checksum"
  | "cancelled"
  | "restartBlocked"
  | "unknown"

export type OperationCategory =
  | "configWrite"
  | "webdavSync"
  | "sftpTransfer"
  | "sftpEditUpload"

export interface CategoryCount {
  category: OperationCategory
  count: number
}

export interface OperationSnapshot {
  mode: "normal" | "draining"
  total: number
  categories: CategoryCount[]
  idle: boolean
}

export interface WaitUntilIdleResult {
  idle: boolean
  snapshot: OperationSnapshot
}

/** Local frontend blockers not tracked by the backend coordinator. */
export interface FrontendRestartBlockers {
  dirtyEditors: Array<{ tabId: string; label: string }>
  savingEditors: Array<{ tabId: string; label: string }>
  recordingTabs: Array<{ tabId: string; label: string }>
  settingsSaving: boolean
  settingsSaveError: string | null
  settingsDirty: boolean
}

export type SnapshotTab =
  | {
      kind: "terminal"
      id: string
      label: string
      serverId: string
      temporaryServer?: import("./server").Server | null
    }
  | {
      kind: "editor"
      id: string
      label: string
      serverId: string
      remotePath: string
      language: string
      terminalTabId?: string | null
    }

export interface SnapshotSplitLayout {
  layout: string
  tabIds: string[]
}

export interface RestartSessionSnapshot {
  schemaVersion: number
  token: string
  sourceVersion: string
  targetVersion: string
  createdAt: number
  expiresAt: number
  tabs: SnapshotTab[]
  activeTabId?: string | null
  splitView?: SnapshotSplitLayout | null
  rememberedSplitViews?: Record<string, unknown>
}

export interface PendingRestartSession {
  token: string
  snapshot: RestartSessionSnapshot
}
