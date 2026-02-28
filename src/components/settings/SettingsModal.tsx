import React, { useState, useEffect, useRef } from "react"
import {
  X,
  Server,
  Key,
  Globe,
  Settings,
  Loader2,
  Check,
  AlertCircle,
  Code,
  RefreshCw,
  Bot,
  Info,
  Folder,
} from "lucide-react"
import {
  Config,
  Server as ServerType,
  Authentication,
  ProxyConfig as ProxyType,
  GeneralSettings,
  Snippet,
  AIChannel,
  AIModel,
} from "../../types"
import { useConfig } from "../../hooks/useConfig"
import { ServerTab } from "./ServerTab"
import { AuthTab } from "./AuthTab"
import { ProxyTab } from "./ProxyTab"
import { GeneralTab } from "./GeneralTab"
import { SyncTab } from "./SyncTab"
import { SnippetsTab } from "./SnippetsTab"
import { AITab } from "./AITab"
import { AboutTab } from "./AboutTab"
import { SFTPTab } from "./SFTPTab"
import { useTranslation } from "../../i18n"

export interface SettingsModalProps {
  isOpen: boolean
  onClose: () => void
  onConnectServer?: (serverId: string) => void
  initialTab?: TabType
  editServerId?: string | null
}

type TabType =
  | "servers"
  | "auth"
  | "proxies"
  | "snippets"
  | "general"
  | "sync"
  | "ai"
  | "about"
  | "sftp"
type SaveStatus = "idle" | "saving" | "saved" | "error"

