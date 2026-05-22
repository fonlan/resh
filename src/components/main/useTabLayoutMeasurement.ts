import { useEffect, useRef, useState } from "react"
import { MIN_TITLEBAR_DRAG_SPACER_WIDTH } from "./helpers"

/**
 * 集中管理 MainWindow 顶部 Tab 栏的尺寸测量：
 * - tabListMaxWidth：tab 列表容器允许的最大宽度（用于决定是否回退到 adaptive 模式）
 * - newTabButtonWidth：右侧新建按钮的实际宽度（用于布局保留空间）
 *
 * 通过 ResizeObserver + window resize 事件保持实时更新；当 ResizeObserver
 * 不可用时降级为一次性测量。
 *
 * 不包含 tab 列表横向溢出检测——因为后者依赖于 resolvedTabWidthMode，
 * 而 resolvedTabWidthMode 又反过来依赖 tabListMaxWidth；保留在调用方
 * 内部声明可避免循环依赖。
 */
export const useTabLayoutMeasurement = (tabsLength: number) => {
  const titleBarRef = useRef<HTMLDivElement | null>(null)
  const tabListRef = useRef<HTMLDivElement | null>(null)
  const rightControlsRef = useRef<HTMLDivElement | null>(null)
  const newTabButtonRef = useRef<HTMLDivElement | null>(null)
  const [tabListMaxWidth, setTabListMaxWidth] = useState(0)
  const [newTabButtonWidth, setNewTabButtonWidth] = useState(0)

  useEffect(() => {
    const titleBarElement = titleBarRef.current
    if (!titleBarElement) {
      return
    }

    const updateLayoutSizes = () => {
      const nextTitleBarWidth = titleBarElement.clientWidth
      const nextRightControlsWidth = rightControlsRef.current?.offsetWidth ?? 0
      const nextTabListMaxWidth = Math.max(
        0,
        nextTitleBarWidth -
          nextRightControlsWidth -
          MIN_TITLEBAR_DRAG_SPACER_WIDTH,
      )
      const nextNewTabButtonWidth = newTabButtonRef.current?.offsetWidth ?? 0
      setTabListMaxWidth((prev) =>
        prev === nextTabListMaxWidth ? prev : nextTabListMaxWidth,
      )
      setNewTabButtonWidth((prev) =>
        prev === nextNewTabButtonWidth ? prev : nextNewTabButtonWidth,
      )
    }

    updateLayoutSizes()

    if (typeof ResizeObserver === "undefined") {
      return
    }

    const resizeObserver = new ResizeObserver(updateLayoutSizes)
    resizeObserver.observe(titleBarElement)
    if (rightControlsRef.current)
      resizeObserver.observe(rightControlsRef.current)
    if (newTabButtonRef.current) resizeObserver.observe(newTabButtonRef.current)

    window.addEventListener("resize", updateLayoutSizes)

    return () => {
      window.removeEventListener("resize", updateLayoutSizes)
      resizeObserver.disconnect()
    }
  }, [tabsLength])

  return {
    titleBarRef,
    tabListRef,
    rightControlsRef,
    newTabButtonRef,
    tabListMaxWidth,
    newTabButtonWidth,
  }
}
