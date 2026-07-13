import { useEffect, useRef, useCallback, useMemo, useState } from "react"
import { Terminal, type IDisposable, type ITheme } from "xterm"
import { FitAddon } from "xterm-addon-fit"
import { WebglAddon } from "xterm-addon-webgl"
import "xterm/css/xterm.css"
import { TerminalSettings, TerminalRightClickMode } from "../types"
import { debounce } from "../utils/common"
import { invoke } from "@tauri-apps/api/core"
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager"
import { isMacOS } from "../utils/platform"
import {
  assertMacOsImeShiftSymbolSelfCheck,
  resolveMacOsImeDroppedShiftSymbol,
} from "../utils/macOsImeShiftSymbol"

// ponytail: tiny self-check at load; fails if Shift+IME mapping/guards regress.
assertMacOsImeShiftSymbolSelfCheck()

type ResolvedTerminalTheme = "light" | "dark" | "orange" | "green"

const resolveTerminalTheme = (
  theme?: "light" | "dark" | "orange" | "green" | "system",
): ResolvedTerminalTheme => {
  if (theme === "system") {
    return window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light"
  }
  return theme || "dark"
}

const getTerminalPalette = (theme: ResolvedTerminalTheme): ITheme => {
  switch (theme) {
    case "light":
      return {
        background: "#ffffff",
        foreground: "#1a202c",
        cursor: "#1a202c",
        cursorAccent: "#ffffff",
        selectionBackground: "rgba(0, 245, 255, 0.3)",
      }
    case "orange":
      return {
        background: "#1c1917",
        foreground: "#fafaf9",
        cursor: "#f97316",
        cursorAccent: "#1c1917",
        selectionBackground: "rgba(249, 115, 22, 0.3)",
      }
    case "green":
      return {
        background: "#0a0f0d",
        foreground: "#f0fdf4",
        cursor: "#86efac",
        cursorAccent: "#0a0f0d",
        selectionBackground: "rgba(134, 239, 172, 0.3)",
      }
    default:
      return {
        background: "#000000",
        foreground: "#ffffff",
        cursor: "#00f5ff",
        cursorAccent: "#000000",
        selectionBackground: "rgba(0, 245, 255, 0.3)",
      }
  }
}

