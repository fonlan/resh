import React, {
  createContext,
  use,
  useCallback,
  useEffect,
  useRef,
  useState,
  ReactNode,
} from "react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import {
  Config,
  SyncConflictAttempt,
  SyncOutcome,
  SyncResolution,
  SyncStatus,
  TriggerSyncResult,
} from "../types"
import { logger } from "../utils/logger"

interface ConfigContextType {
  config: Config | null
  loading: boolean
  error: string | null
  syncConflictAttempt: SyncConflictAttempt | null
  /** RFC3339 timestamp of the last successful sync; null when unknown or never synced. */
  lastSyncedAt: string | null
  loadConfig: () => Promise<void>
  saveConfig: (config: Config) => Promise<void>
  recordServerConnection: (serverId: string) => Promise<void>
  triggerSync: () => Promise<TriggerSyncResult>
  resolveSyncConflicts: (
    attemptToken: string,
    resolutions: SyncResolution[],
  ) => Promise<TriggerSyncResult>
  /**
   * 同步取出当前 Provider 内最新的 Config 引用。
   * 用于 useCallback 闭包：避免依赖里漏写 `config` 时拿到 stale 快照，
   * 且不需要每次都 invoke("get_config") 走一次跨进程 IPC。
   * 在初次加载尚未完成时返回 null，调用方需自行兜底。
   */
  getLatestConfig: () => Config | null
}

const ConfigContext = createContext<ConfigContextType | undefined>(undefined)

