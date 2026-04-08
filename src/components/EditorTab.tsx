import React, { useCallback, useEffect, useRef } from "react"
import MonacoEditor, { OnMount } from "@monaco-editor/react"
import type * as Monaco from "monaco-editor"
import { Save, Undo2, Redo2 } from "lucide-react"
import { useTranslation } from "../i18n"

interface EditorTabProps {
  tabId: string
  remotePath: string
  languageHint: string
  content: string
  isSaving: boolean
  onChange: (value: string) => void
  onSave: () => Promise<boolean>
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

const resolveLanguageId = (
  monaco: typeof Monaco,
  languageHint: string,
  remotePath: string,
): string => {
  const supported = new Set(
    monaco.languages
      .getLanguages()
      .map((item) => item.id.toLowerCase())
      .filter(Boolean),
  )
  const candidates = [
    normalizeLanguageHint(languageHint),
    getLanguageFromPath(remotePath),
    "plaintext",
  ]
  for (const candidate of candidates) {
    if (candidate && supported.has(candidate)) {
      return candidate
    }
  }
  return "plaintext"
}

export const EditorTab: React.FC<EditorTabProps> = ({
  tabId,
  remotePath,
  languageHint,
  content,
  isSaving,
  onChange,
  onSave,
}) => {
  const { t } = useTranslation()
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null)
  const monacoRef = useRef<typeof Monaco | null>(null)

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
    const nextLanguageId = resolveLanguageId(monaco, languageHint, remotePath)
    if (model.getLanguageId() !== nextLanguageId) {
      monaco.editor.setModelLanguage(model, nextLanguageId)
    }
  }, [languageHint, remotePath])

  const handleSave = useCallback(() => {
    void onSave()
  }, [onSave])

  const handleUndo = useCallback(() => {
    editorRef.current?.trigger("editor-toolbar", "undo", null)
  }, [])

  const handleRedo = useCallback(() => {
    editorRef.current?.trigger("editor-toolbar", "redo", null)
  }, [])

  const handleEditorMount: OnMount = useCallback(
    (editor, monaco) => {
      editorRef.current = editor
      monacoRef.current = monaco
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
        void onSave()
      })
      applyModelLanguage()
      editor.focus()
    },
    [applyModelLanguage, onSave],
  )

  useEffect(() => {
    applyModelLanguage()
  }, [applyModelLanguage])

  return (
    <div className="w-full h-full flex flex-col min-h-0 bg-[var(--bg-primary)] text-[var(--text-primary)]">
      <div className="h-10 shrink-0 px-3 border-b border-[var(--glass-border)] bg-[var(--bg-secondary)] flex items-center justify-between gap-2">
        <div className="min-w-0 text-[12px] text-[var(--text-secondary)] truncate">
          {remotePath}
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <button
            type="button"
            className="h-7 px-2 rounded border border-[var(--glass-border)] bg-transparent text-[var(--text-primary)] text-[12px] flex items-center gap-1 hover:bg-[var(--bg-tertiary)] disabled:opacity-60 disabled:cursor-not-allowed"
            onClick={handleUndo}
            disabled={isSaving}
          >
            <Undo2 size={14} /> Undo
          </button>
          <button
            type="button"
            className="h-7 px-2 rounded border border-[var(--glass-border)] bg-transparent text-[var(--text-primary)] text-[12px] flex items-center gap-1 hover:bg-[var(--bg-tertiary)] disabled:opacity-60 disabled:cursor-not-allowed"
            onClick={handleRedo}
            disabled={isSaving}
          >
            <Redo2 size={14} /> Redo
          </button>
          <button
            type="button"
            className="h-7 px-2 rounded border border-[var(--accent-primary)] bg-[var(--accent-primary)]/10 text-[var(--text-primary)] text-[12px] flex items-center gap-1 hover:bg-[var(--accent-primary)]/20 disabled:opacity-60 disabled:cursor-not-allowed"
            onClick={handleSave}
            disabled={isSaving}
          >
            <Save size={14} /> {isSaving ? t.saveStatus.saving : t.common.save}
          </button>
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
            wordWrap: "on",
            tabSize: 2,
            insertSpaces: true,
          }}
        />
      </div>
    </div>
  )
}
