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
 *
 * Phase 3 定稿（与 `src-tauri/tauri.macos.conf.json` 同步锁定）：
 * - `trafficLightPosition`: `{ x: 12, y: 14 }`（40px / `h-10` 行内垂直居中）
 * - 左 inset: `78`（x=12 + 三枚系统按钮与间距 + 与首个标签的呼吸间距）
 * - 仅 `isMacOS()` 渲染；Windows 无此占位，仍用自绘 `WindowControls`
 * - 布局测量经 `leftInsetRef` 读取实测宽度，与此常量同源
 */
export const MACOS_TRAFFIC_LIGHT_INSET_WIDTH = 78

export const getFileNameFromPath = (path: string): string => {
  const normalized = path.replace(/\\/g, "/")
  const segments = normalized.split("/")
  const fileName = segments[segments.length - 1]?.trim()
  return fileName || normalized
}
