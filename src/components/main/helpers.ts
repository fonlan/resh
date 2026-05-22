import type { Config } from "../../types"
import type { SplitLayout } from "../SplitViewButton"

export const EMPTY_SERVERS: Config["servers"] = []
export const EMPTY_AUTHENTICATIONS: Config["authentications"] = []
export const EMPTY_PROXIES: Config["proxies"] = []

export const SPLIT_LAYOUT_REQUIRED_TABS: Record<SplitLayout, number> = {
  horizontal: 2,
  vertical: 2,
  grid: 4,
}

export const MIN_FIXED_TAB_WIDTH = 120
export const MAX_FIXED_TAB_WIDTH = 400
export const DEFAULT_FIXED_TAB_WIDTH = 200
export const MIN_TITLEBAR_DRAG_SPACER_WIDTH = 40

export const getFileNameFromPath = (path: string): string => {
  const normalized = path.replace(/\\/g, "/")
  const segments = normalized.split("/")
  const fileName = segments[segments.length - 1]?.trim()
  return fileName || normalized
}