export const ConfigProvider: React.FC<{ children: ReactNode }> = ({
  children,
}) => {
  const [config, setConfig] = useState<Config | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [syncConflictAttempt, setSyncConflictAttempt] =
    useState<SyncConflictAttempt | null>(null)
  const [lastSyncedAt, setLastSyncedAt] = useState<string | null>(null)

  // configRef 始终指向最新的 config，不受 React 渲染节奏影响
  const configRef = useRef<Config | null>(null)
  const setConfigSafe = useCallback((next: Config | null) => {
    configRef.current = next
    setConfig(next)
  }, [])
  const getLatestConfig = useCallback(() => configRef.current, [])

  /**
   * Read the persisted "last successful sync" time for the currently configured WebDAV account.
   * The value is owned by the backend (`sync-state.json`), so it survives restarts and stays
   * correct for background syncs this window never explicitly triggered.
   */
  const refreshSyncStatus = useCallback(async () => {
    try {
      const status = await invoke<SyncStatus>("get_sync_status")
      setLastSyncedAt(status.lastSyncedAt)
    } catch (err) {
      // A missing or unreadable sync baseline must never break the settings page.
      logger.warn("[ConfigProvider] Failed to read sync status", err)
    }
  }, [])

  const applySyncResult = useCallback(
    (result: TriggerSyncResult) => {
      if (result.outcome.status === "applied" && result.config) {
        setConfigSafe(result.config)
      }

      if (result.outcome.status === "conflicts") {
        setSyncConflictAttempt({
          conflicts: result.outcome.conflicts,
          attemptToken: result.outcome.attemptToken,
        })
      } else if (result.outcome.status !== "applied") {
        // A rejected or failed resolution invalidates its previous token. The next manual sync
        // creates a fresh attempt rather than letting the UI submit stale choices again.
        setSyncConflictAttempt(null)
      } else {
        // An applied sync has just advanced the persisted last-synced time. Re-read it so an open
        // settings page reflects the sync instead of its previous value.
        void refreshSyncStatus()
      }

      return result
    },
    [refreshSyncStatus, setConfigSafe],
  )

  const loadConfig = useCallback(async () => {
    let loadedConfig: Config | null = null
    try {
      setLoading(true)
      logger.info("[ConfigProvider] Loading config...")
      loadedConfig = await invoke<Config>("get_config")
      logger.info("[ConfigProvider] Loaded config", {
        version: loadedConfig.version,
      })
      setConfigSafe(loadedConfig)
      setError(null)
    } catch (err) {
      logger.error("[ConfigProvider] Failed to load config", err)
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }

    // Seed the displayed last-synced time independently of whether a startup sync will run: it
    // comes from the local baseline, which also covers syncs made by previous app runs.
    void refreshSyncStatus()

    // Trigger background sync if enabled. A conflict is retained as discoverable context; it
    // never interrupts startup or replaces the in-memory configuration.
    if (
      loadedConfig?.general?.webdav?.enabled &&
      loadedConfig?.general?.webdav?.url
    ) {
      logger.info("[ConfigProvider] Initiating background startup sync...")
      invoke<TriggerSyncResult>("trigger_sync")
        .then((result) => {
          applySyncResult(result)
          if (result.outcome.status === "applied") {
            logger.info("[ConfigProvider] Startup sync applied")
          } else {
            logger.warn(
              "[ConfigProvider] Startup sync did not apply",
              result.outcome,
            )
          }
        })
        .catch((err) => {
          logger.warn("[ConfigProvider] Startup sync failed", err)
        })
    }
  }, [applySyncResult, refreshSyncStatus, setConfigSafe])

  const saveConfig = useCallback(
    async (newConfig: Config) => {
      try {
        logger.info("[ConfigProvider] Saving config...")
        await invoke("save_config", { config: newConfig })
        logger.info("[ConfigProvider] Config saved successfully")

        setConfigSafe(newConfig)
        setSyncConflictAttempt(null)
        setError(null)
      } catch (err) {
        logger.error("[ConfigProvider] Failed to save config", err)
        setError(err instanceof Error ? err.message : String(err))
        throw err
      }
    },
    [setConfigSafe],
  )

  const recordServerConnection = useCallback(
    async (serverId: string) => {
      try {
        const cfg = await invoke<Config>("record_server_connection", { serverId })
        setConfigSafe(cfg)
        setError(null)
      } catch (err) {
        logger.error("[ConfigProvider] Failed to record server connection", err)
        setError(err instanceof Error ? err.message : String(err))
        throw err
      }
    },
    [setConfigSafe],
  )

  const triggerSync = useCallback(async () => {
    try {
      logger.info("[ConfigProvider] Triggering sync...")
      const result = await invoke<TriggerSyncResult>("trigger_sync")
      applySyncResult(result)
      logger.info("[ConfigProvider] Sync completed", result.outcome)
      return result
    } catch (err) {
      logger.error("[ConfigProvider] Sync failed", err)
      throw err
    }
  }, [applySyncResult])

  const resolveSyncConflicts = useCallback(
    async (attemptToken: string, resolutions: SyncResolution[]) => {
      try {
        logger.info("[ConfigProvider] Applying sync conflict resolutions")
        const result = await invoke<TriggerSyncResult>(
          "resolve_sync_conflicts",
          { attemptToken, resolutions },
        )
        applySyncResult(result)
        return result
      } catch (err) {
        logger.error("[ConfigProvider] Failed to resolve sync conflicts", err)
        throw err
      }
    },
    [applySyncResult],
  )

  useEffect(() => {
    loadConfig()
  }, [loadConfig])

  useEffect(() => {
    let cancelled = false
    let unlistenConfigUpdated: (() => void) | undefined
    let unlistenSyncConflicts: (() => void) | undefined

    void Promise.all([
      listen<Config>("config-updated", (event) => {
        logger.info("[ConfigProvider] Config updated from background sync")
        setConfigSafe(event.payload)
        // Save-triggered background syncs are otherwise invisible here; re-read the persisted
        // timestamp so the settings page's "last synced" line advances on its own.
        void refreshSyncStatus()
      }),
      listen<SyncOutcome>("sync-conflicts", (event) => {
        if (event.payload.status !== "conflicts") return
        setSyncConflictAttempt({
          conflicts: event.payload.conflicts,
          attemptToken: event.payload.attemptToken,
        })
      }),
    ]).then(([removeConfigUpdated, removeSyncConflicts]) => {
      if (cancelled) {
        removeConfigUpdated()
        removeSyncConflicts()
        return
      }
      unlistenConfigUpdated = removeConfigUpdated
      unlistenSyncConflicts = removeSyncConflicts
    })

    return () => {
      cancelled = true
      unlistenConfigUpdated?.()
      unlistenSyncConflicts?.()
    }
  }, [refreshSyncStatus, setConfigSafe])

  return (
    <ConfigContext.Provider
      value={{
        config,
        loading,
        error,
        syncConflictAttempt,
        lastSyncedAt,
        loadConfig,
        saveConfig,
        recordServerConnection,
        triggerSync,
        resolveSyncConflicts,
        getLatestConfig,
      }}
    >
      {children}
    </ConfigContext.Provider>
  )
}

export const useConfig = () => {
  const context = use(ConfigContext)
  if (context === undefined) {
    throw new Error("useConfig must be used within a ConfigProvider")
  }
  return context
}
