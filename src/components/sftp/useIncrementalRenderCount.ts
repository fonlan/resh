import { useEffect, useState } from "react"
import { TREE_RENDER_BATCH_SIZE } from "./helpers"

/**
 * 树节点逐帧增量渲染：超过 TREE_RENDER_BATCH_SIZE 的节点列表分批显示，
 * 避免一次性渲染大目录导致主线程卡死。
 */
export const useIncrementalRenderCount = (
  total: number,
  enabled: boolean,
): number => {
  const [count, setCount] = useState(0)

  useEffect(() => {
    if (!enabled || total <= 0) {
      setCount(0)
      return
    }

    if (total <= TREE_RENDER_BATCH_SIZE) {
      setCount(total)
      return
    }

    let cancelled = false
    let frameId: number | null = null

    setCount(TREE_RENDER_BATCH_SIZE)

    const pump = () => {
      frameId = window.requestAnimationFrame(() => {
        if (cancelled) {
          return
        }

        setCount((prev) => {
          if (prev >= total) {
            return prev
          }

          const next = Math.min(prev + TREE_RENDER_BATCH_SIZE, total)
          if (next < total) {
            pump()
          }
          return next
        })
      })
    }

    pump()

    return () => {
      cancelled = true
      if (frameId !== null) {
        window.cancelAnimationFrame(frameId)
      }
    }
  }, [enabled, total])

  return Math.min(count, total)
}
