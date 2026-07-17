import { create } from "zustand"
import type {
  DownloadProgressEvent,
  PreparedUpdate,
  UpdateErrorCode,
  UpdateInfo,
  UpdateStatus,
} from "../types/update"

export interface UpdateDownloadProgress {
  received: number
  total: number | null
  phase: string
  /** 0–100 when total known, else null */
  percent: number | null
}

interface UpdateState {
  status: UpdateStatus
  currentVersion: string | null
  update: UpdateInfo | null
  prepared: PreparedUpdate | null
  progress: UpdateDownloadProgress | null
  errorMessage: string | null
  errorCode: UpdateErrorCode | null
  lastCheckedAt: number | null
  /** Manual check feedback for About tab (cleared on next action). */
  lastManualResult: "upToDate" | "available" | "error" | null
  dialogOpen: boolean
  /**
   * Soft drain wait timeout: still in maintenance; UI may offer keep-waiting
   * or cancel (never force-kill).
   */
  restartWaitTimedOut: boolean

  setStatus: (status: UpdateStatus) => void
  setCurrentVersion: (version: string | null) => void
  setUpdate: (update: UpdateInfo | null) => void
  setPrepared: (prepared: PreparedUpdate | null) => void
  setProgress: (progress: UpdateDownloadProgress | null) => void
  applyProgressEvent: (event: DownloadProgressEvent) => void
  setError: (message: string | null, code?: UpdateErrorCode | null) => void
  setLastCheckedAt: (ts: number | null) => void
  setLastManualResult: (
    result: "upToDate" | "available" | "error" | null,
  ) => void
  setDialogOpen: (open: boolean) => void
  setRestartWaitTimedOut: (timedOut: boolean) => void
  resetToIdle: () => void
}

const progressFromEvent = (
  event: DownloadProgressEvent,
): UpdateDownloadProgress => {
  const total =
    event.total != null && event.total > 0 ? Number(event.total) : null
  const received = Number(event.received) || 0
  const percent =
    total != null && total > 0
      ? Math.min(100, Math.round((received / total) * 100))
      : null
  return {
    received,
    total,
    phase: event.phase,
    percent,
  }
}

export const useUpdateStore = create<UpdateState>((set) => ({
  status: "idle",
  currentVersion: null,
  update: null,
  prepared: null,
  progress: null,
  errorMessage: null,
  errorCode: null,
  lastCheckedAt: null,
  lastManualResult: null,
  dialogOpen: false,
  restartWaitTimedOut: false,

  setStatus: (status) => set({ status }),
  setCurrentVersion: (currentVersion) => set({ currentVersion }),
  setUpdate: (update) => set({ update }),
  setPrepared: (prepared) => set({ prepared }),
  setProgress: (progress) => set({ progress }),
  applyProgressEvent: (event) =>
    set({ progress: progressFromEvent(event) }),
  setError: (errorMessage, errorCode = "unknown") =>
    set({
      errorMessage,
      errorCode: errorMessage ? errorCode : null,
    }),
  setLastCheckedAt: (lastCheckedAt) => set({ lastCheckedAt }),
  setLastManualResult: (lastManualResult) => set({ lastManualResult }),
  setDialogOpen: (dialogOpen) => set({ dialogOpen }),
  setRestartWaitTimedOut: (restartWaitTimedOut) => set({ restartWaitTimedOut }),
  resetToIdle: () =>
    set({
      status: "idle",
      update: null,
      prepared: null,
      progress: null,
      errorMessage: null,
      errorCode: null,
      lastManualResult: null,
      restartWaitTimedOut: false,
    }),
}))

/** Selectors to avoid re-rendering on every progress tick. */
export const selectUpdateStatus = (s: UpdateState) => s.status
export const selectUpdateInfo = (s: UpdateState) => s.update
export const selectUpdateProgress = (s: UpdateState) => s.progress
export const selectUpdateDialogOpen = (s: UpdateState) => s.dialogOpen
export const selectUpdateErrorMessage = (s: UpdateState) => s.errorMessage
export const selectUpdateErrorCode = (s: UpdateState) => s.errorCode
export const selectPreparedUpdate = (s: UpdateState) => s.prepared
export const selectRestartWaitTimedOut = (s: UpdateState) =>
  s.restartWaitTimedOut
export const selectShowTitleUpdateButton = (s: UpdateState) =>
  s.status === "available" ||
  s.status === "downloading" ||
  s.status === "ready" ||
  s.status === "error" ||
  s.status === "waitingForSafeRestart" ||
  s.status === "restarting"
