import { useEffect, useRef } from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"
import { useConfig } from "./useConfig"
import { useUpdateStore } from "../stores/useUpdateStore"
import type {
  CheckUpdateResult,
  DownloadProgressEvent,
  PreparedUpdate,
  UpdateErrorCode,
} from "../types/update"

/** Delay before first auto-check after config is ready. */
export const UPDATE_STARTUP_DELAY_MS = 8_000
/** Periodic auto-check interval. */
export const UPDATE_INTERVAL_MS = 6 * 60 * 60 * 1000
/** Minimum gap before a visibility/resume catch-up check (same as interval). */
export const UPDATE_RESUME_MIN_GAP_MS = UPDATE_INTERVAL_MS

const mapErrorCode = (code?: string | null): UpdateErrorCode => {
  switch (code) {
    case "network_error":
      return "network"
    case "incomplete_release":
      return "incomplete"
    case "proxy_not_found":
      return "proxy"
    case "rate_limited":
      return "rateLimited"
    default:
      return "unknown"
  }
}

const isBusyStatus = (status: string) =>
  status === "checking" ||
  status === "downloading" ||
  status === "waitingForSafeRestart" ||
  status === "restarting"

/**
 * Global update lifecycle: startup delay, 6h interval, resume catch-up,
 * manual check, download, and progress events.
 *
 * Mount once under ConfigProvider after config has loaded (e.g. App or MainWindow).
 */
