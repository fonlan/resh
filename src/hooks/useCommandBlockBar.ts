import { useEffect, useState } from "react"
import { listen } from "@tauri-apps/api/event"
import type { IDisposable, IMarker, Terminal } from "xterm"

interface CommandBlock {
  id: number
  color: string
  start: IMarker
  end: IMarker | null
}

export interface CommandBlockRect {
  id: number
  y: number
  height: number
  color: string
}

interface CommandBlockBarState {
  blockRects: CommandBlockRect[]
  isAlternateBuffer: boolean
}

const EMPTY_STATE: CommandBlockBarState = {
  blockRects: [],
  isAlternateBuffer: false,
}

const colorForIndex = (index: number) => {
  const hue = (index * 137.508) % 360
  return `hsl(${hue.toFixed(1)}, 65%, 58%)`
}

const sameRects = (a: CommandBlockRect[], b: CommandBlockRect[]) => {
  if (a.length !== b.length) return false
  return a.every((rect, index) => {
    const other = b[index]
    return (
      rect.id === other.id &&
      rect.y === other.y &&
      rect.height === other.height &&
      rect.color === other.color
    )
  })
}

const sameState = (a: CommandBlockBarState, b: CommandBlockBarState) =>
  a.isAlternateBuffer === b.isAlternateBuffer &&
  sameRects(a.blockRects, b.blockRects)

export const useCommandBlockBar = (
  containerId: string,
  sessionId: string | null,
  terminal: Terminal | null,
  isReady: boolean,
  enabled: boolean,
): CommandBlockBarState => {
  const [state, setState] = useState<CommandBlockBarState>(EMPTY_STATE)

  useEffect(() => {
    if (!enabled || !terminal || !isReady) {
      setState(EMPTY_STATE)
      return
    }

    const container = document.getElementById(containerId)
    if (!container) {
      setState(EMPTY_STATE)
      return
    }

    const blocks: CommandBlock[] = []
    const disposables: IDisposable[] = []
    let nextId = 1
    let frameId: number | null = null
    let disposed = false
    let commandBlockUnlisten: (() => void) | null = null

    const commitState = (next: CommandBlockBarState) => {
      setState((current) => (sameState(current, next) ? current : next))
    }

    const computeRects = (): CommandBlockBarState => {
      const buffer = terminal.buffer.active
      const isAlternateBuffer = buffer.type === "alternate"
      if (isAlternateBuffer) {
        return { blockRects: [], isAlternateBuffer: true }
      }

      const screenElement = container.querySelector(
        ".xterm-screen",
      ) as HTMLElement | null
      if (!screenElement || terminal.rows <= 0) {
        return EMPTY_STATE
      }

      const containerRect = container.getBoundingClientRect()
      const screenRect = screenElement.getBoundingClientRect()
      const rowHeight = screenRect.height / terminal.rows
      if (rowHeight <= 0) {
        return EMPTY_STATE
      }

      const rowOffsetY = screenRect.top - containerRect.top
      const viewportY = buffer.viewportY
      const viewportBottom = viewportY + terminal.rows - 1
      const cursorAbs = buffer.baseY + buffer.cursorY

      const blockRects: CommandBlockRect[] = []
      for (const block of blocks) {
        if (block.start.isDisposed) continue

        const startLine = block.start.line
        const endLine =
          block.end && !block.end.isDisposed ? block.end.line : cursorAbs
        const topLine = Math.max(startLine, viewportY)
        const bottomLine = Math.min(endLine, viewportBottom)
        if (topLine > bottomLine) continue

        blockRects.push({
          id: block.id,
          y: rowOffsetY + (topLine - viewportY) * rowHeight,
          height: (bottomLine - topLine + 1) * rowHeight,
          color: block.color,
        })
      }

      return { blockRects, isAlternateBuffer: false }
    }

    const updateRects = () => {
      frameId = null
      if (disposed) return
      commitState(computeRects())
    }

    const scheduleUpdate = () => {
      if (frameId !== null) return
      frameId = window.requestAnimationFrame(updateRects)
    }

    const closeCurrent = () => {
      const current = blocks[blocks.length - 1]
      if (!current || current.end !== null) return
      current.end = terminal.registerMarker(-1) ?? terminal.registerMarker(0)
    }

    const openNew = () => {
      const start = terminal.registerMarker(0)
      if (!start) return

      const block: CommandBlock = {
        id: nextId,
        color: colorForIndex(nextId),
        start,
        end: null,
      }
      nextId += 1

      start.onDispose(() => {
        const index = blocks.indexOf(block)
        if (index >= 0) {
          blocks.splice(index, 1)
          scheduleUpdate()
        }
      })

      blocks.push(block)
    }

    disposables.push(
      terminal.onData((data) => {
        if (terminal.buffer.active.type === "alternate") return

        let changed = false
        for (const char of data) {
          if (char === "\r") {
            closeCurrent()
            openNew()
            changed = true
          }
        }

        if (changed) scheduleUpdate()
      }),
    )

    disposables.push(
      terminal.buffer.onBufferChange((buffer) => {
        if (buffer.type === "alternate") closeCurrent()
        scheduleUpdate()
      }),
    )

    disposables.push(terminal.onScroll(() => scheduleUpdate()))
    disposables.push(terminal.onRender(() => scheduleUpdate()))

    if (sessionId) {
      void listen<string>(`terminal-command-block:${sessionId}`, (event) => {
        if (terminal.buffer.active.type === "alternate") return

        if (event.payload === "start") {
          closeCurrent()
          openNew()
          scheduleUpdate()
        } else if (event.payload === "end") {
          closeCurrent()
          scheduleUpdate()
        }
      }).then((unlisten) => {
        if (disposed) {
          unlisten()
        } else {
          commandBlockUnlisten = unlisten
        }
      })
    }

    scheduleUpdate()

    return () => {
      disposed = true
      if (frameId !== null) {
        window.cancelAnimationFrame(frameId)
        frameId = null
      }
      for (const disposable of disposables) {
        disposable.dispose()
      }
      commandBlockUnlisten?.()

      const snapshot = blocks.slice()
      blocks.length = 0
      for (const block of snapshot) {
        block.start.dispose()
        block.end?.dispose()
      }
      setState(EMPTY_STATE)
    }
  }, [containerId, enabled, isReady, sessionId, terminal])

  return state
}
