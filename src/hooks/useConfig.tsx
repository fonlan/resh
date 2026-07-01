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
import { Config } from "../types"
import { logger } from "../utils/logger"

interface ConfigContextType {
  config: Config | null
  loading: boolean
  error: string | null
  loadConfig: () => Promise<void>
  saveConfig: (config: Config) => Promise<void>
  recordServerConnection: (serverId: string) => Promise<void>
  triggerSync: () => Promise<Config>
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

  // configRef 始终指向最新的 config，不受 React 渲染节奏影响
  const configRef = useRef<Config | null>(null)
  const setConfigSafe = useCallback((next: Config | null) => {
    configRef.current = next
    setConfig(next)
  }, [])
  const getLatestConfig = useCallback(() => configRef.current, [])

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

    // Trigger background sync if enabled
    if (
      loadedConfig?.general?.webdav?.enabled &&
      loadedConfig?.general?.webdav?.url
    ) {
      logger.info("[ConfigProvider] Initiating background startup sync...")
      invoke<Config>("trigger_sync")
        .then((syncedConfig) => {
          logger.info("[ConfigProvider] Startup sync completed")
          setConfigSafe(syncedConfig)
        })
        .catch((err) => {
          logger.warn("[ConfigProvider] Startup sync failed", err)
        })
    }
  }, [setConfigSafe])

  const saveConfig = useCallback(
    async (newConfig: Config) => {
      try {
        logger.info("[ConfigProvider] Saving config...")
        await invoke("save_config", { config: newConfig })
        logger.info("[ConfigProvider] Config saved successfully")

        setConfigSafe(newConfig)
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
      const cfg = await invoke<Config>("trigger_sync")
      logger.info("[ConfigProvider] Sync successful")
      setConfigSafe(cfg)
      return cfg
    } catch (err) {
      logger.error("[ConfigProvider] Sync failed", err)
      throw err
    }
  }, [setConfigSafe])

  useEffect(() => {
    loadConfig()
  }, [loadConfig])

  useEffect(() => {
    let unlisten: (() => void) | undefined

    listen<Config>("config-updated", (event) => {
      logger.info("[ConfigProvider] Config updated from background sync")
      setConfigSafe(event.payload)
    }).then((fn) => {
      unlisten = fn
    })

    return () => {
      if (unlisten) unlisten()
    }
  }, [setConfigSafe])

  return (
    <ConfigContext.Provider
      value={{
        config,
        loading,
        error,
        loadConfig,
        saveConfig,
        recordServerConnection,
        triggerSync,
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
