export type ClientPlatform = "macos" | "windows" | "linux" | "unknown"

export const getClientPlatform = (): ClientPlatform => {
  if (typeof navigator === "undefined") {
    return "unknown"
  }

  const descriptor = `${navigator.platform || ""} ${navigator.userAgent || ""}`
  if (/Mac|iPhone|iPad|iPod/i.test(descriptor)) {
    return "macos"
  }
  if (/Win/i.test(descriptor)) {
    return "windows"
  }
  if (/Linux|X11/i.test(descriptor)) {
    return "linux"
  }
  return "unknown"
}

export const isMacOS = (): boolean => getClientPlatform() === "macos"

export const getEditorPathPlaceholder = (fallback: string): string => {
  switch (getClientPlatform()) {
    case "macos":
      return "/Applications/Visual Studio Code.app"
    case "linux":
      return "/usr/bin/code"
    default:
      return fallback
  }
}