export const SettingsModal: React.FC<SettingsModalProps> = ({
  isOpen,
  onClose,
  onConnectServer,
  initialTab = "servers",
  editServerId,
}) => {
  const { config, loading, error, saveConfig } = useConfig()
  const { t } = useTranslation()
  const [activeTab, setActiveTab] = useState<TabType>(initialTab)
  const [saveStatus, setSaveStatus] = useState<SaveStatus>("idle")
  const [saveError, setSaveError] = useState<string | null>(null)

  useEffect(() => {
    if (editServerId) {
      setActiveTab("servers")
    }
  }, [editServerId])

  // Local state for config
  const [localConfig, setLocalConfig] = useState<Config | null>(null)

  // Ref for debounce timer
  const saveTimeoutRef = useRef<number | null>(null)

  // Ref for saved timeout
  const savedTimeoutRef = useRef<number | null>(null)

  // Ref to store the last saved config to detect actual changes
  const lastSavedConfigRef = useRef<string | null>(null)

  // Ref to store the original config for comparison
  const originalConfigRef = useRef<Config | null>(null)

  // Ref to track if a save is in progress to prevent race conditions
  const isSavingRef = useRef(false)

  // Ref for the modal container to detect clicks outside
  const modalRef = useRef<HTMLDivElement>(null)
  // Track if mouse down was inside the modal
  const mouseDownInsideRef = useRef(false)

  const initializedRef = useRef(false)

  useEffect(() => {
    if (config && !initializedRef.current) {
      if (!originalConfigRef.current) {
        originalConfigRef.current = config
      }

      setLocalConfig(config)
      setSaveError(null)
      lastSavedConfigRef.current = JSON.stringify(config)
      initializedRef.current = true
    }
  }, [config])

  useEffect(() => {
    if (!isOpen) {
      initializedRef.current = false
      setLocalConfig(null)
    }
  }, [isOpen])

  // Auto-save when localConfig changes
  useEffect(() => {
    if (!localConfig) {
      return
    }

    // Check if config actually changed
    const currentConfigStr = JSON.stringify(localConfig)

    if (currentConfigStr === lastSavedConfigRef.current) {
      return // No actual changes, skip save
    }

    // Don't start a new save if one is already in progress
    if (isSavingRef.current) {
      return
    }

    // Clear existing save timeout
    if (saveTimeoutRef.current) {
      clearTimeout(saveTimeoutRef.current)
    }

    // Set saving status
    setSaveStatus("saving")
    setSaveError(null)

    // Debounce: wait 800ms after last change before saving
    saveTimeoutRef.current = window.setTimeout(async () => {
      isSavingRef.current = true
      try {
        await saveConfig(localConfig)

        lastSavedConfigRef.current = JSON.stringify(localConfig)

        setSaveStatus("saved")

        // Reset to idle after 1.5 seconds
        savedTimeoutRef.current = window.setTimeout(() => {
          setSaveStatus("idle")
        }, 1500)
      } catch (err) {
        setSaveStatus("error")
        setSaveError(
          err instanceof Error ? err.message : "Failed to save configuration",
        )
      } finally {
        isSavingRef.current = false
      }
    }, 800)

    // Cleanup timeout on unmount
    return () => {
      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current)
      }
    }
  }, [localConfig, saveConfig])

  const handleOverlayMouseDown = (e: React.MouseEvent) => {
    if (modalRef.current && modalRef.current.contains(e.target as Node)) {
      mouseDownInsideRef.current = true
    } else {
      mouseDownInsideRef.current = false
    }
  }

  const handleOverlayMouseUp = (e: React.MouseEvent) => {
    // Only close if mouse down and mouse up were both outside the modal
    if (
      !mouseDownInsideRef.current &&
      modalRef.current &&
      !modalRef.current.contains(e.target as Node)
    ) {
      onClose()
    }
    mouseDownInsideRef.current = false
  }

  if (!isOpen) {
    return null
  }

  if (loading) {
    return (
      <div
        className="fixed inset-0 flex items-center justify-center z-[1000] animate-in fade-in duration-300"
        style={{
          background: "rgba(2, 6, 23, 0.4)",
          backdropFilter: "blur(12px) saturate(180%)",
        }}
        onMouseDown={handleOverlayMouseDown}
        onMouseUp={handleOverlayMouseUp}
      >
        <div
          className="flex flex-col items-center justify-center gap-4 bg-[var(--bg-secondary)] rounded-lg shadow-2xl max-w-[900px] w-[90%] h-[80vh] overflow-hidden relative animate-in slide-in-from-bottom-2 duration-400"
          style={{
            boxShadow:
              "0 25px 50px -12px rgba(0, 0, 0, 0.5), 0 0 0 1px var(--glass-border), inset 0 1px 1px rgba(255, 255, 255, 0.05)",
          }}
          ref={modalRef}
        >
          <Loader2
            className="animate-spin"
            size={32}
            style={{ color: "var(--text-secondary)" }}
          />
          <p style={{ color: "var(--text-secondary)" }}>{t.common.loading}</p>
        </div>
      </div>
    )
  }

  if (!localConfig) {
    return null
  }

  const handleServersUpdate = (servers: ServerType[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, servers } : null))
  }

  const handleAuthUpdate = (authentications: Authentication[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, authentications } : null))
  }

  const handleProxiesUpdate = (proxies: ProxyType[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, proxies } : null))
  }

  const handleSnippetsUpdate = (snippets: Snippet[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, snippets } : null))
  }

  const handleGeneralUpdate = (general: GeneralSettings) => {
    setLocalConfig((prev) => (prev ? { ...prev, general } : null))
  }

  const handleAIChannelsUpdate = (aiChannels: AIChannel[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, aiChannels } : null))
  }

  const handleAIModelsUpdate = (aiModels: AIModel[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, aiModels } : null))
  }

  const handleAdditionalPromptUpdate = (prompt: string | null | undefined) => {
    setLocalConfig((prev) =>
      prev
        ? {
            ...prev,
            additionalPrompt: prompt === null ? undefined : prompt,
            additionalPromptUpdatedAt:
              prompt !== null && prompt !== undefined
                ? new Date().toISOString()
                : undefined,
          }
        : null,
    )
  }

  const handleConnectServer = async (serverId: string) => {
    // If there's a pending save, flush it immediately
    if (saveTimeoutRef.current) {
      clearTimeout(saveTimeoutRef.current)
      saveTimeoutRef.current = null

      if (localConfig) {
        setSaveStatus("saving")
        try {
          await saveConfig(localConfig)
          setSaveStatus("saved")
          // Short delay to show "saved" status before closing/connecting
          await new Promise((resolve) => setTimeout(resolve, 300))
        } catch (err) {
          setSaveStatus("error")
          // If save failed, we probably shouldn't proceed with connection
          // as the server might not exist in the backend yet
          return
        }
      }
    } else if (isSavingRef.current) {
      // If currently saving, wait for it to complete
      let checks = 0
      while (isSavingRef.current && checks < 20) {
        await new Promise((resolve) => setTimeout(resolve, 100))
        checks++
      }
    }

    onConnectServer?.(serverId)
  }

  const tabs: { id: TabType; label: string; icon: React.ReactNode }[] = [
    { id: "servers", label: t.servers, icon: <Server size={18} /> },
    { id: "auth", label: t.auth, icon: <Key size={18} /> },
    { id: "proxies", label: t.proxies, icon: <Globe size={18} /> },
    { id: "snippets", label: t.snippets, icon: <Code size={18} /> },
    { id: "ai", label: t.ai.tabTitle, icon: <Bot size={18} /> },
    { id: "sftp", label: t.sftp.title, icon: <Folder size={18} /> },
    { id: "sync", label: t.sync, icon: <RefreshCw size={18} /> },
    { id: "general", label: t.general, icon: <Settings size={18} /> },
    { id: "about", label: t.about.tabTitle, icon: <Info size={18} /> },
  ]

  return (
    <div
      className="fixed inset-0 flex items-center justify-center z-[1000] animate-in fade-in duration-300"
      style={{
        background: "rgba(2, 6, 23, 0.4)",
        backdropFilter: "blur(12px) saturate(180%)",
      }}
      onMouseDown={handleOverlayMouseDown}
      onMouseUp={handleOverlayMouseUp}
    >
      <div
        className="absolute inset-0 pointer-events-none opacity-[0.03]"
        style={{
          backgroundImage:
            "url(\"data:image/svg+xml,%3Csvg viewBox='0 0 200 200' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noiseFilter'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.65' numOctaves='3' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noiseFilter)'/%3E%3C/svg%3E\")",
        }}
      />

      <div
        className="relative bg-[var(--bg-secondary)] rounded-lg max-w-[900px] w-[90%] h-[80vh] flex flex-col overflow-hidden animate-in slide-in-from-bottom-2 duration-400"
        style={{
          boxShadow:
            "0 25px 50px -12px rgba(0, 0, 0, 0.5), 0 0 0 1px var(--glass-border), inset 0 1px 1px rgba(255, 255, 255, 0.05)",
        }}
        ref={modalRef}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-[var(--glass-border)] bg-[var(--bg-secondary)]">
          <div className="flex items-center gap-4">
            <h2 className="text-[18px] font-bold text-[var(--text-primary)] m-0">
              {t.settings}
            </h2>
            {/* Save Status Indicator */}
            {saveStatus !== "idle" && (
              <div
                className="flex items-center gap-[6px] text-[12px] px-[10px] py-1 rounded-[20px] font-medium"
                style={
                  saveStatus === "saving"
                    ? {
                        color: "var(--accent-primary)",
                        background: "rgba(59, 130, 246, 0.1)",
                      }
                    : saveStatus === "saved"
                      ? {
                          color: "var(--accent-success)",
                          background: "rgba(34, 197, 94, 0.1)",
                        }
                      : {
                          color: "var(--color-danger)",
                          background: "rgba(239, 68, 68, 0.1)",
                        }
                }
              >
                {saveStatus === "saving" && (
                  <>
                    <Loader2 size={14} className="animate-spin" />
                    <span>{t.saveStatus.saving}</span>
                  </>
                )}
                {saveStatus === "saved" && (
                  <>
                    <Check size={14} />
                    <span>{t.saveStatus.saved}</span>
                  </>
                )}
                {saveStatus === "error" && (
                  <>
                    <AlertCircle size={14} />
                    <span>{t.saveStatus.error}</span>
                  </>
                )}
              </div>
            )}
          </div>
          <button
            type="button"
            onClick={onClose}
            className="bg-transparent border-none text-[var(--text-muted)] cursor-pointer p-1.5 flex items-center justify-center transition-all rounded hover:text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)]"
          >
            <X size={20} />
          </button>
        </div>

        {/* Content */}
        <div className="flex flex-1 overflow-hidden">
          {/* Sidebar */}
          <div className="w-[200px] bg-[var(--bg-primary)] border-r border-[var(--glass-border)] p-3 flex flex-col gap-1">
            {tabs.map((tab) => (
              <button
                type="button"
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`
                  w-full text-left px-[14px] py-2.5 rounded bg-transparent border-none
                  text-[var(--text-secondary)] cursor-pointer text-[13px] font-medium
                  transition-all duration-200 flex items-center gap-2.5 relative
                  hover:bg-[rgba(255,255,255,0.03)] hover:text-[var(--text-primary)] hover:translate-x-0.5
                  ${activeTab === tab.id ? "active" : ""}
                `}
                style={
                  activeTab === tab.id
                    ? {
                        background: "var(--bg-tertiary)",
                        color: "var(--accent-primary)",
                        boxShadow:
                          "0 1px 2px rgba(0, 0, 0, 0.1), inset 0 1px 0 rgba(255, 255, 255, 0.05)",
                      }
                    : {}
                }
              >
                {activeTab === tab.id && (
                  <span
                    className="absolute -left-3 top-[20%] bottom-[20%] w-[3px] rounded-r"
                    style={{
                      background: "var(--accent-primary)",
                      boxShadow: "0 0 10px var(--accent-primary)",
                    }}
                  />
                )}
                {tab.icon}
                <span className="relative z-[1]">{tab.label}</span>
              </button>
            ))}
          </div>

          {/* Tab Content */}
          <div
            className={`flex-1 p-6 bg-[var(--bg-secondary)] ${activeTab === "about" ? "overflow-y-hidden" : "overflow-y-auto"}`}
          >
            {error && (
              <div className="bg-[rgba(239,68,68,0.1)] border border-[rgba(239,68,68,0.2)] text-[var(--color-danger)] px-4 py-3 rounded mb-5 text-[13px]">
                {error}
              </div>
            )}
            {saveError && (
              <div className="bg-[rgba(239,68,68,0.1)] border border-[rgba(239,68,68,0.2)] text-[var(--color-danger)] px-4 py-3 rounded mb-5 text-[13px]">
                {saveError}
              </div>
            )}

            {activeTab === "servers" && (
              <ServerTab
                servers={localConfig.servers}
                authentications={localConfig.authentications}
                proxies={localConfig.proxies}
                snippets={localConfig.snippets}
                onServersUpdate={handleServersUpdate}
                onConnectServer={handleConnectServer}
                editServerId={editServerId}
              />
            )}
            {activeTab === "auth" && (
              <AuthTab
                authentications={localConfig.authentications}
                onAuthUpdate={handleAuthUpdate}
                servers={localConfig.servers}
                onServersUpdate={handleServersUpdate}
              />
            )}
            {activeTab === "proxies" && (
              <ProxyTab
                proxies={localConfig.proxies}
                onProxiesUpdate={handleProxiesUpdate}
                servers={localConfig.servers}
                onServersUpdate={handleServersUpdate}
              />
            )}
            {activeTab === "snippets" && (
              <SnippetsTab
                snippets={localConfig.snippets || []}
                onSnippetsUpdate={handleSnippetsUpdate}
              />
            )}
            {activeTab === "ai" && (
              <AITab
                aiChannels={localConfig.aiChannels || []}
                aiModels={localConfig.aiModels || []}
                proxies={localConfig.proxies || []}
                general={localConfig.general}
                additionalPrompt={localConfig.additionalPrompt}
                onAIChannelsUpdate={handleAIChannelsUpdate}
                onAIModelsUpdate={handleAIModelsUpdate}
                onGeneralUpdate={handleGeneralUpdate}
                onAdditionalPromptUpdate={handleAdditionalPromptUpdate}
              />
            )}
            {activeTab === "sftp" && (
              <SFTPTab
                config={localConfig}
                onChange={(newConfig) => setLocalConfig(newConfig)}
              />
            )}
            {activeTab === "general" && (
              <GeneralTab
                general={localConfig.general}
                onGeneralUpdate={handleGeneralUpdate}
              />
            )}
            {activeTab === "sync" && (
              <SyncTab
                general={localConfig.general}
                onGeneralUpdate={handleGeneralUpdate}
              />
            )}
            {activeTab === "about" && <AboutTab />}
          </div>
        </div>
      </div>
    </div>
  )
}
