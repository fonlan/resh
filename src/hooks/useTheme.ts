import { useEffect } from "react"

export function useTheme(
  theme?: "light" | "dark" | "orange" | "green" | "system",
) {
  useEffect(() => {
    if (!theme) return

    const root = document.documentElement
    root.classList.remove(
      "theme-light",
      "theme-dark",
      "theme-orange",
      "theme-green",
      "theme-system",
    )

    if (theme === "system") {
      const prefersDark = window.matchMedia(
        "(prefers-color-scheme: dark)",
      ).matches
      root.classList.add(prefersDark ? "theme-dark" : "theme-light")
      root.style.colorScheme = prefersDark ? "dark" : "light"

      const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)")
      const handleChange = (e: MediaQueryListEvent) => {
        root.classList.remove("theme-light", "theme-dark")
        root.classList.add(e.matches ? "theme-dark" : "theme-light")
        root.style.colorScheme = e.matches ? "dark" : "light"
      }
      mediaQuery.addEventListener("change", handleChange)
      return () => mediaQuery.removeEventListener("change", handleChange)
    } else {
      root.classList.add(`theme-${theme}`)
      root.style.colorScheme = theme === "light" ? "light" : "dark"
    }
  }, [theme])
}
