import React, {
  useState,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  Suspense,
} from "react"
import {
  Settings,
  X,
  Code,
  Circle,
  MessageSquare,
  Folder,
  Download,
  Loader2,
  AlertCircle,
  ArrowDownCircle,
} from "lucide-react"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { Config, EditorAIContext } from "../types"
// SettingsModal is now lazy loaded
const SettingsModal = React.lazy(() =>
  import("./settings/SettingsModal").then((module) => ({
    default: module.SettingsModal,
  })),
)
const AISidebar = React.lazy(() =>
  import("./AISidebar").then((module) => ({ default: module.AISidebar })),
)
const SFTPSidebar = React.lazy(() =>
  import("./SFTPSidebar").then((module) => ({ default: module.SFTPSidebar })),
)
const SnippetsSidebar = React.lazy(() =>
  import("./SnippetsSidebar").then((module) => ({
    default: module.SnippetsSidebar,
  })),
)
const TerminalTab = React.lazy(() =>
  import("./TerminalTab").then((module) => ({ default: module.TerminalTab })),
)
const EditorTab = React.lazy(() =>
  import("./EditorTab").then((module) => ({ default: module.EditorTab })),
)
import { WindowControls } from "./WindowControls"
import { WelcomeScreen } from "./WelcomeScreen"
import { NewTabButton } from "./NewTabButton"
import type { QuickConnectTarget } from "./NewTabButton"
import { SplitViewButton, SplitLayout } from "./SplitViewButton"
import { SplitTabPickerModal } from "./SplitTabPickerModal"
import { TabContextMenu } from "./TabContextMenu"
import { ServerContextMenu } from "./ServerContextMenu"
import { ToastContainer, ToastItem } from "./Toast"
import { useConfig } from "../hooks/useConfig"
import { generateId } from "../utils/idGenerator"
import { getRecentServers } from "../utils/recentServers"
import { useTranslation } from "../i18n"
import { useTabDragDrop } from "../hooks/useTabDragDrop"
import { EmojiText } from "./EmojiText"
import { useTransferStore } from "../stores/transferStore"
import {
  useUpdateStore,
  selectShowTitleUpdateButton,
  selectUpdateStatus,
  selectUpdateProgress,
  selectUpdateDialogOpen,
} from "../stores/useUpdateStore"
import { UpdateDialog } from "./UpdateDialog"
import { updateManagerApi } from "../hooks/useUpdateManager"
import type { SettingsSaveApi } from "./settings/SettingsModal"
import type {
  FrontendRestartBlockers,
  RestartSessionSnapshot,
  SnapshotTab,
} from "../types/update"
import {
  ackRestartSession,
  cancelSafeRestart,
  getPendingRestartSession,
  prepareSafeRestart,
} from "../utils/restartUpdate"
import {
  isEditorTab,
  isTerminalTab,
  type EditorDocumentState,
  type EditorTabState,
  type OpenEditorTabPayload,
  type PendingDirtyEditorAction,
  type SplitViewState,
  type Tab,
} from "./main/types"
import {
  DEFAULT_FIXED_TAB_WIDTH,
  EMPTY_AUTHENTICATIONS,
  EMPTY_PROXIES,
  EMPTY_SERVERS,
  getFileNameFromPath,
  MAX_FIXED_TAB_WIDTH,
  MIN_FIXED_TAB_WIDTH,
  SPLIT_LAYOUT_REQUIRED_TABS,
} from "./main/helpers"
import { useTabLayoutMeasurement } from "./main/useTabLayoutMeasurement"
import { isMacOS } from "../utils/platform"

type RememberedSplitViews = Record<string, SplitViewState>

const isRememberedSplitViewValid = (
  splitView: SplitViewState,
  validTabIds: Set<string>,
): boolean => {
  const requiredTabs = SPLIT_LAYOUT_REQUIRED_TABS[splitView.layout]
  return (
    splitView.tabIds.length >= requiredTabs &&
    splitView.tabIds.every((splitTabId) => validTabIds.has(splitTabId))
  )
}

const pruneRememberedSplitViews = (
  splitViews: RememberedSplitViews,
  validTabIds: Set<string>,
): RememberedSplitViews => {
  let changed = false
  const next: RememberedSplitViews = {}

  Object.entries(splitViews).forEach(([tabId, splitView]) => {
    const hasValidOwner = validTabIds.has(tabId)

    if (!hasValidOwner || !isRememberedSplitViewValid(splitView, validTabIds)) {
      changed = true
      return
    }

    next[tabId] = splitView
  })

  return changed ? next : splitViews
}

