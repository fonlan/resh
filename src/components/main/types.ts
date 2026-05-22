import type { Config } from "../../types"
import type { SplitLayout } from "../SplitViewButton"

export type TabKind = "terminal" | "editor"

export interface BaseTab {
  id: string
  label: string
  serverId: string
  kind: TabKind
}

export interface TerminalTabState extends BaseTab {
  kind: "terminal"
  temporaryServer?: Config["servers"][number]
}

export interface EditorTabState extends BaseTab {
  kind: "editor"
  sessionId: string
  remotePath: string
  localPath: string
  dirty: boolean
  language: string
}

export type Tab = TerminalTabState | EditorTabState

export const isTerminalTab = (tab: Tab): tab is TerminalTabState =>
  tab.kind === "terminal"

export const isEditorTab = (tab: Tab): tab is EditorTabState =>
  tab.kind === "editor"

export interface OpenEditorTabPayload {
  serverId: string
  sessionId: string
  remotePath: string
  localPath: string
  content: string
  encoding: string
  language: string
  dirty?: boolean
  label?: string
}

export interface EditorDocumentState {
  content: string
  savedContent: string
  encoding: string
  isSaving: boolean
}

export type PendingDirtyEditorAction =
  | {
      type: "switch"
      sourceTabId: string
      nextTabId: string
    }
  | {
      type: "close"
      tabId: string
    }

export interface SplitViewState {
  layout: SplitLayout
  tabIds: string[]
}
