import React, { useCallback, useEffect, useMemo, useRef, useState } from "react"
import MonacoEditor, { OnMount } from "@monaco-editor/react"
import type * as Monaco from "monaco-editor"
import { Save, Undo2, Redo2, TextWrap } from "lucide-react"
import { useTranslation } from "../i18n"
import type { Theme } from "../types"

interface EditorTabProps {
  tabId: string
  remotePath: string
  languageHint: string
  content: string
  encoding: string
  dirty: boolean
  terminalFontFamily: string
  terminalFontSize: number
  appTheme: Theme
  isSaving: boolean
  onChange: (value: string) => void
  onSave: () => Promise<boolean>
  onLanguageChange: (languageId: string) => void
}

const EXTENSION_LANGUAGE_MAP: Record<string, string> = {
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  json: "json",
  md: "markdown",
  markdown: "markdown",
  rs: "rust",
  py: "python",
  go: "go",
  java: "java",
  c: "c",
  h: "cpp",
  cc: "cpp",
  cxx: "cpp",
  cpp: "cpp",
  hpp: "cpp",
  cs: "csharp",
  php: "php",
  rb: "ruby",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  fish: "shell",
  yml: "yaml",
  yaml: "yaml",
  xml: "xml",
  html: "html",
  css: "css",
  scss: "scss",
  less: "less",
  sql: "sql",
  toml: "toml",
  ini: "ini",
  conf: "ini",
  dockerfile: "dockerfile",
}

const FILE_NAME_LANGUAGE_MAP: Record<string, string> = {
  dockerfile: "dockerfile",
  "docker-compose.yml": "yaml",
  "docker-compose.yaml": "yaml",
  makefile: "plaintext",
  ".bashrc": "shell",
  ".zshrc": "shell",
  ".profile": "shell",
  ".env": "ini",
}

const LANGUAGE_PICKER_ORDER = [
  "plaintext",
  "markdown",
  "yaml",
  "json",
  "typescript",
  "javascript",
  "python",
  "rust",
  "go",
  "java",
  "c",
  "cpp",
  "csharp",
  "php",
  "ruby",
  "shell",
  "html",
  "css",
  "scss",
  "less",
  "xml",
  "sql",
  "toml",
  "ini",
  "dockerfile",
]

const LANGUAGE_DISPLAY_NAME_MAP: Record<string, string> = {
  plaintext: "Plain Text",
  markdown: "Markdown",
  yaml: "YAML",
  json: "JSON",
  typescript: "TypeScript",
  javascript: "JavaScript",
  python: "Python",
  rust: "Rust",
  go: "Go",
  java: "Java",
  c: "C",
  cpp: "C++",
  csharp: "C#",
  php: "PHP",
  ruby: "Ruby",
  shell: "Shell",
  html: "HTML",
  css: "CSS",
  scss: "SCSS",
  less: "Less",
  xml: "XML",
  sql: "SQL",
  toml: "TOML",
  ini: "INI",
  dockerfile: "Dockerfile",
}

const normalizeLanguageHint = (hint: string): string => {
  const normalized = hint.trim().toLowerCase()
  if (!normalized) {
    return ""
  }
  const aliasMap: Record<string, string> = {
    text: "plaintext",
    plain: "plaintext",
    txt: "plaintext",
    shellscript: "shell",
  }
  return aliasMap[normalized] || normalized
}

const getLanguageFromPath = (remotePath: string): string => {
  const normalizedPath = remotePath.replace(/\\/g, "/")
  const fileName = normalizedPath.split("/").pop()?.toLowerCase() || ""
  if (!fileName) {
    return ""
  }
  if (FILE_NAME_LANGUAGE_MAP[fileName]) {
    return FILE_NAME_LANGUAGE_MAP[fileName]
  }
  const lastDotIndex = fileName.lastIndexOf(".")
  if (lastDotIndex <= 0 || lastDotIndex === fileName.length - 1) {
    return ""
  }
  const extension = fileName.substring(lastDotIndex + 1)
  return EXTENSION_LANGUAGE_MAP[extension] || ""
}

const getDefaultLanguageSelection = (
  languageHint: string,
  remotePath: string,
): string => {
  const hintLanguage = normalizeLanguageHint(languageHint)
  if (hintLanguage) {
    return hintLanguage
  }
  return getLanguageFromPath(remotePath) || "plaintext"
}