export const MainWindow: React.FC = () => {
  const { config, saveConfig, recordServerConnection, getLatestConfig } =
    useConfig()
  const { t } = useTranslation()
  const [tabs, setTabs] = useState<Tab[]>([])
  const [activeTabId, setActiveTabId] = useState<string | null>(null)
  const [isSettingsOpen, setIsSettingsOpen] = useState(false)
  const [settingsInitialTab, setSettingsInitialTab] = useState<
    "servers" | "auth" | "proxies" | "snippets" | "general"
  >("servers")
  const [isSnippetsOpen, setIsSnippetsOpen] = useState(false)
  const [isSFTPOpen, setIsSFTPOpen] = useState(false)
  const [isAIOpen, setIsAIOpen] = useState(false)
  const [isSidebarsInitialized, setIsSidebarsInitialized] = useState(false)
  const [hasLoadedSnippetsSidebar, setHasLoadedSnippetsSidebar] =
    useState(false)
  const [hasLoadedSFTPSidebar, setHasLoadedSFTPSidebar] = useState(false)
  const [hasLoadedAISidebar, setHasLoadedAISidebar] = useState(false)

  const initListener = useTransferStore((state) => state.initListener)

  // Slice update store so download progress does not re-render the whole window.
  const showUpdateButton = useUpdateStore(selectShowTitleUpdateButton)
  const updateStatus = useUpdateStore(selectUpdateStatus)
  const updateProgress = useUpdateStore(selectUpdateProgress)
  const updateDialogOpen = useUpdateStore(selectUpdateDialogOpen)

  // Initialize transfer store listener
  useEffect(() => {
    let cleanup: (() => void) | undefined
    initListener().then((unlisten) => {
      cleanup = unlisten
    })
    return () => {
      cleanup?.()
    }
  }, [initListener])

  useEffect(() => {
    if (config && !isSidebarsInitialized) {
      if (config.general.sftpSidebarLocked) {
        setHasLoadedSFTPSidebar(true)
        setIsSFTPOpen(true)
      }
      if (config.general.aiSidebarLocked) {
        setHasLoadedAISidebar(true)
        setIsAIOpen(true)
      }
      if (config.general.snippetsSidebarLocked) {
        setHasLoadedSnippetsSidebar(true)
        setIsSnippetsOpen(true)
      }
      setIsSidebarsInitialized(true)
    }
  }, [config, isSidebarsInitialized])

  const [toasts, setToasts] = useState<ToastItem[]>([])
  const showToast = useCallback(
    (message: string, type: ToastItem["type"] = "info", duration?: number) => {
      const id = generateId()
      setToasts((prev) => [...prev, { id, type, message, duration }])
    },
    [],
  )

  // Listen for sync failed events
  useEffect(() => {
    let isMounted = true

    const syncFailedListener = listen<string>("sync-failed", (event) => {
      if (isMounted) {
        showToast(`同步失败: ${event.payload}`, "error")
      }
    })

    return () => {
      isMounted = false
      syncFailedListener.then((unlisten) => unlisten())
    }
  }, [showToast])

  const removeToast = (id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id))
  }

  useEffect(() => {
    if (config?.general.sftpSidebarLocked !== undefined) {
      setTimeout(() => {
        window.dispatchEvent(new CustomEvent("resh-force-terminal-resize"))
      }, 250)
    }
  }, [config?.general.sftpSidebarLocked])

  const [contextMenu, setContextMenu] = useState<{
    x: number
    y: number
    tabId: string
  } | null>(null)
  const [serverContextMenu, setServerContextMenu] = useState<{
    x: number
    y: number
    serverId: string
  } | null>(null)
  const [editServerId, setEditServerId] = useState<string | null>(null)
  const [recordingTabs, setRecordingTabs] = useState<Set<string>>(new Set())
  const settingsSaveApiRef = useRef<SettingsSaveApi | null>(null)
  const sessionRestoreAttemptedRef = useRef(false)
  const sessionRestoreCompletedOldIdsRef = useRef(new Set<string>())
  /** Full old tab id → new tab id across restore attempts (for split remaps). */
  const sessionRestoreIdRemapRef = useRef(new Map<string, string>())
  const sessionRestoreRetryCountRef = useRef(0)
  const [sessionRestoreRetryTick, setSessionRestoreRetryTick] = useState(0)
  const [tabSessions, setTabSessions] = useState<Record<string, string>>({}) // tabId -> sessionId
  const [editorDocuments, setEditorDocuments] = useState<
    Record<string, EditorDocumentState>
  >({})
  const [pendingDirtyAction, setPendingDirtyAction] =
    useState<PendingDirtyEditorAction | null>(null)
  const [splitView, setSplitView] = useState<SplitViewState | null>(null)
  const [rememberedSplitViews, setRememberedSplitViews] =
    useState<RememberedSplitViews>({})
  const [pendingSplitLayout, setPendingSplitLayout] =
    useState<SplitLayout | null>(null)
  const {
    titleBarRef,
    tabListRef,
    rightControlsRef,
    newTabButtonRef,
    tabListMaxWidth,
    newTabButtonWidth,
  } = useTabLayoutMeasurement(tabs.length)
  const [isTabListOverflowing, setIsTabListOverflowing] = useState(false)

  const servers = config?.servers || EMPTY_SERVERS

  const collectFrontendRestartBlockers = useCallback((): FrontendRestartBlockers => {
    const dirtyEditors: FrontendRestartBlockers["dirtyEditors"] = []
    const savingEditors: FrontendRestartBlockers["savingEditors"] = []
    const recording: FrontendRestartBlockers["recordingTabs"] = []

    for (const tab of tabs) {
      if (isEditorTab(tab)) {
        if (tab.dirty) {
          dirtyEditors.push({ tabId: tab.id, label: tab.label })
        }
        const doc = editorDocuments[tab.id]
        if (doc?.isSaving) {
          savingEditors.push({ tabId: tab.id, label: tab.label })
        }
      }
      if (isTerminalTab(tab) && recordingTabs.has(tab.id)) {
        recording.push({ tabId: tab.id, label: tab.label })
      }
    }

    const settingsStatus = settingsSaveApiRef.current?.getStatus()
    return {
      dirtyEditors,
      savingEditors,
      recordingTabs: recording,
      settingsSaving: settingsStatus?.isSaving ?? false,
      settingsSaveError: settingsStatus?.saveError ?? null,
      settingsDirty: settingsStatus?.dirty ?? false,
    }
  }, [tabs, editorDocuments, recordingTabs])

  const buildRestartSnapshot = useCallback((): RestartSessionSnapshot => {
    const prepared = useUpdateStore.getState().prepared
    const update = useUpdateStore.getState().update
    const currentVersion =
      useUpdateStore.getState().currentVersion ??
      update?.currentVersion ??
      prepared?.currentVersion ??
      "0.0.0"
    const targetVersion =
      prepared?.version ?? update?.version ?? currentVersion

    const snapshotTabs: SnapshotTab[] = []
    for (const tab of tabs) {
      if (isTerminalTab(tab)) {
        snapshotTabs.push({
          kind: "terminal",
          id: tab.id,
          label: tab.label,
          serverId: tab.serverId,
          temporaryServer: tab.temporaryServer ?? null,
        })
        continue
      }
      if (isEditorTab(tab)) {
        // Dirty editors must not enter the snapshot.
        if (tab.dirty) continue
        const doc = editorDocuments[tab.id]
        if (doc?.isSaving) continue
        snapshotTabs.push({
          kind: "editor",
          id: tab.id,
          label: tab.label,
          serverId: tab.serverId,
          remotePath: tab.remotePath,
          language: tab.language || "plaintext",
          terminalTabId: null,
        })
      }
    }

    const nowSec = Math.floor(Date.now() / 1000)
    return {
      schemaVersion: 1,
      token: "",
      sourceVersion: currentVersion,
      targetVersion,
      createdAt: nowSec,
      expiresAt: nowSec + 24 * 60 * 60,
      tabs: snapshotTabs,
      activeTabId,
      splitView: splitView
        ? {
            layout: splitView.layout,
            tabIds: splitView.tabIds,
          }
        : null,
      rememberedSplitViews: rememberedSplitViews as Record<string, unknown>,
    }
  }, [
    tabs,
    editorDocuments,
    activeTabId,
    splitView,
    rememberedSplitViews,
  ])

  const handleRequestSafeRestart = useCallback(async () => {
    // Flush settings if open.
    if (isSettingsOpen && settingsSaveApiRef.current) {
      const flush = await settingsSaveApiRef.current.flush()
      if (!flush.ok) {
        showToast(
          flush.error || t.updateRestartBlockedSettings,
          "error",
        )
        return
      }
    }

    const blockers = collectFrontendRestartBlockers()
    if (blockers.dirtyEditors.length > 0) {
      showToast(t.updateRestartBlockedDirty, "error")
      return
    }
    if (blockers.savingEditors.length > 0) {
      showToast(t.updateRestartBlockedSaving, "error")
      return
    }
    if (blockers.recordingTabs.length > 0) {
      showToast(t.updateRestartBlockedRecording, "error")
      return
    }
    if (
      blockers.settingsSaving ||
      blockers.settingsDirty ||
      blockers.settingsSaveError
    ) {
      showToast(t.updateRestartBlockedSettings, "error")
      return
    }

    const snapshot = buildRestartSnapshot()
    try {
      await prepareSafeRestart({
        snapshot,
        blockers,
      })
      // Phase 3: snapshot + draining complete. Install helper / process exit is Phase 4.
      // Keep restarting status briefly so UI shows ready-for-install; do not exit.
      // Never surface restore tokens (one-time capability).
      showToast(
        "Safe restart prepared. Install will complete in a later update.",
        "info",
        6000,
      )
      // Return to ready so user can retry after install lands.
      useUpdateStore.getState().setStatus("ready")
      await cancelSafeRestart()
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      if (message === "RESTART_CANCELLED") {
        // User cancelled while waiting; cancelSafeRestart already restored status.
        return
      }
      if (message === "RESTART_WAIT_TIMEOUT") {
        showToast(t.updateRestartTimeout, "error")
        // stay in waiting; user can keep waiting or cancel
        return
      }
      await cancelSafeRestart().catch(() => {})
      useUpdateStore.getState().setStatus("ready")
      if (message === "DIRTY_EDITORS") {
        showToast(t.updateRestartBlockedDirty, "error")
      } else if (message === "SAVING_EDITORS") {
        showToast(t.updateRestartBlockedSaving, "error")
      } else if (message === "RECORDING_TABS") {
        showToast(t.updateRestartBlockedRecording, "error")
      } else if (message === "SETTINGS_PENDING") {
        showToast(t.updateRestartBlockedSettings, "error")
      } else {
        showToast(message, "error")
      }
    }
  }, [
    isSettingsOpen,
    collectFrontendRestartBlockers,
    buildRestartSnapshot,
    showToast,
    t,
  ])

  useEffect(() => {
    updateManagerApi.requestSafeRestart = handleRequestSafeRestart
    updateManagerApi.cancelSafeRestart = cancelSafeRestart
    return () => {
      updateManagerApi.requestSafeRestart = async () => {
        throw new Error("Safe restart is not registered")
      }
    }
  }, [handleRequestSafeRestart])

  // Restore tabs after update install (CLI --restore-update-session).
  // Full success → ack & delete snapshot. Partial failure keeps snapshot and
  // allows limited in-session retries without recreating already restored tabs.
  // Terminals are connected first so editors can reopen; final tab bar order
  // and activeTabId follow the snapshot sequence after remapping.
  useEffect(() => {
    if (!config || sessionRestoreAttemptedRef.current) return
    sessionRestoreAttemptedRef.current = true
    let cancelled = false

    void (async () => {
      try {
        const pending = await getPendingRestartSession()
        if (!pending || cancelled) {
          // Strict Mode remount / effect re-run: allow a fresh attempt when
          // this run was aborted before any restore work.
          if (cancelled && sessionRestoreCompletedOldIdsRef.current.size === 0) {
            sessionRestoreAttemptedRef.current = false
          }
          return
        }

        const snap = pending.snapshot
        const failures: string[] = []
        const idRemap = sessionRestoreIdRemapRef.current
        const already = sessionRestoreCompletedOldIdsRef.current
        const newTerminalsThisPass: Tab[] = []

        // 1) Restore missing terminal tabs (without recordServerConnection).
        for (const tab of snap.tabs) {
          if (tab.kind !== "terminal") continue
          if (already.has(tab.id)) continue
          const newId = generateId()
          if (tab.temporaryServer) {
            newTerminalsThisPass.push({
              id: newId,
              kind: "terminal",
              label: tab.label,
              serverId: tab.temporaryServer.id || tab.serverId,
              temporaryServer: tab.temporaryServer,
            })
            idRemap.set(tab.id, newId)
            already.add(tab.id)
          } else {
            const serverExists = config.servers.some((s) => s.id === tab.serverId)
            if (!serverExists) {
              failures.push(`${tab.label}: server missing`)
              continue
            }
            newTerminalsThisPass.push({
              id: newId,
              kind: "terminal",
              label: tab.label,
              serverId: tab.serverId,
            })
            idRemap.set(tab.id, newId)
            already.add(tab.id)
          }
        }

        if (newTerminalsThisPass.length > 0) {
          setTabs((prev) => {
            if (prev.length === 0) return newTerminalsThisPass
            const existingIds = new Set(prev.map((t) => t.id))
            return [
              ...prev,
              ...newTerminalsThisPass.filter((t) => !existingIds.has(t.id)),
            ]
          })
        }

        // 2) Wait briefly for terminal sessions to register via onSessionChange.
        if (newTerminalsThisPass.length > 0 || snap.tabs.some((t) => t.kind === "editor")) {
          await new Promise((r) => setTimeout(r, 1200))
        }
        if (cancelled) return

        // 3) Reopen saved editor tabs by remotePath (new sessionId/localPath).
        let latestSessions: Record<string, string> = {}
        setTabSessions((prev) => {
          latestSessions = prev
          return prev
        })
        await new Promise((r) => setTimeout(r, 0))

        let allTerminals: Tab[] = []
        setTabs((prev) => {
          allTerminals = prev.filter((t) => isTerminalTab(t))
          return prev
        })
        await new Promise((r) => setTimeout(r, 0))

        const resolveSessionForServer = async (
          serverId: string,
        ): Promise<string | undefined> => {
          for (const term of allTerminals) {
            if (isTerminalTab(term) && term.serverId === serverId) {
              const sid = latestSessions[term.id]
              if (sid) return sid
            }
          }
          await new Promise((r) => setTimeout(r, 800))
          setTabSessions((prev) => {
            latestSessions = prev
            return prev
          })
          await new Promise((r) => setTimeout(r, 0))
          for (const term of allTerminals) {
            if (isTerminalTab(term) && term.serverId === serverId) {
              const sid = latestSessions[term.id]
              if (sid) return sid
            }
          }
          return undefined
        }

        for (const tab of snap.tabs) {
          if (tab.kind !== "editor") continue
          if (already.has(tab.id)) continue

          const sessionId = await resolveSessionForServer(tab.serverId)
          if (!sessionId) {
            failures.push(`${tab.label}: no SSH session`)
            continue
          }
          try {
            const opened = await invoke<{
              sessionId: string
              remotePath: string
              localPath: string
              content: string
              encoding: string
              languageHint?: string | null
            }>("sftp_open_text_file", {
              sessionId,
              remotePath: tab.remotePath,
            })
            const editorTabId = generateId()
            const language =
              opened.languageHint || tab.language || "plaintext"
            setTabs((prev) => [
              ...prev,
              {
                id: editorTabId,
                kind: "editor",
                label: tab.label,
                serverId: tab.serverId,
                sessionId: opened.sessionId,
                remotePath: opened.remotePath,
                localPath: opened.localPath,
                dirty: false,
                language,
              },
            ])
            setEditorDocuments((prev) => ({
              ...prev,
              [editorTabId]: {
                content: opened.content,
                savedContent: opened.content,
                encoding: opened.encoding,
                isSaving: false,
              },
            }))
            idRemap.set(tab.id, editorTabId)
            already.add(tab.id)
          } catch (e) {
            failures.push(
              `${tab.label}: ${e instanceof Error ? e.message : String(e)}`,
            )
          }
        }

        if (cancelled) return

        // 4) Reorder tabs to snapshot order and set activeTabId after full remap.
        setTabs((prev) => {
          const byNewId = new Map(prev.map((t) => [t.id, t]))
          const ordered: Tab[] = []
          const used = new Set<string>()
          for (const tab of snap.tabs) {
            const newId = idRemap.get(tab.id)
            if (!newId) continue
            const live = byNewId.get(newId)
            if (!live || used.has(newId)) continue
            ordered.push(live)
            used.add(newId)
          }
          for (const t of prev) {
            if (!used.has(t.id)) ordered.push(t)
          }
          return ordered
        })

        if (snap.activeTabId) {
          const activeNew = idRemap.get(snap.activeTabId)
          if (activeNew) setActiveTabId(activeNew)
        } else if (idRemap.size > 0) {
          setActiveTabId((prev) => {
            if (prev) return prev
            const first = snap.tabs
              .map((t) => idRemap.get(t.id))
              .find((id): id is string => !!id)
            return first || null
          })
        }

        // 5) Restore current + remembered split layouts with full remap.
        const remapSplit = (
          layout: { layout: string; tabIds: string[] } | null | undefined,
        ): SplitViewState | null => {
          if (!layout) return null
          const remapped = layout.tabIds
            .map((id) => idRemap.get(id))
            .filter((id): id is string => !!id)
          if (remapped.length < 2) return null
          return {
            layout: layout.layout as SplitLayout,
            tabIds: remapped,
          }
        }

        if (snap.splitView) {
          const current = remapSplit(snap.splitView)
          if (current) setSplitView(current)
        }

        if (snap.rememberedSplitViews) {
          const nextRemembered: RememberedSplitViews = {}
          for (const [oldKey, value] of Object.entries(
            snap.rememberedSplitViews,
          )) {
            const newKey = idRemap.get(oldKey)
            if (!newKey) continue
            const raw = value as {
              layout?: string
              tabIds?: string[]
            } | null
            if (!raw || !raw.layout || !Array.isArray(raw.tabIds)) continue
            const remapped = remapSplit({
              layout: raw.layout,
              tabIds: raw.tabIds,
            })
            if (remapped) nextRemembered[newKey] = remapped
          }
          if (Object.keys(nextRemembered).length > 0) {
            setRememberedSplitViews((prev) => ({ ...prev, ...nextRemembered }))
          }
        }

        // Permanent failures (missing server) should not block ack of the rest.
        const permanentOnly =
          failures.length > 0 &&
          failures.every((f) => f.includes("server missing"))
        const hasRetryableFailure =
          failures.length > 0 &&
          failures.some((f) => !f.includes("server missing"))
        const expectedRestorable = snap.tabs.filter((tab) => {
          if (tab.kind === "terminal" && !tab.temporaryServer) {
            return config.servers.some((s) => s.id === tab.serverId)
          }
          return true
        }).length
        const fullyRestored =
          already.size >= expectedRestorable && !hasRetryableFailure

        if (fullyRestored || permanentOnly) {
          try {
            await ackRestartSession(pending.token)
            if (failures.length > 0) {
              showToast(
                t.updateRestorePartial.replace(
                  "{details}",
                  failures.slice(0, 3).join("; "),
                ),
                "error",
                8000,
              )
            } else {
              showToast(t.updateRestoreSuccess, "success")
            }
          } catch {
            sessionRestoreAttemptedRef.current = false
            if (sessionRestoreRetryCountRef.current < 2) {
              sessionRestoreRetryCountRef.current += 1
              window.setTimeout(() => {
                setSessionRestoreRetryTick((n) => n + 1)
              }, 1500)
            }
            showToast(
              t.updateRestorePartial.replace("{details}", "ack failed"),
              "error",
            )
          }
        } else {
          showToast(
            t.updateRestorePartial.replace(
              "{details}",
              failures.slice(0, 3).join("; ") || "incomplete restore",
            ),
            "error",
            8000,
          )
          sessionRestoreAttemptedRef.current = false
          if (hasRetryableFailure && sessionRestoreRetryCountRef.current < 2) {
            sessionRestoreRetryCountRef.current += 1
            window.setTimeout(() => {
              setSessionRestoreRetryTick((n) => n + 1)
            }, 2000)
          }
        }
      } catch (e) {
        // Transient load/parse failures: allow limited retry this session.
        const detail =
          e instanceof Error ? e.message : String(e ?? "restore failed")
        showToast(
          t.updateRestorePartial.replace("{details}", detail.slice(0, 120)),
          "error",
          8000,
        )
        sessionRestoreAttemptedRef.current = false
        if (sessionRestoreRetryCountRef.current < 2) {
          sessionRestoreRetryCountRef.current += 1
          window.setTimeout(() => {
            setSessionRestoreRetryTick((n) => n + 1)
          }, 2000)
        }
      }
    })()

    return () => {
      cancelled = true
      // Allow remount (Strict Mode) to retry when nothing was restored yet.
      if (sessionRestoreCompletedOldIdsRef.current.size === 0) {
        sessionRestoreAttemptedRef.current = false
      }
    }
  }, [config, showToast, t, sessionRestoreRetryTick])

  const authentications = config?.authentications || EMPTY_AUTHENTICATIONS
  const proxies = config?.proxies || EMPTY_PROXIES

  const serverById = useMemo(() => {
    const map = new Map<string, Config["servers"][number]>()
    servers.forEach((server) => {
      map.set(server.id, server)
    })
    return map
  }, [servers])
  const temporaryServerById = useMemo(() => {
    const map = new Map<string, Config["servers"][number]>()
    tabs.forEach((tab) => {
      if (isTerminalTab(tab) && tab.temporaryServer) {
        map.set(tab.serverId, tab.temporaryServer)
      }
    })
    return map
  }, [tabs])

  const allServers = useMemo(
    () => [
      ...servers,
      ...tabs
        .filter(isTerminalTab)
        .map((tab) => tab.temporaryServer)
        .filter((server): server is Config["servers"][number] => !!server),
    ],
    [servers, tabs],
  )

  const getEditorTabDisplayLabel = (tab: EditorTabState): string => {
    const serverName =
      serverById.get(tab.serverId)?.name ||
      temporaryServerById.get(tab.serverId)?.name
    if (!serverName) {
      return tab.label
    }
    return `${tab.label}@${serverName}`
  }
  const editorContextByTabId = useMemo<Record<string, EditorAIContext>>(() => {
    const next: Record<string, EditorAIContext> = {}
    tabs.forEach((tab) => {
      if (!isEditorTab(tab)) {
        return
      }
      const doc = editorDocuments[tab.id]
      if (!doc) {
        return
      }
      next[tab.id] = {
        tabId: tab.id,
        remotePath: tab.remotePath,
        language: tab.language,
        content: doc.content,
      }
    })
    return next
  }, [tabs, editorDocuments])

  const activeTab = tabs.find((tab) => tab.id === activeTabId) || null

  const activeServerId = activeTab?.serverId
  const activeSFTPSessionId = activeTab
    ? isEditorTab(activeTab)
      ? activeTab.sessionId
      : tabSessions[activeTab.id] || undefined
    : undefined
  const activeAISshSessionId = activeTab
    ? isEditorTab(activeTab)
      ? activeTab.sessionId
      : tabSessions[activeTab.id] || undefined
    : undefined
  const activeAICurrentTabId = activeTab
    ? isEditorTab(activeTab)
      ? activeTab.id
      : tabSessions[activeTab.id] || undefined
    : undefined
  const shouldRenderSFTPSidebar = !activeTab || isTerminalTab(activeTab)

  const handleTabSessionChange = (tabId: string, sessionId: string | null) => {
    const normalizedSessionId = sessionId || ""
    setTabSessions((prev) => {
      if (prev[tabId] === normalizedSessionId) {
        return prev
      }

      return {
        ...prev,
        [tabId]: normalizedSessionId,
      }
    })
  }

  const triggerTerminalResize = useCallback(() => {
    setTimeout(() => {
      window.dispatchEvent(new CustomEvent("resh-force-terminal-resize"))
    }, 40)
  }, [])

  const handleExitSplitView = useCallback(() => {
    const splitTabIds = splitView?.tabIds || []
    setSplitView(null)
    setPendingSplitLayout(null)
    if (splitTabIds.length > 0) {
      setRememberedSplitViews((prev) => {
        let changed = false
        const next = { ...prev }
        splitTabIds.forEach((tabId) => {
          if (tabId in next) {
            delete next[tabId]
            changed = true
          }
        })
        return changed ? next : prev
      })
    }
    triggerTerminalResize()
  }, [splitView, triggerTerminalResize])

  const handleStartSplitSelection = useCallback((layout: SplitLayout) => {
    setPendingSplitLayout(layout)
  }, [])

  const handleConfirmSplitSelection = useCallback(
    (selectedTabIds: string[]) => {
      if (!pendingSplitLayout) {
        return
      }

      const requiredTabs = SPLIT_LAYOUT_REQUIRED_TABS[pendingSplitLayout]
      if (selectedTabIds.length !== requiredTabs) {
        return
      }

      const nextActiveTabId =
        selectedTabIds.includes(activeTabId || "") && activeTabId
          ? activeTabId
          : selectedTabIds[0]

      const nextSplitView: SplitViewState = {
        layout: pendingSplitLayout,
        tabIds: selectedTabIds,
      }

      setSplitView(nextSplitView)
      setRememberedSplitViews((prev) => ({
        ...prev,
        ...Object.fromEntries(
          selectedTabIds.map((tabId) => [tabId, nextSplitView]),
        ),
      }))
      setActiveTabId(nextActiveTabId)
      setPendingSplitLayout(null)
      triggerTerminalResize()
    },
    [pendingSplitLayout, activeTabId, triggerTerminalResize],
  )

  const selectTabImmediate = useCallback(
    (tabId: string) => {
      setSplitView(rememberedSplitViews[tabId] || null)
      setPendingSplitLayout(null)
      setActiveTabId(tabId)
      triggerTerminalResize()
    },
    [rememberedSplitViews, triggerTerminalResize],
  )

  const handleTabSelect = useCallback(
    (tabId: string) => {
      if (tabId === activeTabId) {
        return
      }
      const currentTab = activeTabId
        ? tabs.find((tab) => tab.id === activeTabId) || null
        : null
      if (currentTab && isEditorTab(currentTab) && currentTab.dirty) {
        setPendingDirtyAction({
          type: "switch",
          sourceTabId: currentTab.id,
          nextTabId: tabId,
        })
        return
      }
      selectTabImmediate(tabId)
    },
    [activeTabId, tabs, selectTabImmediate],
  )

  useEffect(() => {
    if (tabs.length === 0) {
      if (splitView) {
        setSplitView(null)
      }
      if (Object.keys(rememberedSplitViews).length > 0) {
        setRememberedSplitViews({})
      }
      if (activeTabId !== null) {
        setActiveTabId(null)
      }
      return
    }

    const validTabIds = new Set(tabs.map((tab) => tab.id))

    setRememberedSplitViews((prev) =>
      pruneRememberedSplitViews(prev, validTabIds),
    )

    if (activeTabId && !validTabIds.has(activeTabId)) {
      const nextActiveTabId = tabs[0].id
      const nextSplitView = rememberedSplitViews[nextActiveTabId] || null
      setActiveTabId(nextActiveTabId)
      setSplitView(
        nextSplitView && isRememberedSplitViewValid(nextSplitView, validTabIds)
          ? nextSplitView
          : null,
      )
      triggerTerminalResize()
      return
    }

    if (!splitView) {
      return
    }

    const filteredTabIds = splitView.tabIds.filter((tabId) =>
      validTabIds.has(tabId),
    )
    const requiredTabs = SPLIT_LAYOUT_REQUIRED_TABS[splitView.layout]

    if (filteredTabIds.length < requiredTabs) {
      setSplitView(null)
      if (filteredTabIds.length > 0) {
        setActiveTabId(filteredTabIds[0])
      }
      triggerTerminalResize()
      return
    }

    const splitTabsChanged =
      filteredTabIds.join("|") !== splitView.tabIds.join("|")
    if (splitTabsChanged) {
      setSplitView({
        ...splitView,
        tabIds: filteredTabIds,
      })
    }

    if (activeTabId && !filteredTabIds.includes(activeTabId)) {
      setActiveTabId(filteredTabIds[0])
    }
  }, [tabs, activeTabId, splitView, rememberedSplitViews, triggerTerminalResize])

  useEffect(() => {
    if (servers.length === 0) return

    setTabs((prevTabs) => {
      let hasChanges = false
      const newTabs = prevTabs.map((tab) => {
        if (!isTerminalTab(tab) || tab.temporaryServer) {
          return tab
        }
        const server = serverById.get(tab.serverId)
        if (server && server.name !== tab.label) {
          hasChanges = true
          return { ...tab, label: server.name }
        }
        return tab
      })
      return hasChanges ? newTabs : prevTabs
    })
  }, [servers.length, servers])

  const {
    draggedIndex: draggedTabIndex,
    dropTargetIndex,
    handleDragStart: handleTabDragStart,
    handleDragOver: handleTabDragOver,
    handleDrop: handleTabDrop,
    handleDragEnd: handleTabDragEnd,
  } = useTabDragDrop(tabs, setTabs)

  const handleTabKeyDown = (e: React.KeyboardEvent, index: number) => {
    if (e.ctrlKey || e.metaKey) {
      if (e.key === "ArrowRight" && index < tabs.length - 1) {
        e.preventDefault()
        const newTabs = [...tabs]
        ;[newTabs[index], newTabs[index + 1]] = [
          newTabs[index + 1],
          newTabs[index],
        ]
        setTabs(newTabs)
      } else if (e.key === "ArrowLeft" && index > 0) {
        e.preventDefault()
        const newTabs = [...tabs]
        ;[newTabs[index], newTabs[index - 1]] = [
          newTabs[index - 1],
          newTabs[index],
        ]
        setTabs(newTabs)
      }
    }
  }

  const closeTabImmediate = useCallback(
    (tabId: string) => {
      const targetTab = tabs.find((tab) => tab.id === tabId)
      if (!targetTab) {
        return
      }
      const newTabs = tabs.filter((t) => t.id !== tabId)
      const validTabIds = new Set(newTabs.map((tab) => tab.id))
      setTabs(newTabs)
      if (activeTabId === tabId) {
        const nextActiveTabId = newTabs.length > 0 ? newTabs[0].id : null
        const nextSplitView = nextActiveTabId
          ? rememberedSplitViews[nextActiveTabId] || null
          : null
        setActiveTabId(nextActiveTabId)
        setSplitView(
          nextSplitView && isRememberedSplitViewValid(nextSplitView, validTabIds)
            ? nextSplitView
            : null,
        )
      } else if (splitView?.tabIds.includes(tabId)) {
        setSplitView(null)
      }
      setRememberedSplitViews((prev) =>
        pruneRememberedSplitViews(prev, validTabIds),
      )
      triggerTerminalResize()
      setTabSessions((prev) => {
        const next = { ...prev }
        delete next[tabId]
        return next
      })
      if (isEditorTab(targetTab)) {
        setEditorDocuments((prev) => {
          if (!prev[tabId]) {
            return prev
          }
          const next = { ...prev }
          delete next[tabId]
          return next
        })
      }
      if (isTerminalTab(targetTab)) {
        setRecordingTabs((prev) => {
          if (!prev.has(tabId)) {
            return prev
          }
          const next = new Set(prev)
          next.delete(tabId)
          return next
        })
      }
    },
    [tabs, activeTabId, rememberedSplitViews, splitView, triggerTerminalResize],
  )

  const handleCloseTab = useCallback(
    (tabId: string) => {
      const targetTab = tabs.find((tab) => tab.id === tabId)
      if (!targetTab) {
        return
      }
      if (isEditorTab(targetTab) && targetTab.dirty) {
        setPendingDirtyAction({
          type: "close",
          tabId: targetTab.id,
        })
        return
      }
      closeTabImmediate(tabId)
    },
    [tabs, closeTabImmediate],
  )

  const handleAddTab = useCallback(
    async (serverId: string) => {
      // 通过 ref 拿最新的 config，避免 useCallback 闭包里 `config` 是 stale 快照；
      // 兜底回退到一次跨进程 invoke，仅在 Provider 尚未加载完成时触发。
      const currentConfig =
        getLatestConfig() ?? (await invoke<Config>("get_config"))
      const server = currentConfig.servers.find((s) => s.id === serverId)

      if (!server) {
        return
      }

      const newTab: Tab = {
        id: generateId(),
        kind: "terminal",
        label: server.name,
        serverId: server.id,
      }

      setTabs((prev) => [...prev, newTab])
      setSplitView(null)
      setPendingSplitLayout(null)
      setActiveTabId(newTab.id)
      triggerTerminalResize()

      await recordServerConnection(serverId)
    },
    [recordServerConnection, getLatestConfig, triggerTerminalResize],
  )

  const handleAddQuickConnectTab = useCallback((target: QuickConnectTarget) => {
    const normalizedHost = target.host.trim()
    if (!normalizedHost) {
      return
    }

    const normalizedUsername = target.username?.trim() || ""
    const temporaryServerId = `temp-${generateId()}`
    const temporaryServer: Config["servers"][number] = {
      id: temporaryServerId,
      name: normalizedUsername
        ? `${normalizedUsername}@${normalizedHost}`
        : normalizedHost,
      group: null,
      host: normalizedHost,
      port: 22,
      username: normalizedUsername,
      authId: null,
      proxyId: null,
      jumphostId: null,
      portForwards: [],
      keepAlive: 0,
      autoExecCommands: [],
      snippets: [],
      sftpFavoritePaths: [],
      additionalPrompt: null,
      synced: false,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }

    const newTab: Tab = {
      id: generateId(),
      kind: "terminal",
      label: temporaryServer.name,
      serverId: temporaryServer.id,
      temporaryServer,
    }

    setTabs((prev) => [...prev, newTab])
    setSplitView(null)
    setPendingSplitLayout(null)
    setActiveTabId(newTab.id)
    triggerTerminalResize()
  }, [triggerTerminalResize])

  const handleAddEditorTab = useCallback(
    (payload: OpenEditorTabPayload) => {
      const serverId = payload.serverId?.trim()
      const sessionId = payload.sessionId?.trim()
      const remotePath = payload.remotePath?.trim()
      const localPath = payload.localPath?.trim()
      const content = payload.content
      const encoding = payload.encoding?.trim()
      const language = payload.language?.trim() || "plaintext"
      if (
        !serverId ||
        !sessionId ||
        !remotePath ||
        !localPath ||
        typeof content !== "string" ||
        !encoding
      ) {
        showToast(t.mainWindow.editor.invalidPayload, "error")
        return
      }
      const explicitLabel = payload.label?.trim()
      const label = explicitLabel || getFileNameFromPath(remotePath)
      const nextDirty = payload.dirty === true
      const newTab: Tab = {
        id: generateId(),
        kind: "editor",
        label,
        serverId,
        sessionId,
        remotePath,
        localPath,
        dirty: nextDirty,
        language,
      }
      setTabs((prev) => [...prev, newTab])
      setEditorDocuments((prev) => ({
        ...prev,
        [newTab.id]: {
          content,
          savedContent: content,
          encoding,
          isSaving: false,
        },
      }))
      setSplitView(null)
      setPendingSplitLayout(null)
      setActiveTabId(newTab.id)
      triggerTerminalResize()
    },
    [showToast, t.mainWindow.editor.invalidPayload, triggerTerminalResize],
  )

  const handleEditorContentChange = useCallback(
    (tabId: string, next: string) => {
      let nextDirty = false
      setEditorDocuments((prev) => {
        const current = prev[tabId]
        if (!current) {
          return prev
        }
        nextDirty = next !== current.savedContent
        if (current.content === next) {
          return prev
        }
        return {
          ...prev,
          [tabId]: {
            ...current,
            content: next,
          },
        }
      })
      setTabs((prev) =>
        prev.map((tab) => {
          if (
            !isEditorTab(tab) ||
            tab.id !== tabId ||
            tab.dirty === nextDirty
          ) {
            return tab
          }
          return { ...tab, dirty: nextDirty }
        }),
      )
    },
    [],
  )

  const handleEditorLanguageChange = useCallback(
    (tabId: string, nextLanguage: string) => {
      const normalizedLanguage = nextLanguage.trim().toLowerCase()
      if (!normalizedLanguage) {
        return
      }
      setTabs((prev) =>
        prev.map((tab) => {
          if (
            !isEditorTab(tab) ||
            tab.id !== tabId ||
            tab.language === normalizedLanguage
          ) {
            return tab
          }
          return {
            ...tab,
            language: normalizedLanguage,
          }
        }),
      )
    },
    [],
  )
  const handleSaveEditorTab = useCallback(
    async (tabId: string): Promise<boolean> => {
      const targetTab = tabs.find((tab): tab is EditorTabState => {
        return tab.id === tabId && isEditorTab(tab)
      })
      if (!targetTab) {
        return false
      }
      const currentDocument = editorDocuments[tabId]
      if (!currentDocument) {
        showToast(t.mainWindow.editor.documentMissing, "error")
        return false
      }
      if (currentDocument.isSaving) {
        return false
      }
      const savingContent = currentDocument.content
      setEditorDocuments((prev) => {
        const doc = prev[tabId]
        if (!doc) {
          return prev
        }
        return {
          ...prev,
          [tabId]: {
            ...doc,
            isSaving: true,
          },
        }
      })
      try {
        await invoke("sftp_save_text_file", {
          sessionId: targetTab.sessionId,
          remotePath: targetTab.remotePath,
          localPath: targetTab.localPath,
          content: savingContent,
          encoding: currentDocument.encoding,
        })
        let nextDirtyAfterSave = false
        setEditorDocuments((prev) => {
          const doc = prev[tabId]
          if (!doc) {
            return prev
          }
          nextDirtyAfterSave = doc.content !== savingContent
          return {
            ...prev,
            [tabId]: {
              ...doc,
              savedContent: savingContent,
              isSaving: false,
            },
          }
        })
        setTabs((prev) =>
          prev.map((tab) =>
            isEditorTab(tab) && tab.id === tabId
              ? { ...tab, dirty: nextDirtyAfterSave }
              : tab,
          ),
        )
        showToast(t.saveStatus.saved, "success")
        return true
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error)
        showToast(
          t.mainWindow.editor.saveFailed.replace("{error}", message),
          "error",
        )
        setEditorDocuments((prev) => {
          const doc = prev[tabId]
          if (!doc) {
            return prev
          }
          return {
            ...prev,
            [tabId]: {
              ...doc,
              isSaving: false,
            },
          }
        })
        return false
      }
    },
    [
      tabs,
      editorDocuments,
      showToast,
      t.saveStatus.saved,
      t.mainWindow.editor.documentMissing,
      t.mainWindow.editor.saveFailed,
    ],
  )
  useEffect(() => {
    const handleOpenEditorTab = (event: Event) => {
      const detail = (event as CustomEvent<OpenEditorTabPayload>).detail
      if (!detail) {
        return
      }
      handleAddEditorTab(detail)
    }
    window.addEventListener("open-editor-tab", handleOpenEditorTab)
    return () => {
      window.removeEventListener("open-editor-tab", handleOpenEditorTab)
    }
  }, [handleAddEditorTab])
  const handleCloneTab = useCallback(
    (tabId: string) => {
      const sourceTab = tabs.find((t) => t.id === tabId)
      if (!sourceTab) return
      const sourceDocument = isEditorTab(sourceTab)
        ? editorDocuments[sourceTab.id]
        : null
      if (isEditorTab(sourceTab) && !sourceDocument) {
        showToast(t.mainWindow.editor.documentMissing, "error")
        return
      }

      const sourceIndex = tabs.findIndex((t) => t.id === tabId)
      const newTab: Tab = isTerminalTab(sourceTab)
        ? {
            id: generateId(),
            kind: "terminal",
            label: sourceTab.label,
            serverId: sourceTab.serverId,
            temporaryServer: sourceTab.temporaryServer,
          }
        : {
            id: generateId(),
            kind: "editor",
            label: sourceTab.label,
            serverId: sourceTab.serverId,
            sessionId: sourceTab.sessionId,
            remotePath: sourceTab.remotePath,
            localPath: sourceTab.localPath,
            dirty: sourceTab.dirty,
            language: sourceTab.language,
          }

      const newTabs = [...tabs]
      newTabs.splice(sourceIndex + 1, 0, newTab)
      setTabs(newTabs)
      if (sourceDocument) {
        setEditorDocuments((prev) => ({
          ...prev,
          [newTab.id]: {
            content: sourceDocument.content,
            savedContent: sourceDocument.savedContent,
            encoding: sourceDocument.encoding,
            isSaving: false,
          },
        }))
      }
      setSplitView(null)
      setPendingSplitLayout(null)
      setActiveTabId(newTab.id)
      triggerTerminalResize()
    },
    [
      tabs,
      editorDocuments,
      showToast,
      t.mainWindow.editor.documentMissing,
      triggerTerminalResize,
    ],
  )

  const handleCloseOthers = useCallback(
    (tabId: string) => {
      const targetTab = tabs.find((tab) => tab.id === tabId)
      if (!targetTab) {
        return
      }
      setTabs([targetTab])
      setSplitView(null)
      setPendingSplitLayout(null)
      setRememberedSplitViews({})
      setActiveTabId(tabId)
      triggerTerminalResize()
      setEditorDocuments((prev) => {
        if (!isEditorTab(targetTab)) {
          return {}
        }
        const targetDocument = prev[targetTab.id]
        if (!targetDocument) {
          return {}
        }
        return {
          [targetTab.id]: targetDocument,
        }
      })
      setTabSessions((prev) => {
        if (isEditorTab(targetTab)) {
          return {}
        }
        const currentSessionId = prev[targetTab.id]
        if (!currentSessionId) {
          return {}
        }
        return { [targetTab.id]: currentSessionId }
      })
      setRecordingTabs((prev) => {
        if (!isTerminalTab(targetTab) || !prev.has(targetTab.id)) {
          return new Set()
        }
        return new Set([targetTab.id])
      })
    },
    [tabs, triggerTerminalResize],
  )

  const pendingDirtyTab = pendingDirtyAction
    ? pendingDirtyAction.type === "switch"
      ? tabs.find((tab) => tab.id === pendingDirtyAction.sourceTabId) || null
      : tabs.find((tab) => tab.id === pendingDirtyAction.tabId) || null
    : null

  useEffect(() => {
    if (pendingDirtyAction && !pendingDirtyTab) {
      setPendingDirtyAction(null)
    }
  }, [pendingDirtyAction, pendingDirtyTab])

  const handleCancelPendingDirtyAction = useCallback(() => {
    setPendingDirtyAction(null)
  }, [])

  const handleDiscardPendingDirtyAction = useCallback(() => {
    if (!pendingDirtyAction) {
      return
    }
    if (pendingDirtyAction.type === "switch") {
      setEditorDocuments((prev) => {
        const sourceDoc = prev[pendingDirtyAction.sourceTabId]
        if (!sourceDoc) {
          return prev
        }
        return {
          ...prev,
          [pendingDirtyAction.sourceTabId]: {
            ...sourceDoc,
            content: sourceDoc.savedContent,
          },
        }
      })
      setTabs((prev) =>
        prev.map((tab) =>
          isEditorTab(tab) && tab.id === pendingDirtyAction.sourceTabId
            ? { ...tab, dirty: false }
            : tab,
        ),
      )
      selectTabImmediate(pendingDirtyAction.nextTabId)
    } else {
      closeTabImmediate(pendingDirtyAction.tabId)
    }
    setPendingDirtyAction(null)
  }, [pendingDirtyAction, selectTabImmediate, closeTabImmediate])

  const handleSavePendingDirtyAction = useCallback(async () => {
    if (!pendingDirtyAction) {
      return
    }
    const targetTabId =
      pendingDirtyAction.type === "switch"
        ? pendingDirtyAction.sourceTabId
        : pendingDirtyAction.tabId
    const saved = await handleSaveEditorTab(targetTabId)
    if (!saved) {
      return
    }
    if (pendingDirtyAction.type === "switch") {
      selectTabImmediate(pendingDirtyAction.nextTabId)
    } else {
      closeTabImmediate(pendingDirtyAction.tabId)
    }
    setPendingDirtyAction(null)
  }, [
    pendingDirtyAction,
    handleSaveEditorTab,
    selectTabImmediate,
    closeTabImmediate,
  ])

  const handleExportLogs = (tabId: string) => {
    const targetTab = tabs.find((tab) => tab.id === tabId)
    if (!targetTab || !isTerminalTab(targetTab)) {
      return
    }
    const event = new CustomEvent(`export-terminal-logs:${tabId}`)
    window.dispatchEvent(event)
  }

  const handleStartRecording = useCallback(
    async (tabId: string) => {
      // Check if the tab corresponds to an active session
      // Since session_id is currently not directly mapped in Tab interface (it might be managed inside TerminalTab),
      // we need to rely on the fact that for now, let's assume tabId is the sessionId or we can get it.
      // Wait, TerminalTab generates the session ID or receives it.
      // Looking at TerminalTab usage: <TerminalTab tabId={tab.id} ... />
      // And in connection.rs: connect_to_server returns a session_id.
      // MainWindow doesn't know the session_id directly.
      // However, the TerminalTab can listen for an event or expose a ref.
      // OR, we can assume tabId IS the sessionId if we structured it that way, but we generateId() for tabs.

      // We need a way to map tabId to sessionId.
      // Option: Dispatch an event to the specific TerminalTab to initiate the "Start Recording" process?
      // Or simpler: TerminalTab listens for a global event/context.

      // Let's dispatch a custom event that TerminalTab listens to.
      // When TerminalTab receives "start-recording:<tabId>", it calls the backend.
      // But MainWindow needs to know the file path first? No, TerminalTab can handle the UI flow too?
      // No, context menu is in MainWindow.

      // Better: MainWindow asks the user for the path, then tells TerminalTab "Start recording to <path>".
      // TerminalTab knows its session_id.

      const tab = tabs.find((t) => t.id === tabId)
      if (!tab || !isTerminalTab(tab)) {
        return
      }
      let defaultName = `recording-${tabId}.txt`
      if (config) {
        const server = config.servers.find((s) => s.id === tab.serverId)
        if (server) {
          defaultName = `recording-${server.host.replace(/[^a-z0-9]/gi, "_")}-${new Date().toISOString().replace(/[:.]/g, "-")}.txt`
        }
      }

      try {
        const path = await invoke<string | null>("select_save_path", {
          defaultName,
        })
        if (path) {
          // Dispatch event to TerminalTab
          window.dispatchEvent(
            new CustomEvent(`start-recording:${tabId}`, { detail: { path } }),
          )
          setRecordingTabs((prev) => new Set(prev).add(tabId))
        }
      } catch (error) {
        // Failed to select save path
      }
    },
    [tabs, config],
  )

  const handleStopRecording = (tabId: string) => {
    const targetTab = tabs.find((tab) => tab.id === tabId)
    if (!targetTab || !isTerminalTab(targetTab)) {
      return
    }
    window.dispatchEvent(new CustomEvent(`stop-recording:${tabId}`))
    setRecordingTabs((prev) => {
      const next = new Set(prev)
      next.delete(tabId)
      return next
    })
  }

  const handleReconnect = (tabId: string) => {
    const targetTab = tabs.find((tab) => tab.id === tabId)
    if (!targetTab || !isTerminalTab(targetTab)) {
      return
    }
    window.dispatchEvent(new CustomEvent(`reconnect:${tabId}`))
  }

  const handleContextMenu = (e: React.MouseEvent, tabId: string) => {
    e.preventDefault()
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      tabId,
    })
  }

  const handleServerContextMenu = (e: React.MouseEvent, serverId: string) => {
    e.preventDefault()
    setServerContextMenu({
      x: e.clientX,
      y: e.clientY,
      serverId,
    })
  }

  const handleConnectServer = (serverId: string) => {
    handleAddTab(serverId)
    setIsSettingsOpen(false)
    setEditServerId(null)
  }

  const handleOpenSettings = (
    tab: "servers" | "auth" | "proxies" | "snippets" | "general" = "servers",
  ) => {
    setSettingsInitialTab(tab)
    setIsSettingsOpen(true)
    if (tab !== "servers") {
      setEditServerId(null)
    }
  }

  useEffect(() => {
    let unlisten: (() => void) | undefined
    void listen("resh-open-settings", () => handleOpenSettings("general")).then(
      (cleanup) => {
        unlisten = cleanup
      },
    )
    return () => unlisten?.()
  }, [])

  const handleEditServerFromMenu = (serverId: string) => {
    setEditServerId(serverId)
    handleOpenSettings("servers")
    setServerContextMenu(null)
  }

  const handleToggleSnippetsLock = useCallback(async () => {
    if (!config) return
    const currentLocked = config.general.snippetsSidebarLocked
    const newConfig = {
      ...config,
      general: {
        ...config.general,
        snippetsSidebarLocked: !currentLocked,
      },
    }
    await saveConfig(newConfig)
  }, [config, saveConfig])

  const handleToggleAILock = useCallback(async () => {
    if (!config) return
    const currentLocked = config.general.aiSidebarLocked
    const newConfig = {
      ...config,
      general: {
        ...config.general,
        aiSidebarLocked: !currentLocked,
      },
    }
    await saveConfig(newConfig)
  }, [config, saveConfig])

  const handleToggleSFTPLock = useCallback(async () => {
    if (!config) return
    const currentLocked = config.general.sftpSidebarLocked
    const newConfig = {
      ...config,
      general: {
        ...config.general,
        sftpSidebarLocked: !currentLocked,
      },
    }
    await saveConfig(newConfig)
  }, [config, saveConfig])

  const prefetchSettings = () => {
    import("./settings/SettingsModal")
  }

  const prefetchSFTPSidebar = () => {
    void import("./SFTPSidebar")
  }

  const prefetchAISidebar = () => {
    void import("./AISidebar")
  }

  const prefetchSnippetsSidebar = () => {
    void import("./SnippetsSidebar")
  }

  const recentServers = config
    ? getRecentServers(
        config.general.recentServerIds,
        servers,
        config.general.maxRecentServers,
      )
    : []
  const tabWidthMode =
    config?.general.tabWidthMode === "adaptive" ? "adaptive" : "fixed"
  const tabFixedWidthRaw = config?.general.tabFixedWidth
  const tabFixedWidth =
    typeof tabFixedWidthRaw === "number" && Number.isFinite(tabFixedWidthRaw)
      ? Math.max(
          MIN_FIXED_TAB_WIDTH,
          Math.min(MAX_FIXED_TAB_WIDTH, tabFixedWidthRaw),
        )
      : DEFAULT_FIXED_TAB_WIDTH
  const fixedModeTotalWidth = tabs.length * tabFixedWidth + newTabButtonWidth
  const shouldFallbackToAdaptive =
    tabWidthMode === "fixed" &&
    tabListMaxWidth > 0 &&
    fixedModeTotalWidth > tabListMaxWidth
  const resolvedTabWidthMode = shouldFallbackToAdaptive
    ? "adaptive"
    : tabWidthMode

  useEffect(() => {
    const tabListElement = tabListRef.current
    if (!tabListElement) {
      return
    }

    const updateOverflowState = () => {
      const nextOverflowing =
        tabListElement.scrollWidth > tabListElement.clientWidth + 1
      setIsTabListOverflowing((prev) =>
        prev === nextOverflowing ? prev : nextOverflowing,
      )
    }

    updateOverflowState()
    const frameId = window.requestAnimationFrame(updateOverflowState)

    if (typeof ResizeObserver === "undefined") {
      return () => {
        window.cancelAnimationFrame(frameId)
      }
    }

    const resizeObserver = new ResizeObserver(() => {
      updateOverflowState()
    })
    resizeObserver.observe(tabListElement)

    return () => {
      window.cancelAnimationFrame(frameId)
      resizeObserver.disconnect()
    }
  }, [tabs, resolvedTabWidthMode, tabFixedWidth])

  const globalSnippets = config?.snippets || []
  const activeServer = activeServerId
    ? serverById.get(activeServerId) ||
      temporaryServerById.get(activeServerId) ||
      null
    : null
  const serverSnippets = activeServer?.snippets || []
  const displayedSnippets = [...globalSnippets, ...serverSnippets]
  const isSplitMode = splitView !== null
  const splitLayoutClassName = !splitView
    ? "flex-1 flex flex-col min-w-0 relative h-full"
    : splitView.layout === "horizontal"
      ? "flex-1 min-w-0 relative h-full grid grid-cols-2 gap-2 p-2"
      : splitView.layout === "vertical"
        ? "flex-1 min-w-0 relative h-full grid grid-rows-2 gap-2 p-2"
        : "flex-1 min-w-0 relative h-full grid grid-cols-2 grid-rows-2 gap-2 p-2"
  const pendingSplitRequiredCount = pendingSplitLayout
    ? SPLIT_LAYOUT_REQUIRED_TABS[pendingSplitLayout]
    : 0
  const initialSplitSelectedTabIds = useMemo(() => {
    if (!pendingSplitLayout) {
      return []
    }

    const requiredTabs = SPLIT_LAYOUT_REQUIRED_TABS[pendingSplitLayout]
    const candidateIds: string[] = []

    if (activeTabId) {
      candidateIds.push(activeTabId)
    }

    if (splitView?.layout === pendingSplitLayout) {
      splitView.tabIds.forEach((id) => {
        if (!candidateIds.includes(id)) {
          candidateIds.push(id)
        }
      })
    }

    tabs.forEach((tab) => {
      if (!candidateIds.includes(tab.id)) {
        candidateIds.push(tab.id)
      }
    })

    return candidateIds.slice(0, requiredTabs)
  }, [pendingSplitLayout, splitView, tabs, activeTabId])

  // Calculate z-index for sidebars based on lock state and open order
  // Rule: unlocked sidebars always appear above locked ones
  // If both unlocked, later-opened appears on top (rendering order determines this naturally)
  const aiZIndex = config?.general.aiSidebarLocked ? 10 : 50
  const snippetsZIndex = config?.general.snippetsSidebarLocked ? 10 : 50
  const sftpZIndex = config?.general.sftpSidebarLocked ? 10 : 50
  const contextMenuTab = contextMenu
    ? tabs.find((tab) => tab.id === contextMenu.tabId) || null
    : null

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden animate-[fadeIn_0.4s_ease-out]">
      <style>{`
        @keyframes fadeIn {
          from { opacity: 0; transform: translateY(10px); }
          to { opacity: 1; transform: translateY(0); }
        }
      `}</style>
      {/* Title Bar with drag region */}
      <div
        ref={titleBarRef}
        className="flex min-w-0 bg-[var(--bg-secondary)] h-10 border-b border-[var(--glass-border)] select-none relative shrink-0"
      >
        {/* Tab Bar */}
        <div
          ref={tabListRef}
          className={`flex flex-[0_1_auto] min-w-0 overflow-y-hidden p-0 gap-0 ${
            isTabListOverflowing
              ? "overflow-x-auto"
              : "overflow-x-hidden no-scrollbar"
          }`}
          role="tablist"
        >
          {tabs.map((tab, index) => {
            const visibleLabel = isEditorTab(tab)
              ? getEditorTabDisplayLabel(tab)
              : tab.label
            const renderedLabel =
              isEditorTab(tab) && tab.dirty ? `${visibleLabel} *` : visibleLabel
            return (
              <div
                key={tab.id}
                draggable
                onDragStart={() => handleTabDragStart(index)}
                onDragOver={(e) => handleTabDragOver(e, index)}
                onDrop={(e) => handleTabDrop(e, index)}
                onDragEnd={handleTabDragEnd}
                onKeyDown={(e) => handleTabKeyDown(e, index)}
                role="tab"
                tabIndex={activeTabId === tab.id ? 0 : -1}
                aria-selected={activeTabId === tab.id}
                aria-label={t.mainWindow.tabAriaLabel
                  .replace("{index}", (index + 1).toString())
                  .replace("{total}", tabs.length.toString())}
                className={`flex items-center gap-2 px-4 h-10 ${resolvedTabWidthMode === "adaptive" ? "w-auto min-w-[120px] max-w-[320px]" : "w-auto"} bg-transparent border-0 border-r border-r-[var(--glass-border)] rounded-none text-[var(--text-secondary)] cursor-pointer whitespace-nowrap transition-all relative overflow-hidden text-[13px] font-medium leading-snug shrink-0 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] ${activeTabId === tab.id ? '!bg-[var(--bg-primary)] !border-t-[3px] !border-t-[var(--accent-primary)] !text-[var(--text-primary)] after:content-[""] after:absolute after:-bottom-px after:left-0 after:right-0 after:h-px after:bg-[var(--bg-primary)] after:z-10' : ""} ${
                  draggedTabIndex === index ? "opacity-40 cursor-grabbing" : ""
                } ${dropTargetIndex === index ? "border-l-2 border-l-[var(--accent-primary)]" : ""}`}
                style={
                  resolvedTabWidthMode === "fixed"
                    ? { width: `${tabFixedWidth}px` }
                    : undefined
                }
                onClick={() => handleTabSelect(tab.id)}
                onContextMenu={(e) => handleContextMenu(e, tab.id)}
              >
                {isTerminalTab(tab) && recordingTabs.has(tab.id) && (
                  <Circle
                    size={8}
                    fill="#ef4444"
                    stroke="#ef4444"
                    className="mr-2 animate-pulse"
                  />
                )}
                <span
                  className="flex-1 overflow-hidden text-ellipsis whitespace-nowrap"
                  title={isEditorTab(tab) ? tab.remotePath : visibleLabel}
                >
                  <EmojiText text={renderedLabel} />
                </span>
                <button
                  type="button"
                  className="flex items-center justify-center w-[18px] h-[18px] bg-transparent border-none text-[var(--text-muted)] text-[14px] cursor-pointer rounded-[4px] transition-all hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)]"
                  onClick={(e) => {
                    e.stopPropagation()
                    handleCloseTab(tab.id)
                  }}
                  aria-label={t.mainWindow.closeTab}
                >
                  <X size={14} />
                </button>
              </div>
            )
          })}
          <div ref={newTabButtonRef} className="shrink-0" role="presentation">
            <NewTabButton
              servers={config?.servers || []}
              serverSort={config?.general.tabNewServerSort}
              recentServerIds={config?.general.recentServerIds}
              serverConnectionCounts={config?.general.serverConnectionCounts}
              onServerSelect={handleAddTab}
              onQuickConnect={handleAddQuickConnectTab}
              onOpenSettings={() => handleOpenSettings("servers")}
            />
          </div>
        </div>

        {/* Drag region spacer - empty area for dragging */}
        <div
          className="flex-1 min-w-[40px] basis-0"
          data-tauri-drag-region
        ></div>

        {/* Right side: Settings button + Window controls */}
        <div ref={rightControlsRef} className="flex items-center shrink-0">
          {showUpdateButton && (
            <button
              type="button"
              className={`flex items-center justify-center w-10 h-10 border-none cursor-pointer transition-all relative ${
                updateStatus === "error"
                  ? "bg-transparent text-red-400 hover:bg-[var(--bg-tertiary)]"
                  : updateStatus === "ready"
                    ? "bg-transparent text-emerald-400 hover:bg-[var(--bg-tertiary)]"
                    : "bg-transparent text-[var(--accent-primary)] hover:bg-[var(--bg-tertiary)]"
              }`}
              onMouseDown={(e) => {
                // Prevent title-bar drag from stealing the interaction.
                e.stopPropagation()
              }}
              onClick={() => {
                updateManagerApi.openDialog()
              }}
              aria-label={
                updateStatus === "downloading"
                  ? t.updateTitleDownloading
                  : updateStatus === "ready"
                    ? t.updateTitleReady
                    : updateStatus === "error"
                      ? t.updateTitleError
                      : t.updateTitleButton
              }
              title={
                updateStatus === "downloading"
                  ? `${t.updateTitleDownloading}${
                      updateProgress?.percent != null
                        ? ` ${updateProgress.percent}%`
                        : ""
                    }`
                  : updateStatus === "ready"
                    ? t.updateTitleReady
                    : updateStatus === "error"
                      ? t.updateTitleError
                      : t.updateTitleButton
              }
            >
              {updateStatus === "downloading" ? (
                <span className="relative flex items-center justify-center">
                  <Loader2 size={18} className="animate-spin" />
                  {updateProgress?.percent != null && (
                    <span className="absolute -bottom-1 text-[9px] font-mono leading-none">
                      {updateProgress.percent}
                    </span>
                  )}
                </span>
              ) : updateStatus === "error" ? (
                <AlertCircle size={18} />
              ) : updateStatus === "ready" ? (
                <ArrowDownCircle size={18} />
              ) : (
                <Download size={18} />
              )}
            </button>
          )}
          <button
            type="button"
            className={`flex items-center justify-center w-10 h-10 border-none text-[var(--text-secondary)] cursor-pointer transition-all ${isSFTPOpen && shouldRenderSFTPSidebar ? "bg-[var(--bg-tertiary)] text-[var(--accent-primary)]" : "bg-transparent hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"}`}
            onMouseDown={(e) => {
              e.stopPropagation()
              if (!shouldRenderSFTPSidebar) {
                return
              }
              if (!isSFTPOpen) {
                setHasLoadedSFTPSidebar(true)
                setIsSFTPOpen(true)
              } else if (config?.general.sftpSidebarLocked) {
                handleToggleSFTPLock()
              } else {
                setIsSFTPOpen(false)
              }
            }}
            onMouseEnter={prefetchSFTPSidebar}
            onFocus={prefetchSFTPSidebar}
            aria-label="SFTP"
            title="SFTP"
          >
            <Folder size={18} />
          </button>
          <button
            type="button"
            className={`flex items-center justify-center w-10 h-10 border-none text-[var(--text-secondary)] cursor-pointer transition-all ${isAIOpen ? "bg-[var(--bg-tertiary)] text-[var(--accent-primary)]" : "bg-transparent hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"}`}
            onMouseDown={(e) => {
              e.stopPropagation()
              if (!isAIOpen) {
                setHasLoadedAISidebar(true)
                setIsAIOpen(true)
              } else if (config?.general.aiSidebarLocked) {
                handleToggleAILock()
              } else {
                setIsAIOpen(false)
              }
            }}
            onMouseEnter={prefetchAISidebar}
            onFocus={prefetchAISidebar}
            aria-label={t.mainWindow.aiAssistant}
            title={t.mainWindow.aiAssistant}
          >
            <MessageSquare size={18} />
          </button>
          <button
            type="button"
            className={`flex items-center justify-center w-10 h-10 border-none text-[var(--text-secondary)] cursor-pointer transition-all ${isSnippetsOpen ? "bg-[var(--bg-tertiary)] text-[var(--accent-primary)]" : "bg-transparent hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"}`}
            onMouseDown={(e) => {
              e.stopPropagation()
              if (!isSnippetsOpen) {
                setHasLoadedSnippetsSidebar(true)
                setIsSnippetsOpen(true)
              } else if (config?.general.snippetsSidebarLocked) {
                handleToggleSnippetsLock()
              } else {
                setIsSnippetsOpen(false)
              }
            }}
            onMouseEnter={prefetchSnippetsSidebar}
            onFocus={prefetchSnippetsSidebar}
            aria-label={t.mainWindow.snippetsTooltip}
            title={t.mainWindow.snippetsTooltip}
          >
            <Code size={18} />
          </button>
          <SplitViewButton
            tabCount={tabs.length}
            isSplitActive={isSplitMode}
            onSelectLayout={handleStartSplitSelection}
            onExitSplit={handleExitSplitView}
          />
          <button
            type="button"
            className="flex items-center justify-center w-10 h-10 bg-transparent border-none text-[var(--text-secondary)] cursor-pointer transition-all hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
            onClick={() => handleOpenSettings("servers")}
            onMouseEnter={prefetchSettings}
            onFocus={prefetchSettings}
            aria-label={t.mainWindow.settings}
            title={t.mainWindow.settings}
          >
            <Settings size={18} />
          </button>
          {!isMacOS() && <WindowControls />}
        </div>
      </div>

      {/* Content Area */}
      <div
        className="flex-1 flex flex-col bg-[var(--bg-primary)] overflow-hidden relative min-h-0"
        style={{ position: "relative", display: "flex", flexDirection: "row" }}
      >
        <Suspense fallback={null}>
          {hasLoadedSFTPSidebar && (
            <SFTPSidebar
              isOpen={isSFTPOpen && shouldRenderSFTPSidebar}
              onClose={() => setIsSFTPOpen(false)}
              isLocked={config?.general.sftpSidebarLocked || false}
              onToggleLock={handleToggleSFTPLock}
              serverId={activeServerId}
              sessionId={activeSFTPSessionId}
              onShowToast={showToast}
              zIndex={sftpZIndex}
            />
          )}
        </Suspense>
        <div className={splitLayoutClassName}>
          {tabs.length === 0 ? (
            <WelcomeScreen
              servers={recentServers}
              onServerClick={handleAddTab}
              onOpenSettings={() => handleOpenSettings("servers")}
              onServerContextMenu={handleServerContextMenu}
            />
          ) : (
            tabs.map((tab) => {
              const server =
                serverById.get(tab.serverId) ||
                (isTerminalTab(tab) ? tab.temporaryServer : null) ||
                null
              if (!server) return null

              const editorDocument = isEditorTab(tab)
                ? editorDocuments[tab.id]
                : null
              const isVisibleInLayout = splitView
                ? splitView.tabIds.includes(tab.id)
                : activeTabId === tab.id
              const isFocusedTab = activeTabId === tab.id

              return (
                <div
                  key={tab.id}
                  className={
                    isSplitMode ? "min-w-0 min-h-0 flex" : "min-h-0 flex-1 flex"
                  }
                  style={{
                    display: isVisibleInLayout ? "flex" : "none",
                    minHeight: 0,
                    minWidth: 0,
                  }}
                >
                  <div
                    className={`w-full h-full min-w-0 min-h-0 overflow-hidden ${isSplitMode ? `rounded-[var(--radius-md)] border ${isFocusedTab ? "border-[var(--accent-primary)] ring-2 ring-[var(--accent-primary)]/40" : "border-[var(--glass-border)]"}` : ""}`}
                  >
                    {isTerminalTab(tab) ? (
                      <Suspense
                        fallback={
                          <div className="flex-1 bg-[var(--bg-primary)]" />
                        }
                      >
                        <TerminalTab
                          tabId={tab.id}
                          serverId={tab.serverId}
                          isActive={isFocusedTab}
                          isVisible={isVisibleInLayout}
                          onClose={handleCloseTab}
                          onActivate={() => handleTabSelect(tab.id)}
                          server={server}
                          servers={allServers}
                          authentications={authentications}
                          proxies={proxies}
                          terminalSettings={config?.general.terminal}
                          theme={config?.general.theme}
                          onSessionChange={(sessionId) =>
                            handleTabSessionChange(tab.id, sessionId)
                          }
                          onShowToast={showToast}
                        />
                      </Suspense>
                    ) : (
                      <Suspense
                        fallback={
                          <div className="flex-1 bg-[var(--bg-primary)]" />
                        }
                      >
                        {editorDocument ? (
                          <EditorTab
                            tabId={tab.id}
                            remotePath={tab.remotePath}
                            languageHint={tab.language}
                            content={editorDocument.content}
                            encoding={editorDocument.encoding}
                            dirty={tab.dirty}
                            terminalFontFamily={
                              config?.general.terminal.fontFamily ||
                              "Consolas, monospace"
                            }
                            terminalFontSize={
                              config?.general.terminal.fontSize || 13
                            }
                            appTheme={config?.general.theme || "dark"}
                            isSaving={editorDocument.isSaving}
                            onChange={(value) =>
                              handleEditorContentChange(tab.id, value)
                            }
                            onSave={() => handleSaveEditorTab(tab.id)}
                            onLanguageChange={(languageId) =>
                              handleEditorLanguageChange(tab.id, languageId)
                            }
                          />
                        ) : (
                          <div className="w-full h-full bg-[var(--bg-primary)] text-[var(--text-primary)] p-6">
                            <div className="max-w-[960px] mx-auto text-[13px] text-[var(--danger)]">
                              Editor document is missing for this tab.
                            </div>
                          </div>
                        )}
                      </Suspense>
                    )}
                  </div>
                </div>
              )
            })
          )}
        </div>
        <Suspense fallback={null}>
          {hasLoadedAISidebar && (
            <AISidebar
              isOpen={isAIOpen}
              onClose={() => setIsAIOpen(false)}
              isLocked={config?.general.aiSidebarLocked || false}
              onToggleLock={handleToggleAILock}
              onShowToast={showToast}
              currentServerId={activeServerId}
              currentTabId={activeAICurrentTabId}
              currentSshSessionId={activeAISshSessionId}
              editorContextByTabId={editorContextByTabId}
              zIndex={aiZIndex}
            />
          )}
        </Suspense>
        <Suspense fallback={null}>
          {hasLoadedSnippetsSidebar && (
            <SnippetsSidebar
              isOpen={isSnippetsOpen}
              onClose={() => setIsSnippetsOpen(false)}
              snippets={displayedSnippets}
              onOpenSettings={() => handleOpenSettings("snippets")}
              isLocked={config?.general.snippetsSidebarLocked || false}
              onToggleLock={handleToggleSnippetsLock}
              zIndex={snippetsZIndex}
            />
          )}
        </Suspense>
      </div>

      <SplitTabPickerModal
        isOpen={pendingSplitLayout !== null}
        layout={pendingSplitLayout}
        tabs={tabs.map((tab) => ({ id: tab.id, label: tab.label }))}
        requiredCount={pendingSplitRequiredCount}
        initialSelectedTabIds={initialSplitSelectedTabIds}
        onCancel={() => setPendingSplitLayout(null)}
        onConfirm={handleConfirmSplitSelection}
      />

      {/* Settings Modal */}
      <Suspense fallback={null}>
        {isSettingsOpen && (
          <SettingsModal
            isOpen={isSettingsOpen}
            onClose={() => {
              setIsSettingsOpen(false)
              setEditServerId(null)
            }}
            onConnectServer={handleConnectServer}
            initialTab={settingsInitialTab}
            editServerId={editServerId}
            settingsSaveApiRef={settingsSaveApiRef}
          />
        )}
      </Suspense>

      {pendingDirtyAction &&
        pendingDirtyTab &&
        isEditorTab(pendingDirtyTab) && (
          <div className="fixed inset-0 z-[1200] flex items-center justify-center bg-black/45 backdrop-blur-sm">
            <div className="w-[420px] max-w-[92vw] rounded-[var(--radius-md)] border border-[var(--glass-border)] bg-[var(--bg-secondary)] p-4 shadow-[0_20px_40px_rgba(0,0,0,0.35)]">
              <h2 className="text-[15px] font-semibold text-[var(--text-primary)]">
                {t.mainWindow.editorUnsavedTitle}
              </h2>
              <p className="mt-2 text-[13px] leading-relaxed text-[var(--text-secondary)]">
                {(pendingDirtyAction.type === "switch"
                  ? t.mainWindow.editorUnsavedSwitch
                  : t.mainWindow.editorUnsavedClose
                ).replace("{name}", pendingDirtyTab.label)}
              </p>
              <div className="mt-4 flex items-center justify-end gap-2">
                <button
                  type="button"
                  className="h-8 px-3 rounded border border-[var(--glass-border)] bg-transparent text-[12px] text-[var(--text-secondary)] hover:bg-[var(--bg-tertiary)]"
                  onClick={handleCancelPendingDirtyAction}
                >
                  {t.common.cancel}
                </button>
                <button
                  type="button"
                  className="h-8 px-3 rounded border border-[var(--glass-border)] bg-transparent text-[12px] text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)]"
                  onClick={handleDiscardPendingDirtyAction}
                >
                  {t.mainWindow.discardChanges}
                </button>
                <button
                  type="button"
                  className="h-8 px-3 rounded border border-[var(--accent-primary)] bg-[var(--accent-primary)]/15 text-[12px] text-[var(--text-primary)] hover:bg-[var(--accent-primary)]/25"
                  onClick={() => {
                    void handleSavePendingDirtyAction()
                  }}
                >
                  {t.common.save}
                </button>
              </div>
            </div>
          </div>
        )}

      {/* Tab Context Menu */}
      {contextMenu && contextMenuTab && (
        <TabContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          tabId={contextMenu.tabId}
          tabType={contextMenuTab.kind}
          isRecording={
            isTerminalTab(contextMenuTab) &&
            recordingTabs.has(contextMenuTab.id)
          }
          onClose={() => setContextMenu(null)}
          onClone={handleCloneTab}
          onReconnect={handleReconnect}
          onExport={handleExportLogs}
          onStartRecording={handleStartRecording}
          onStopRecording={handleStopRecording}
          onCloseTab={handleCloseTab}
          onCloseOthers={handleCloseOthers}
        />
      )}

      {serverContextMenu && (
        <ServerContextMenu
          x={serverContextMenu.x}
          y={serverContextMenu.y}
          serverId={serverContextMenu.serverId}
          onClose={() => setServerContextMenu(null)}
          onEdit={handleEditServerFromMenu}
          onConnect={(serverId) => handleAddTab(serverId)}
        />
      )}

      {/* Toast Notifications */}
      <ToastContainer toasts={toasts} onRemove={removeToast} />

      <UpdateDialog
        isOpen={updateDialogOpen}
        onClose={() => updateManagerApi.closeDialog()}
      />
    </div>
  )
}
