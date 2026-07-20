import type { RemoteFileRevision } from "../main/types"

export interface FileEntry {
  name: string
  path: string
  is_dir: boolean
  is_symlink?: boolean
  target_is_dir?: boolean
  link_target?: string
  size: number
  modified: number
  permissions?: number
  children?: FileEntry[]
  isExpanded?: boolean
  isLoading?: boolean
}

export interface DirectoryListResult {
  path: string
  files: FileEntry[]
  error: string | null
}

export interface DirectoryListingHandle {
  token: string
  total: number
}

export interface DirectoryListingPage {
  files: FileEntry[]
  total: number
  next_offset: number | null
}

export interface SftpOpenTextFileResult {
  sessionId: string
  remotePath: string
  localPath: string
  content: string
  encoding: string
  languageHint?: string
  revision: RemoteFileRevision
}

export interface SftpSaveTextFileSavedOutcome {
  status: "saved"
  revision: RemoteFileRevision
}

export interface SftpSaveTextFileConflictOutcome {
  status: "conflict"
  reason: "metadataChanged" | "deleted" | string
  currentRevision: RemoteFileRevision
  remoteContent: string | null
  remoteEncoding: string | null
  snapshotError: string | null
}

export type SftpSaveTextFileOutcome =
  | SftpSaveTextFileSavedOutcome
  | SftpSaveTextFileConflictOutcome

export interface SftpCheckTextFileUnchangedOutcome {
  status: "unchanged"
  revision: RemoteFileRevision
}

export interface SftpCheckTextFileChangedOutcome {
  status: "changed"
  reason: "metadataChanged" | "deleted" | string
  currentRevision: RemoteFileRevision
  remoteContent: string | null
  remoteEncoding: string | null
  snapshotError: string | null
}

export type SftpCheckTextFileOutcome =
  | SftpCheckTextFileUnchangedOutcome
  | SftpCheckTextFileChangedOutcome

export interface CopyFallbackModalState {
  isOpen: boolean
  sessionId: string
  sourcePath: string
  destPath: string
  targetPath: string
}

export interface ContextSubmenuPosition {
  top: number
  left: number
  maxHeight: number
}

export interface ContextMenuPosition {
  top: number
  left: number
}

export type SortType = "name" | "modified"
export type SortOrder = "asc" | "desc"

export interface SortState {
  type: SortType
  order: SortOrder
}

export const DEFAULT_SORT_STATE: SortState = { type: "name", order: "asc" }

export interface ClipboardState {
  sourcePath: string
  sourceName: string
  isDir: boolean
  isCut: boolean
  sessionId: string
}

export interface SessionState {
  rootFiles: FileEntry[]
  currentPath: string
  sortState: SortState
  isLoading: boolean
}

export interface FavoriteTarget {
  serverId: string
  path: string
}
