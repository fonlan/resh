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
 * waitingForSafeRestart / restarting are reserved for later install phases.
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
  | "unknown"