export const useTerminal = (
  containerId: string,
  sessionIdRef: React.RefObject<string | null>,
  settings?: TerminalSettings,
  theme?: "light" | "dark" | "orange" | "green" | "system",
  terminalRightClickMode: TerminalRightClickMode = "contextMenu",
  onData?: (data: string) => void,
  onResize?: (cols: number, rows: number) => void,
) => {
  const terminalRef = useRef<Terminal | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)
  const webglAddonRef = useRef<WebglAddon | null>(null)
  const webglContextLossDisposableRef = useRef<IDisposable | null>(null)
  const [isReady, setIsReady] = useState(false)

  const onDataRef = useRef(onData)
  const onResizeRef = useRef(onResize)
  const terminalRightClickModeRef = useRef(terminalRightClickMode)

  useEffect(() => {
    onDataRef.current = onData
  }, [onData])

  useEffect(() => {
    onResizeRef.current = onResize
  }, [onResize])

  useEffect(() => {
    terminalRightClickModeRef.current = terminalRightClickMode
  }, [terminalRightClickMode])

  useEffect(() => {
    const container = document.getElementById(containerId)
    if (!container) return

    const actualTheme = resolveTerminalTheme(theme)

    const term = new Terminal({
      cursorBlink: true,
      fontSize: settings?.fontSize || 14,
      fontFamily:
        settings?.fontFamily ||
        "'Maple Mono NF CN', 'JetBrains Mono', 'Consolas', monospace",
      cursorStyle:
        (settings?.cursorStyle as "block" | "underline" | "bar") || "block",
      scrollback: settings?.scrollback || 25000,
      theme: getTerminalPalette(actualTheme),
    })

    // TEMP (phase-1 IME debug): enable with localStorage.setItem('resh.debugImeKeys','1')
    // Remove in phase-3 after macOS IME Shift-symbol fix lands.
    const debugImeKeys =
      typeof localStorage !== "undefined" &&
      localStorage.getItem("resh.debugImeKeys") === "1"

    const logImeDebug = (label: string, payload: Record<string, unknown>) => {
      if (!debugImeKeys) return
      console.log(`[resh-ime-debug] ${label}`, payload)
    }

    // macOS Chinese IME: track real composition so we never inject during 组字.
    // Also short-window dedupe if xterm later delivers the same char via textarea.
    let imeComposing = false
    let recentImeInject: { ch: string; at: number } | null = null
    const IME_INJECT_DEDUPE_MS = 40
    let imeCompositionCleanup: (() => void) | null = null

    const markImeInjected = (ch: string) => {
      recentImeInject = { ch, at: performance.now() }
    }

    // Dedupe only the next matching onData. Any non-match (or expiry) clears the
    // marker so a later legitimate same char is never swallowed (review P2).
    const shouldDropDuplicateOnData = (data: string): boolean => {
      if (!recentImeInject) return false
      if (performance.now() - recentImeInject.at > IME_INJECT_DEDUPE_MS) {
        recentImeInject = null
        return false
      }
      if (data !== recentImeInject.ch) {
        recentImeInject = null
        return false
      }
      recentImeInject = null
      return true
    }

    term.attachCustomKeyEventHandler((event) => {
      if (debugImeKeys) {
        logImeDebug("customKey", {
          type: event.type,
          key: event.key,
          code: event.code,
          keyCode: event.keyCode,
          shiftKey: event.shiftKey,
          metaKey: event.metaKey,
          ctrlKey: event.ctrlKey,
          altKey: event.altKey,
          isComposing: event.isComposing,
          imeComposing,
          path:
            event.keyCode === 229
              ? "likely-xterm-composition-helper-229"
              : "normal-key-path-if-not-cancelled",
        })
      }

      // Scheme A: macOS IME keyCode 229 + Shift + mappable code → inject symbol.
      // xterm CompositionHelper would otherwise diff an unchanged textarea and drop.
      if (isMacOS() && event.type === "keydown") {
        const dropped = resolveMacOsImeDroppedShiftSymbol(event, {
          imeComposing,
        })
        if (dropped) {
          markImeInjected(dropped)
          // Direct onData: avoid term.paste (would re-enter onData and self-dedupe).
          // return false skips xterm 229 CompositionHelper; short-window dedupe
          // still drops a later textarea/input path if the IME also commits the char.
          onDataRef.current?.(dropped)
          logImeDebug("ime-shift-inject", {
            code: event.code,
            ch: dropped,
            keyCode: event.keyCode,
          })
          return false
        }
      }

      // Only intercept macOS Cmd+C/V/A. Non-meta keys fall through (except inject above).
      if (!isMacOS() || !event.metaKey || event.ctrlKey || event.altKey) {
        // Control+C must continue through xterm so the shell receives ETX.
        return true
      }

      switch (event.key.toLowerCase()) {
        case "c": {
          const selection = term.getSelection()
          if (selection) void writeText(selection)
          return false
        }
        case "v":
          void readText().then((text) => {
            if (text) term.paste(text)
          })
          return false
        case "a":
          term.selectAll()
          return false
        default:
          return true
      }
    })

    const fitAddon = new FitAddon()
    term.loadAddon(fitAddon)

    // Register onData inside the hook to ensure it's always attached to the current term
    const disposable = term.onData((data) => {
      // Drop duplicate if xterm also emits the same char we just injected.
      if (shouldDropDuplicateOnData(data)) {
        logImeDebug("onData-deduped", { data })
        return
      }
      // Keep debug-off hot path allocation-free (SSH output can be high volume).
      if (debugImeKeys) {
        logImeDebug("onData", {
          data,
          length: data.length,
          codes: [...data].map((c) => c.charCodeAt(0)),
        })
      }
      onDataRef.current?.(data)
    })

    const selectionDisposable = term.onSelectionChange(
      debounce(() => {
        if (term.hasSelection()) {
          const selection = term.getSelection()
          if (selection) {
            if (terminalRightClickModeRef.current === "selectionCopyPaste") {
              writeText(selection).catch(() => {
                // Failed to auto-copy selection
              })
            }
            // Sync selection to backend for AI tools
            const currentSessionId = sessionIdRef.current
            if (currentSessionId) {
              invoke("update_terminal_selection", {
                sessionId: currentSessionId, // Tauri converts camelCase to snake_case
                selection,
              }).catch((err) => {
                console.error("Failed to update backend selection:", err)
              })
            }
          }
        }
      }, 500),
    )

    // Handle OSC 52 (Clipboard)
    const oscDisposable = term.parser.registerOscHandler(52, (data) => {
      try {
        const parts = data.split(";")
        if (parts.length < 2) return false

        const b64Data = parts.slice(1).join(";")
        if (b64Data === "?") return true

        const text = new TextDecoder().decode(
          Uint8Array.from(atob(b64Data), (c) => c.charCodeAt(0)),
        )

        writeText(text).catch(() => {
          // OSC 52 write failed
        })

        return true
      } catch (e) {
        return false
      }
    })

    const refreshTerminal = () => {
      if (
        !term.element ||
        container.clientWidth <= 0 ||
        container.clientHeight <= 0
      ) {
        return
      }

      try {
        fitAddon.fit()
        if (term.rows > 0) {
          term.refresh(0, term.rows - 1)
        }
        onResizeRef.current?.(term.cols, term.rows)
      } catch (e) {
        // Fit/refresh can fail while the terminal is hidden.
      }
    }

    const disposeWebglAddon = (refresh = true) => {
      webglContextLossDisposableRef.current?.dispose()
      webglContextLossDisposableRef.current = null

      const webglAddon = webglAddonRef.current
      if (!webglAddon) return

      webglAddonRef.current = null
      try {
        webglAddon.dispose()
      } catch (e) {
        // WebGL renderer was already gone.
      }

      if (refresh) {
        refreshTerminal()
      }
    }

    const loadWebglAddon = () => {
      if (webglAddonRef.current) return

      let webglAddon: WebglAddon | null = null
      try {
        webglAddon = new WebglAddon()
        webglContextLossDisposableRef.current = webglAddon.onContextLoss(() => {
          // ponytail: fall back to xterm's default renderer after GPU loss; rebuild WebGL later only if perf proves it matters.
          disposeWebglAddon()
        })
        term.loadAddon(webglAddon)
        webglAddonRef.current = webglAddon
      } catch (e) {
        webglContextLossDisposableRef.current?.dispose()
        webglContextLossDisposableRef.current = null
        webglAddon?.dispose()
        // WebGL renderer could not be loaded.
      }
    }

    let imeDebugCleanup: (() => void) | null = null

    const attachImeCompositionTracking = () => {
      if (imeCompositionCleanup || !term.element || !isMacOS()) return
      const textarea = term.element.querySelector(
        "textarea.xterm-helper-textarea",
      ) as HTMLTextAreaElement | null
      if (!textarea) return

      const onCompositionStart = () => {
        imeComposing = true
        logImeDebug("composition-tracking", { active: true })
      }
      const onCompositionEnd = () => {
        imeComposing = false
        logImeDebug("composition-tracking", { active: false })
      }

      textarea.addEventListener("compositionstart", onCompositionStart)
      textarea.addEventListener("compositionend", onCompositionEnd)

      imeCompositionCleanup = () => {
        textarea.removeEventListener("compositionstart", onCompositionStart)
        textarea.removeEventListener("compositionend", onCompositionEnd)
        imeComposing = false
      }
    }

    const attachImeDebugListeners = () => {
      if (!debugImeKeys || imeDebugCleanup || !term.element) return
      const textarea = term.element.querySelector(
        "textarea.xterm-helper-textarea",
      ) as HTMLTextAreaElement | null
      if (!textarea) return

      const onKey = (ev: KeyboardEvent) => {
        const valueAtEvent = textarea.value
        logImeDebug(`textarea.${ev.type}`, {
          key: ev.key,
          code: ev.code,
          keyCode: ev.keyCode,
          shiftKey: ev.shiftKey,
          metaKey: ev.metaKey,
          ctrlKey: ev.ctrlKey,
          altKey: ev.altKey,
          isComposing: ev.isComposing,
          imeComposing,
          defaultPrevented: ev.defaultPrevented,
          textareaValue: valueAtEvent,
          textareaValueLen: valueAtEvent.length,
        })

        // Approximate CompositionHelper._handleAnyTextareaChanges (setTimeout 0):
        // capture old/new/diff after the same microtask delay xterm uses.
        if (ev.type === "keydown" && ev.keyCode === 229 && !ev.isComposing) {
          const oldValue = valueAtEvent
          window.setTimeout(() => {
            const newValue = textarea.value
            const grew = newValue.length > oldValue.length
            const diff = grew
              ? newValue.slice(oldValue.length) ||
                newValue.replace(oldValue, "")
              : ""
            logImeDebug("textarea.229-post-timeout-snapshot", {
              key: ev.key,
              code: ev.code,
              shiftKey: ev.shiftKey,
              isComposing: ev.isComposing,
              oldValue,
              newValue,
              oldLen: oldValue.length,
              newLen: newValue.length,
              grew,
              diff,
              unchanged: oldValue === newValue,
              inferenceNote:
                "Mirrors xterm CompositionHelper textarea-diff path; if unchanged and no onData, path A drop is supported.",
            })
          }, 0)
        }
      }

      const onInput = (ev: Event) => {
        const ie = ev as InputEvent
        logImeDebug(`textarea.${ev.type}`, {
          inputType: ie.inputType,
          data: ie.data,
          composed: ie.composed,
          isComposing: ie.isComposing,
          textareaValue: textarea.value,
          textareaValueLen: textarea.value.length,
        })
      }

      const onComposition = (ev: CompositionEvent) => {
        logImeDebug(`textarea.${ev.type}`, {
          data: ev.data,
          textareaValue: textarea.value,
          textareaValueLen: textarea.value.length,
        })
      }

      const types = [
        "keydown",
        "keypress",
        "keyup",
        "input",
        "compositionstart",
        "compositionupdate",
        "compositionend",
        "beforeinput",
      ] as const

      for (const t of types) {
        if (t === "input" || t === "beforeinput") {
          textarea.addEventListener(t, onInput)
        } else if (t.startsWith("composition")) {
          textarea.addEventListener(t, onComposition as EventListener)
        } else {
          textarea.addEventListener(t, onKey as EventListener)
        }
      }

      imeDebugCleanup = () => {
        for (const t of types) {
          if (t === "input" || t === "beforeinput") {
            textarea.removeEventListener(t, onInput)
          } else if (t.startsWith("composition")) {
            textarea.removeEventListener(t, onComposition as EventListener)
          } else {
            textarea.removeEventListener(t, onKey as EventListener)
          }
        }
      }
      logImeDebug("listeners-attached", { containerId })
    }

    const initTerminal = () => {
      if (
        !term.element &&
        container.clientWidth > 0 &&
        container.clientHeight > 0
      ) {
        term.open(container)
        refreshTerminal()
        loadWebglAddon()
        attachImeCompositionTracking()
        attachImeDebugListeners()
      }
    }

    // Initial check
    initTerminal()

    const handleResize = debounce(() => {
      if (container.clientWidth > 0 && container.clientHeight > 0) {
        if (!term.element) {
          initTerminal()
        }
        refreshTerminal()
      }
    }, 50)

    const resizeObserver = new ResizeObserver(() => {
      // Use requestAnimationFrame to ensure we run after the CSS transition completes
      requestAnimationFrame(() => {
        handleResize()
      })
    })

    resizeObserver.observe(container)

    // Also observe the parent to catch layout shifts (e.g., sidebar toggle)
    if (container.parentElement) {
      resizeObserver.observe(container.parentElement)
    }

    // Observe the grandparent (the main flex container) to catch broader layout changes
    if (container.parentElement?.parentElement) {
      resizeObserver.observe(container.parentElement.parentElement)
    }

    // Listen for explicit resize requests (e.g., from sidebar toggle)
    const forceResizeHandler = () => {
      handleResize()
    }
    window.addEventListener("resh-force-terminal-resize", forceResizeHandler)

    const wakeRefreshHandler = () => {
      if (document.visibilityState === "hidden") return
      requestAnimationFrame(refreshTerminal)
    }
    document.addEventListener("visibilitychange", wakeRefreshHandler)
    window.addEventListener("focus", wakeRefreshHandler)
    window.addEventListener("pageshow", wakeRefreshHandler)

    terminalRef.current = term
    fitAddonRef.current = fitAddon
    setIsReady(true)

    return () => {
      imeDebugCleanup?.()
      imeDebugCleanup = null
      imeCompositionCleanup?.()
      imeCompositionCleanup = null
      disposable.dispose()
      selectionDisposable.dispose()
      oscDisposable.dispose()
      resizeObserver.disconnect()
      window.removeEventListener(
        "resh-force-terminal-resize",
        forceResizeHandler,
      )
      document.removeEventListener("visibilitychange", wakeRefreshHandler)
      window.removeEventListener("focus", wakeRefreshHandler)
      window.removeEventListener("pageshow", wakeRefreshHandler)
      disposeWebglAddon(false)
      term.dispose()
      terminalRef.current = null
      webglAddonRef.current = null
      webglContextLossDisposableRef.current = null
      setIsReady(false)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [containerId])

  // Update terminal settings dynamically
  useEffect(() => {
    const term = terminalRef.current
    if (!term) return

    if (settings) {
      term.options.fontSize = settings.fontSize || 14
      term.options.fontFamily =
        settings.fontFamily ||
        "'Maple Mono NF CN', 'JetBrains Mono', 'Consolas', monospace"
      term.options.cursorStyle =
        (settings.cursorStyle as "block" | "underline" | "bar") || "block"
      term.options.scrollback = settings.scrollback || 25000
    }

    term.options.theme = getTerminalPalette(resolveTerminalTheme(theme))

    // Re-fit after settings change (e.g. font size)
    // Small timeout to ensure DOM update if necessary
    setTimeout(() => {
      fitAddonRef.current?.fit()
      // Also notify backend about resize if needed
      if (term.element && onResizeRef.current) {
        onResizeRef.current(term.cols, term.rows)
      }
    }, 10)
  }, [settings, theme])

  useEffect(() => {
    if (theme !== "system") return

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)")
    const handleSystemThemeChange = (event: MediaQueryListEvent) => {
      const term = terminalRef.current
      if (!term) return
      term.options.theme = getTerminalPalette(event.matches ? "dark" : "light")
      if (term.rows > 0) term.refresh(0, term.rows - 1)
    }

    mediaQuery.addEventListener("change", handleSystemThemeChange)
    return () => mediaQuery.removeEventListener("change", handleSystemThemeChange)
  }, [theme])

  const write = useCallback((data: string) => {
    terminalRef.current?.write(data)
  }, [])

  const focus = useCallback(() => {
    terminalRef.current?.focus()
  }, [])

  const getBufferText = useCallback(() => {
    if (!terminalRef.current) return ""
    const buffer = terminalRef.current.buffer.active
    let text = ""
    for (let i = 0; i < buffer.length; i++) {
      const line = buffer.getLine(i)
      if (line) {
        // translateToString(true) trims the trailing whitespace
        const lineText = line.translateToString(true)
        text += lineText

        // If this line is NOT wrapped to the next line, it means it's a real newline
        const nextLine = buffer.getLine(i + 1)
        if (!nextLine || !nextLine.isWrapped) {
          text += "\n"
        }
      }
    }
    return text
  }, [])

  return useMemo(
    () => ({
      terminal: terminalRef.current,
      isReady,
      write,
      focus,
      getBufferText,
    }),
    [isReady, write, focus, getBufferText],
  )
}