const resolveLanguageId = (
  monaco: typeof Monaco,
  candidates: string[],
): string => {
  const supported = new Set(
    monaco.languages
      .getLanguages()
      .map((item) => item.id.toLowerCase())
      .filter(Boolean),
  )
  for (const rawCandidate of candidates) {
    const candidate = normalizeLanguageHint(rawCandidate)
    if (candidate && supported.has(candidate)) {
      return candidate
    }
  }
  return "plaintext"
}

const getLanguageDisplayName = (languageId: string): string => {
  const normalized = normalizeLanguageHint(languageId)
  return LANGUAGE_DISPLAY_NAME_MAP[normalized] || normalized || "plaintext"
}

const MONACO_LIGHT_THEME = "resh-monaco-light"
const MONACO_DARK_THEME = "resh-monaco-dark"

const readCssVar = (
  styles: CSSStyleDeclaration,
  name: string,
  fallback: string,
): string => {
  const value = styles.getPropertyValue(name).trim()
  return value || fallback
}

const withAlpha = (color: string, alphaHex: string): string => {
  const normalized = color.trim()
  if (/^#[0-9a-fA-F]{6}$/.test(normalized)) {
    return `${normalized}${alphaHex}`
  }
  return color
}

export const EditorTab: React.FC<EditorTabProps> = ({
  tabId,
  remotePath,
  languageHint,
  content,
  encoding,
  dirty,
  terminalFontFamily,
  terminalFontSize,
  appTheme,
  isSaving,
  onChange,
  onSave,
  onLanguageChange,
}) => {
  const { t } = useTranslation()
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null)
  const monacoRef = useRef<typeof Monaco | null>(null)
  const cursorDisposableRef = useRef<Monaco.IDisposable | null>(null)
  const languageMenuRef = useRef<HTMLDivElement | null>(null)
  const [cursorLine, setCursorLine] = useState(1)
  const [cursorColumn, setCursorColumn] = useState(1)
  const [wordWrapEnabled, setWordWrapEnabled] = useState(false)
  const [isLanguageMenuOpen, setIsLanguageMenuOpen] = useState(false)
  const [supportedLanguageIds, setSupportedLanguageIds] = useState<string[]>([])
  const [selectedLanguageId, setSelectedLanguageId] = useState(() =>
    getDefaultLanguageSelection(languageHint, remotePath),
  )

  const languageOptions = useMemo(() => {
    const supportedSet =
      supportedLanguageIds.length > 0 ? new Set(supportedLanguageIds) : null
    const seen = new Set<string>()
    const ordered: string[] = []
    const appendLanguage = (rawLanguage: string) => {
      const languageId = normalizeLanguageHint(rawLanguage)
      if (!languageId || seen.has(languageId)) {
        return
      }
      if (supportedSet && !supportedSet.has(languageId)) {
        return
      }
      seen.add(languageId)
      ordered.push(languageId)
    }
    appendLanguage(selectedLanguageId)
    LANGUAGE_PICKER_ORDER.forEach(appendLanguage)
    Object.values(EXTENSION_LANGUAGE_MAP).forEach(appendLanguage)
    Object.values(FILE_NAME_LANGUAGE_MAP).forEach(appendLanguage)
    appendLanguage("plaintext")
    return ordered
  }, [selectedLanguageId, supportedLanguageIds])

  const applyModelLanguage = useCallback(() => {
    const editor = editorRef.current
    const monaco = monacoRef.current
    if (!editor || !monaco) {
      return
    }
    const model = editor.getModel()
    if (!model) {
      return
    }
    const nextLanguageId = resolveLanguageId(monaco, [
      selectedLanguageId,
      getLanguageFromPath(remotePath),
      "plaintext",
    ])
    if (model.getLanguageId() !== nextLanguageId) {
      monaco.editor.setModelLanguage(model, nextLanguageId)
    }
    if (nextLanguageId !== selectedLanguageId) {
      setSelectedLanguageId(nextLanguageId)
      onLanguageChange(nextLanguageId)
    }
  }, [selectedLanguageId, remotePath, onLanguageChange])

  const applyMonacoTheme = useCallback(() => {
    const monaco = monacoRef.current
    if (!monaco) {
      return
    }
    const root = document.documentElement
    const styles = getComputedStyle(root)
    const isLightTheme =
      appTheme === "light" ||
      (appTheme === "system" && root.classList.contains("theme-light"))
    const bgPrimary = readCssVar(
      styles,
      "--bg-primary",
      isLightTheme ? "#f8fafc" : "#020617",
    )
    const bgSecondary = readCssVar(
      styles,
      "--bg-secondary",
      isLightTheme ? "#ffffff" : "#0f172a",
    )
    const textPrimary = readCssVar(
      styles,
      "--text-primary",
      isLightTheme ? "#0f172a" : "#f8fafc",
    )
    const textSecondary = readCssVar(
      styles,
      "--text-secondary",
      isLightTheme ? "#475569" : "#94a3b8",
    )
    const textMuted = readCssVar(
      styles,
      "--text-muted",
      isLightTheme ? "#94a3b8" : "#64748b",
    )
    const accentPrimary = readCssVar(styles, "--accent-primary", "#3b82f6")
    const selectionColor = withAlpha(accentPrimary, isLightTheme ? "33" : "55")
    const inactiveSelectionColor = withAlpha(
      accentPrimary,
      isLightTheme ? "1f" : "3d",
    )
    monaco.editor.defineTheme(MONACO_LIGHT_THEME, {
      base: "vs",
      inherit: true,
      rules: [],
      colors: {
        "editor.background": bgPrimary,
        "editor.foreground": textPrimary,
        "editor.lineHighlightBackground": bgSecondary,
        "editor.selectionBackground": selectionColor,
        "editor.inactiveSelectionBackground": inactiveSelectionColor,
        "editorCursor.foreground": accentPrimary,
        "editorLineNumber.foreground": textMuted,
        "editorLineNumber.activeForeground": textSecondary,
      },
    })
    monaco.editor.defineTheme(MONACO_DARK_THEME, {
      base: "vs-dark",
      inherit: true,
      rules: [],
      colors: {
        "editor.background": bgPrimary,
        "editor.foreground": textPrimary,
        "editor.lineHighlightBackground": bgSecondary,
        "editor.selectionBackground": selectionColor,
        "editor.inactiveSelectionBackground": inactiveSelectionColor,
        "editorCursor.foreground": accentPrimary,
        "editorLineNumber.foreground": textMuted,
        "editorLineNumber.activeForeground": textSecondary,
      },
    })
    monaco.editor.setTheme(
      isLightTheme ? MONACO_LIGHT_THEME : MONACO_DARK_THEME,
    )
  }, [appTheme])

  const applyEditorTypography = useCallback(() => {
    const editor = editorRef.current
    if (!editor) {
      return
    }
    editor.updateOptions({
      fontFamily: terminalFontFamily,
      fontSize: terminalFontSize,
    })
  }, [terminalFontFamily, terminalFontSize])

  const applyEditorReadOnly = useCallback(() => {
    const editor = editorRef.current
    if (!editor) {
      return
    }
    editor.updateOptions({
      readOnly: isSaving,
    })
  }, [isSaving])

  const applyEditorWordWrap = useCallback(() => {
    const editor = editorRef.current
    if (!editor) {
      return
    }
    editor.updateOptions({
      wordWrap: wordWrapEnabled ? "on" : "off",
    })
  }, [wordWrapEnabled])

  const handleSave = useCallback(() => {
    void onSave()
  }, [onSave])

  const handleUndo = useCallback(() => {
    editorRef.current?.trigger("editor-toolbar", "undo", null)
  }, [])

  const handleRedo = useCallback(() => {
    editorRef.current?.trigger("editor-toolbar", "redo", null)
  }, [])

  const handleWordWrapToggle = useCallback(() => {
    setWordWrapEnabled((prev) => !prev)
  }, [])

  const handleLanguageMenuToggle = useCallback(() => {
    setIsLanguageMenuOpen((prev) => !prev)
  }, [])

  const handleLanguageSelect = useCallback(
    (languageId: string) => {
      const normalized = normalizeLanguageHint(languageId) || "plaintext"
      setSelectedLanguageId(normalized)
      onLanguageChange(normalized)
      setIsLanguageMenuOpen(false)
    },
    [onLanguageChange],
  )

  const handleEditorMount: OnMount = useCallback(
    (editor, monaco) => {
      cursorDisposableRef.current?.dispose()
      editorRef.current = editor
      monacoRef.current = monaco
      const position = editor.getPosition()
      if (position) {
        setCursorLine(position.lineNumber)
        setCursorColumn(position.column)
      }
      cursorDisposableRef.current = editor.onDidChangeCursorPosition(
        (event) => {
          setCursorLine(event.position.lineNumber)
          setCursorColumn(event.position.column)
        },
      )
      const nextSupportedLanguageIds = Array.from(
        new Set<string>(
          monaco.languages
            .getLanguages()
            .map((item: Monaco.languages.ILanguageExtensionPoint) =>
              item.id.toLowerCase(),
            )
            .filter((languageId: string): languageId is string =>
              Boolean(languageId),
            ),
        ),
      )
      setSupportedLanguageIds(nextSupportedLanguageIds)
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
        void onSave()
      })
      applyModelLanguage()
      applyMonacoTheme()
      applyEditorTypography()
      applyEditorReadOnly()
      applyEditorWordWrap()
      editor.focus()
    },
    [
      applyModelLanguage,
      applyMonacoTheme,
      applyEditorTypography,
      applyEditorReadOnly,
      applyEditorWordWrap,
      onSave,
    ],
  )

  useEffect(() => {
    const nextLanguageId = getDefaultLanguageSelection(languageHint, remotePath)
    setSelectedLanguageId((prev) => {
      if (prev === nextLanguageId) {
        return prev
      }
      return nextLanguageId
    })
  }, [languageHint, remotePath])

  useEffect(() => {
    applyModelLanguage()
  }, [applyModelLanguage])

  useEffect(() => {
    applyMonacoTheme()
  }, [applyMonacoTheme])

  useEffect(() => {
    const root = document.documentElement
    const observer = new MutationObserver(() => {
      applyMonacoTheme()
    })
    observer.observe(root, {
      attributes: true,
      attributeFilter: ["class"],
    })
    return () => observer.disconnect()
  }, [applyMonacoTheme])

  useEffect(() => {
    applyEditorTypography()
  }, [applyEditorTypography])

  useEffect(() => {
    applyEditorReadOnly()
  }, [applyEditorReadOnly])

  useEffect(() => {
    applyEditorWordWrap()
  }, [applyEditorWordWrap])

  useEffect(() => {
    if (!isLanguageMenuOpen) {
      return
    }
    const handleWindowMouseDown = (event: MouseEvent) => {
      const target = event.target
      if (target instanceof Node && languageMenuRef.current?.contains(target)) {
        return
      }
      setIsLanguageMenuOpen(false)
    }
    const handleWindowKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setIsLanguageMenuOpen(false)
      }
    }
    window.addEventListener("mousedown", handleWindowMouseDown)
    window.addEventListener("keydown", handleWindowKeyDown)
    return () => {
      window.removeEventListener("mousedown", handleWindowMouseDown)
      window.removeEventListener("keydown", handleWindowKeyDown)
    }
  }, [isLanguageMenuOpen])

  useEffect(() => {
    return () => {
      cursorDisposableRef.current?.dispose()
      cursorDisposableRef.current = null
    }
  }, [])

  return (
    <div className="w-full h-full flex flex-col min-h-0 bg-[var(--bg-primary)] text-[var(--text-primary)]">
      <div className="h-10 shrink-0 px-3 border-b border-[var(--glass-border)] bg-[var(--bg-secondary)] flex items-center gap-2">
        <div className="flex items-center gap-2 shrink-0">
          <button
            type="button"
            className="h-7 w-7 rounded border border-[var(--accent-primary)] bg-[var(--accent-primary)]/10 text-[var(--text-primary)] flex items-center justify-center hover:bg-[var(--accent-primary)]/20 disabled:opacity-60 disabled:cursor-not-allowed"
            onClick={handleSave}
            disabled={isSaving}
            title={isSaving ? t.saveStatus.saving : t.editorTab.save}
            aria-label={isSaving ? t.saveStatus.saving : t.editorTab.save}
          >
            <Save size={14} />
          </button>
          <button
            type="button"
            className="h-7 w-7 rounded border border-[var(--glass-border)] bg-transparent text-[var(--text-primary)] flex items-center justify-center hover:bg-[var(--bg-tertiary)] disabled:opacity-60 disabled:cursor-not-allowed"
            onClick={handleUndo}
            disabled={isSaving}
            title={t.editorTab.undo}
            aria-label={t.editorTab.undo}
          >
            <Undo2 size={14} />
          </button>
          <button
            type="button"
            className="h-7 w-7 rounded border border-[var(--glass-border)] bg-transparent text-[var(--text-primary)] flex items-center justify-center hover:bg-[var(--bg-tertiary)] disabled:opacity-60 disabled:cursor-not-allowed"
            onClick={handleRedo}
            disabled={isSaving}
            title={t.editorTab.redo}
            aria-label={t.editorTab.redo}
          >
            <Redo2 size={14} />
          </button>
          <button
            type="button"
            className={`h-7 w-7 rounded border text-[12px] flex items-center justify-center hover:bg-[var(--bg-tertiary)] disabled:opacity-60 disabled:cursor-not-allowed ${wordWrapEnabled ? "border-[var(--accent-primary)] bg-[var(--accent-primary)]/10 text-[var(--text-primary)]" : "border-[var(--glass-border)] bg-transparent text-[var(--text-primary)]"}`}
            onClick={handleWordWrapToggle}
            disabled={isSaving}
            title={
              wordWrapEnabled
                ? t.editorTab.wordWrapDisable
                : t.editorTab.wordWrapEnable
            }
            aria-label={
              wordWrapEnabled
                ? t.editorTab.wordWrapDisable
                : t.editorTab.wordWrapEnable
            }
          >
            <TextWrap size={14} />
          </button>
          <div className="relative shrink-0" ref={languageMenuRef}>
            <button
              type="button"
              className="h-7 min-w-[120px] max-w-[180px] px-2 rounded border border-[var(--glass-border)] bg-transparent text-[12px] text-[var(--text-primary)] flex items-center justify-between gap-2 hover:bg-[var(--bg-tertiary)] disabled:opacity-60 disabled:cursor-not-allowed"
              onClick={handleLanguageMenuToggle}
              disabled={isSaving}
              title={t.editorTab.language}
              aria-label={t.editorTab.language}
              aria-haspopup="menu"
              aria-expanded={isLanguageMenuOpen}
            >
              <span className="truncate">
                {getLanguageDisplayName(selectedLanguageId)}
              </span>
              <span className="text-[10px] leading-none">▾</span>
            </button>
            {isLanguageMenuOpen ? (
              <div className="absolute top-[calc(100%+6px)] left-0 z-20 w-[180px] max-h-56 overflow-y-auto rounded border border-[var(--glass-border)] bg-[var(--bg-secondary)] py-1 shadow-lg">
                {languageOptions.map((languageId) => (
                  <button
                    key={languageId}
                    type="button"
                    className={`w-full h-7 px-2 text-[12px] flex items-center justify-between gap-2 text-left hover:bg-[var(--bg-tertiary)] ${languageId === selectedLanguageId ? "text-[var(--accent-primary)]" : "text-[var(--text-primary)]"}`}
                    onClick={() => handleLanguageSelect(languageId)}
                    title={getLanguageDisplayName(languageId)}
                  >
                    <span className="truncate">
                      {getLanguageDisplayName(languageId)}
                    </span>
                    <span className="text-[11px] leading-none">
                      {languageId === selectedLanguageId ? "✓" : ""}
                    </span>
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        </div>
        <div
          className="min-w-0 flex-1 text-[12px] text-[var(--text-secondary)] truncate text-right"
          title={remotePath}
        >
          {remotePath}
        </div>
      </div>
      <div className="flex-1 min-h-0">
        <MonacoEditor
          key={tabId}
          path={`sftp://${tabId}${remotePath}`}
          value={content}
          defaultLanguage="plaintext"
          onMount={handleEditorMount}
          onChange={(value) => onChange(value ?? "")}
          options={{
            automaticLayout: true,
            minimap: { enabled: false },
            scrollBeyondLastLine: false,
            wordWrap: wordWrapEnabled ? "on" : "off",
            tabSize: 2,
            insertSpaces: true,
            fontFamily: terminalFontFamily,
            fontSize: terminalFontSize,
            readOnly: isSaving,
          }}
        />
      </div>
      <div className="h-7 shrink-0 px-3 border-t border-[var(--glass-border)] bg-[var(--bg-secondary)] flex items-center justify-between gap-3 text-[11px] text-[var(--text-secondary)]">
        <div className="truncate" title={encoding}>
          {t.editorTab.encoding.replace("{encoding}", encoding)}
        </div>
        <div className="flex items-center gap-3 shrink-0">
          <span className={dirty ? "text-[var(--danger)]" : ""}>
            {dirty ? t.editorTab.dirty : t.editorTab.saved}
          </span>
          <span>
            {t.editorTab.lineColumn
              .replace("{line}", String(cursorLine))
              .replace("{column}", String(cursorColumn))}
          </span>
        </div>
      </div>
    </div>
  )
}
