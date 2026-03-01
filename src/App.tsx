import { Suspense, lazy, useEffect } from "react"
import { emit } from "@tauri-apps/api/event"
import { useConfig } from "./hooks/useConfig"
import { useTheme } from "./hooks/useTheme"

const MainWindow = lazy(() =>
  import("./components/MainWindow").then((module) => ({
    default: module.MainWindow,
  })),
)

const AppReadySignal = () => {
  useEffect(() => {
    void emit("resh-app-ready").catch(() => {})
  }, [])

  return null
}

const NON_TEXT_INPUT_TYPES = new Set([
  "button",
  "checkbox",
  "color",
  "file",
  "hidden",
  "image",
  "radio",
  "range",
  "reset",
  "submit",
])

const isNativeContextMenuAllowedTarget = (
  target: EventTarget | null,
): boolean => {
  if (!(target instanceof Element)) {
    return false
  }

  const editableElement = target.closest("input, textarea")
  if (editableElement instanceof HTMLTextAreaElement) {
    return true
  }

  if (!(editableElement instanceof HTMLInputElement)) {
    return false
  }

  const inputType = (editableElement.type || "text").toLowerCase()
  return !NON_TEXT_INPUT_TYPES.has(inputType)
}

function App() {
  const { config, loading } = useConfig()
  const theme = config?.general.theme

  useTheme(theme)

  useEffect(() => {
    const handleGlobalContextMenu = (event: MouseEvent) => {
      if (isNativeContextMenuAllowedTarget(event.target)) {
        return
      }

      event.preventDefault()
    }

    document.addEventListener("contextmenu", handleGlobalContextMenu, true)
    return () => {
      document.removeEventListener("contextmenu", handleGlobalContextMenu, true)
    }
  }, [])

  if (loading) {
    return null
  }

  return (
    <Suspense fallback={null}>
      <div className="w-full h-screen flex flex-col">
        <AppReadySignal />
        <MainWindow />
      </div>
    </Suspense>
  )
}

export default App