export function useUpdateManager() {
  const { config, loading } = useConfig()
  const autoCheck = config?.general.update?.autoCheck ?? true

  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const startupTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const inFlightRef = useRef(false)
  const lastAutoCheckAtRef = useRef(0)
  const mountedRef = useRef(true)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
    }
  }, [])

  // Load current version once.
  useEffect(() => {
    if (loading) return
    let cancelled = false
    void (async () => {
      try {
        const version = await invoke<string>("get_app_version_cmd")
        if (!cancelled && mountedRef.current) {
          useUpdateStore.getState().setCurrentVersion(version)
        }
      } catch {
        // Non-fatal; About tab can still use Tauri getVersion().
      }
    })()
    return () => {
      cancelled = true
    }
  }, [loading])

  // Progress listener (once).
  useEffect(() => {
    let unlisten: UnlistenFn | undefined
    let disposed = false

    void (async () => {
      try {
        unlisten = await listen<DownloadProgressEvent>(
          "update-download-progress",
          (event) => {
            if (disposed) return
            const store = useUpdateStore.getState()
            if (store.status !== "downloading") return
            store.applyProgressEvent(event.payload)
          },
        )
      } catch (err) {
        console.error("Failed to listen for update download progress:", err)
      }
    })()

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  const runCheck = async (options: {
    force: boolean
    manual: boolean
  }): Promise<void> => {
    const store = useUpdateStore.getState()
    // Block while a download (or cancel cleanup) is still in flight on either side.
    if (
      inFlightRef.current ||
      inFlightDownload ||
      isBusyStatus(store.status)
    ) {
      return
    }

    // Preserve available/ready while auto-checking unless forcing a full refresh.
    const preserveAvailable =
      !options.manual &&
      (store.status === "available" || store.status === "ready")

    inFlightRef.current = true
    if (!preserveAvailable) {
      store.setStatus("checking")
      store.setError(null)
      if (options.manual) {
        store.setLastManualResult(null)
      }
    }

    try {
      const result = await invoke<CheckUpdateResult>("check_for_update_cmd", {
        force: options.force,
      })
      if (!mountedRef.current) return

      const s = useUpdateStore.getState()
      s.setLastCheckedAt(Date.now())
      if (!options.manual) {
        lastAutoCheckAtRef.current = Date.now()
      }

      switch (result.status) {
        case "upToDate": {
          s.setCurrentVersion(result.currentVersion)
          if (!preserveAvailable || options.manual) {
            s.setUpdate(null)
            s.setPrepared(null)
            s.setProgress(null)
            s.setStatus("upToDate")
          }
          if (options.manual) {
            s.setLastManualResult("upToDate")
          }
          break
        }
        case "updateAvailable": {
          s.setUpdate(result.update)
          s.setCurrentVersion(result.update.currentVersion)
          // Keep prepared if same version already downloaded.
          if (
            s.prepared &&
            s.prepared.version === result.update.version &&
            s.prepared.tagName === result.update.tagName
          ) {
            s.setStatus("ready")
          } else {
            s.setPrepared(null)
            s.setProgress(null)
            s.setStatus("available")
          }
          if (options.manual) {
            s.setLastManualResult("available")
          }
          break
        }
        case "rateLimited": {
          s.setError(result.message, "rateLimited")
          if (!preserveAvailable) {
            s.setStatus("error")
          }
          if (options.manual) {
            s.setLastManualResult("error")
          }
          break
        }
        case "error": {
          s.setError(result.message, mapErrorCode(result.code))
          if (!preserveAvailable) {
            s.setStatus("error")
          }
          if (options.manual) {
            s.setLastManualResult("error")
          }
          break
        }
        default:
          break
      }
    } catch (err) {
      if (!mountedRef.current) return
      const message = err instanceof Error ? err.message : String(err)
      const s = useUpdateStore.getState()
      s.setError(message, "network")
      if (!preserveAvailable) {
        s.setStatus("error")
      }
      if (options.manual) {
        s.setLastManualResult("error")
      }
    } finally {
      inFlightRef.current = false
    }
  }

  // Expose actions on the store via a stable module-level bridge is awkward;
  // callers invoke check/download through helpers below after manager mounts.
  useEffect(() => {
    updateManagerApi.check = (manual: boolean) =>
      runCheck({ force: true, manual })
    updateManagerApi.download = downloadUpdateAction
    updateManagerApi.cancelDownload = cancelDownloadAction
    updateManagerApi.openDialog = () =>
      useUpdateStore.getState().setDialogOpen(true)
    updateManagerApi.closeDialog = () =>
      useUpdateStore.getState().setDialogOpen(false)
  })

  // Startup + interval when autoCheck is enabled.
  useEffect(() => {
    if (loading) return

    const clearTimers = () => {
      if (startupTimerRef.current) {
        clearTimeout(startupTimerRef.current)
        startupTimerRef.current = null
      }
      if (intervalRef.current) {
        clearInterval(intervalRef.current)
        intervalRef.current = null
      }
    }

    clearTimers()

    if (!autoCheck) {
      return clearTimers
    }

    startupTimerRef.current = setTimeout(() => {
      void runCheck({ force: false, manual: false })
    }, UPDATE_STARTUP_DELAY_MS)

    intervalRef.current = setInterval(() => {
      void runCheck({ force: false, manual: false })
    }, UPDATE_INTERVAL_MS)

    return clearTimers
    // autoCheck is the only scheduling config we care about for timers.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loading, autoCheck])

  // Visibility / resume catch-up (one check if interval elapsed).
  useEffect(() => {
    if (loading || !autoCheck) return

    const maybeCatchUp = () => {
      if (document.visibilityState !== "visible") return
      const last = lastAutoCheckAtRef.current
      const now = Date.now()
      if (last > 0 && now - last < UPDATE_RESUME_MIN_GAP_MS) {
        return
      }
      // Also skip if still within startup delay window and never checked.
      if (last === 0 && now - (performance.timeOrigin || 0) < UPDATE_STARTUP_DELAY_MS) {
        return
      }
      // Mark immediately to avoid focus+visibility double-fire storms.
      lastAutoCheckAtRef.current = now
      void runCheck({ force: false, manual: false })
    }

    const onVisibility = () => {
      if (document.visibilityState === "visible") {
        maybeCatchUp()
      }
    }

    document.addEventListener("visibilitychange", onVisibility)
    window.addEventListener("focus", maybeCatchUp)
    return () => {
      document.removeEventListener("visibilitychange", onVisibility)
      window.removeEventListener("focus", maybeCatchUp)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loading, autoCheck])
}

async function downloadUpdateAction(): Promise<void> {
  const store = useUpdateStore.getState()
  const update = store.update
  if (!update?.id) {
    store.setError("No update available to download", "unknown")
    store.setStatus("error")
    return
  }
  if (store.status === "downloading" || inFlightDownload) {
    return
  }
  if (store.status === "checking") {
    return
  }

  inFlightDownload = true
  store.setError(null)
  store.setStatus("downloading")
  store.setProgress({
    received: 0,
    total: update.installAsset.size || null,
    phase: "asset",
    percent: update.installAsset.size ? 0 : null,
  })

  try {
    const prepared = await invoke<PreparedUpdate>("download_update_cmd", {
      updateId: update.id,
    })
    const s = useUpdateStore.getState()
    s.setPrepared(prepared)
    s.setProgress({
      received: prepared.size,
      total: prepared.size,
      phase: "ready",
      percent: 100,
    })
    s.setStatus("ready")
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    const s = useUpdateStore.getState()
    const cancelled =
      message.toLowerCase().includes("cancel") ||
      message.toLowerCase().includes("cancelled")
    if (cancelled) {
      s.setError(null)
      s.setProgress(null)
      s.setStatus(s.update ? "available" : "idle")
    } else {
      s.setError(message, "download")
      // Keep update info so user can retry.
      s.setStatus("error")
      s.setProgress(null)
    }
  } finally {
    inFlightDownload = false
  }
}

async function cancelDownloadAction(): Promise<void> {
  // Only signal backend cancel. Keep status=downloading / inFlightDownload until
  // downloadUpdateAction's promise settles so checks cannot race the cleanup.
  try {
    await invoke("cancel_update_download_cmd")
  } catch (err) {
    console.warn("cancel_update_download_cmd:", err)
  }
}

let inFlightDownload = false

/** Imperative API for About / dialog / title bar (stable across renders). */
export const updateManagerApi = {
  check: async (_manual: boolean) => {},
  download: async () => {},
  cancelDownload: async () => {},
  openDialog: () => {},
  closeDialog: () => {},
  /**
   * Request safe restart after download. MainWindow should register
   * `requestSafeRestart` to collect session snapshot + blockers.
   */
  requestSafeRestart: async (): Promise<void> => {
    throw new Error("Safe restart is not registered")
  },
  cancelSafeRestart: async (): Promise<void> => {
    const { cancelSafeRestart } = await import("../utils/restartUpdate")
    await cancelSafeRestart()
  },
  /** Resume soft drain wait after RESTART_WAIT_TIMEOUT (keep waiting). */
  continueRestartWait: async (): Promise<void> => {
    const { continueRestartWait } = await import("../utils/restartUpdate")
    continueRestartWait()
  },
}

export function formatBytes(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n) || n < 0) return "—"
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`
}
