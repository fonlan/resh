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

/**
 * macOS Overlay 标题栏中原生交通灯占用的左侧安全区宽度（逻辑像素）。
 * 与 `tauri.macos.conf.json` 的 `trafficLightPosition`（x≈12）及三枚按钮间距对齐；
 * 最终可在真机上微调，但布局测量须与此视觉宽度同源（经 leftInset ref 实测）。
 */
export const MACOS_TRAFFIC_LIGHT_INSET_WIDTH = 78

export const getFileNameFromPath = (path: string): string => {
  const normalized = path.replace(/\\/g, "/")
  const segments = normalized.split("/")
  const fileName = segments[segments.length - 1]?.trim()
  return fileName || normalized
}
