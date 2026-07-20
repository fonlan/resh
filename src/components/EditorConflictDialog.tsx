import { DiffEditor } from "@monaco-editor/react"
import { AlertTriangle, Eye, RefreshCw } from "lucide-react"
import { useEffect, useRef, useState } from "react"
import { useTranslation } from "../i18n"
import type { EditorConflictState } from "./main/types"

interface EditorConflictDialogProps {
  conflict: EditorConflictState
  remotePath: string
  localContent: string
  language: string
  isSaving: boolean
  onAdoptRemote: () => void
  onOverwrite: () => Promise<void>
  onClose: () => void
}

const formatSize = (size: number | null | undefined): string => {
  if (size === null || size === undefined) return "-"
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}

const formatModified = (mtime: number | null | undefined): string => {
  if (!mtime) return "-"
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "medium",
  }).format(new Date(mtime * 1000))
}

export function EditorConflictDialog({
  conflict,
  remotePath,
  localContent,
  language,
  isSaving,
  onAdoptRemote,
  onOverwrite,
  onClose,
}: EditorConflictDialogProps) {
  const { t } = useTranslation()
  const dialogRef = useRef<HTMLDivElement | null>(null)
  const previouslyFocusedRef = useRef<HTMLElement | null>(null)
  const [showDiff, setShowDiff] = useState(false)
  const [confirmingOverwrite, setConfirmingOverwrite] = useState(false)
  const remoteContentAvailable = conflict.remoteContent !== null

  useEffect(() => {
    setShowDiff(false)
    setConfirmingOverwrite(false)
  }, [conflict])

  useEffect(() => {
    previouslyFocusedRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null

    const focusFirstAction = () => {
      dialogRef.current
        ?.querySelector<HTMLElement>("[data-editor-conflict-focusable]")
        ?.focus()
    }
    const frame = window.requestAnimationFrame(focusFirstAction)
    return () => {
      window.cancelAnimationFrame(frame)
      previouslyFocusedRef.current?.focus?.()
      previouslyFocusedRef.current = null
    }
  }, [])

  useEffect(() => {
    const dialog = dialogRef.current
    if (!dialog) return

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing) return
      if (event.key === "Escape") {
        if (isSaving) return
        event.preventDefault()
        if (confirmingOverwrite) {
          setConfirmingOverwrite(false)
        } else {
          onClose()
        }
        return
      }
      if (event.key !== "Tab") return

      const focusable = Array.from(
        dialog.querySelectorAll<HTMLElement>("[data-editor-conflict-focusable]"),
      ).filter((element) => !element.hasAttribute("disabled"))
      if (focusable.length === 0) return
      const currentIndex = focusable.indexOf(document.activeElement as HTMLElement)
      if (event.shiftKey) {
        if (currentIndex <= 0) {
          event.preventDefault()
          focusable[focusable.length - 1].focus()
        }
      } else if (currentIndex === -1 || currentIndex === focusable.length - 1) {
        event.preventDefault()
        focusable[0].focus()
      }
    }

    window.addEventListener("keydown", handleKeyDown)
    return () => {
      window.removeEventListener("keydown", handleKeyDown)
    }
  }, [confirmingOverwrite, isSaving, onClose])

  const reason =
    conflict.reason === "deleted"
      ? t.mainWindow.editorConflict.remoteDeleted
      : t.mainWindow.editorConflict.remoteChanged

  return (
    <div className="fixed inset-0 z-[1300] flex items-center justify-center bg-black/55 p-4 backdrop-blur-sm">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="editor-conflict-title"
        className="flex max-h-[min(820px,calc(100vh-2rem))] w-[min(980px,96vw)] flex-col overflow-hidden rounded-[var(--radius-md)] border border-[var(--glass-border)] bg-[var(--bg-secondary)] shadow-[0_24px_64px_rgba(0,0,0,0.45)]"
      >
        <div className="flex items-start gap-3 border-b border-[var(--glass-border)] px-5 py-4">
          <AlertTriangle
            className="mt-0.5 shrink-0 text-amber-500"
            size={20}
            aria-hidden="true"
          />
          <div className="min-w-0">
            <h2
              id="editor-conflict-title"
              className="text-[16px] font-semibold text-[var(--text-primary)]"
            >
              {t.mainWindow.editorConflict.title}
            </h2>
            <p className="mt-1 break-all text-[13px] leading-relaxed text-[var(--text-secondary)]">
              {t.mainWindow.editorConflict.description.replace("{path}", remotePath)}
            </p>
          </div>
        </div>

        <div className="min-h-0 overflow-y-auto px-5 py-4">
          <div className="rounded-[var(--radius-sm)] border border-amber-500/35 bg-amber-500/10 px-3 py-2 text-[12px] text-[var(--text-primary)]">
            {reason}
          </div>

          <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-[12px]">
            <dt className="text-[var(--text-secondary)]">
              {t.mainWindow.editorConflict.remotePath}
            </dt>
            <dd className="break-all text-[var(--text-primary)]">{remotePath}</dd>
            <dt className="text-[var(--text-secondary)]">
              {t.mainWindow.editorConflict.remoteSize}
            </dt>
            <dd className="text-[var(--text-primary)]">
              {conflict.currentRevision.exists
                ? formatSize(conflict.currentRevision.size)
                : t.mainWindow.editorConflict.notAvailable}
            </dd>
            <dt className="text-[var(--text-secondary)]">
              {t.mainWindow.editorConflict.remoteModified}
            </dt>
            <dd className="text-[var(--text-primary)]">
              {conflict.currentRevision.exists
                ? formatModified(conflict.currentRevision.mtime)
                : t.mainWindow.editorConflict.notAvailable}
            </dd>
          </dl>

          {conflict.snapshotError ? (
            <p className="mt-4 rounded-[var(--radius-sm)] border border-[var(--glass-border)] bg-[var(--bg-primary)] px-3 py-2 text-[12px] text-[var(--text-secondary)]">
              {t.mainWindow.editorConflict.snapshotUnavailable.replace(
                "{error}",
                conflict.snapshotError,
              )}
            </p>
          ) : null}

          {showDiff && remoteContentAvailable ? (
            <div className="mt-4">
              <div className="mb-2 flex items-center justify-between gap-3 text-[12px] text-[var(--text-secondary)]">
                <span>{t.mainWindow.editorConflict.localVersion}</span>
                <span>{t.mainWindow.editorConflict.remoteVersion}</span>
              </div>
              <div className="h-[min(52vh,480px)] overflow-hidden rounded-[var(--radius-sm)] border border-[var(--glass-border)]">
                <DiffEditor
                  height="100%"
                  language={language || "plaintext"}
                  original={localContent}
                  modified={conflict.remoteContent ?? ""}
                  options={{
                    readOnly: true,
                    originalEditable: false,
                    renderSideBySide: true,
                    minimap: { enabled: false },
                    scrollBeyondLastLine: false,
                    automaticLayout: true,
                  }}
                />
              </div>
            </div>
          ) : null}
        </div>

        <div className="border-t border-[var(--glass-border)] px-5 py-4">
          {confirmingOverwrite ? (
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p className="max-w-[560px] text-[12px] leading-relaxed text-[var(--danger)]">
                {t.mainWindow.editorConflict.overwriteConfirm}
              </p>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  data-editor-conflict-focusable
                  className="h-8 rounded border border-[var(--glass-border)] px-3 text-[12px] text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)] disabled:cursor-not-allowed disabled:opacity-60"
                  disabled={isSaving}
                  onClick={() => setConfirmingOverwrite(false)}
                >
                  {t.common.cancel}
                </button>
                <button
                  type="button"
                  data-editor-conflict-focusable
                  className="h-8 rounded border border-red-500/70 bg-red-500/15 px-3 text-[12px] text-red-600 hover:bg-red-500/25 dark:text-red-300 disabled:cursor-not-allowed disabled:opacity-60"
                  disabled={isSaving}
                  onClick={() => void onOverwrite()}
                >
                  {isSaving
                    ? t.saveStatus.saving
                    : t.mainWindow.editorConflict.overwriteConfirmAction}
                </button>
              </div>
            </div>
          ) : (
            <div className="flex flex-wrap items-center justify-between gap-3">
              <button
                type="button"
                data-editor-conflict-focusable
                className="inline-flex h-8 items-center gap-1.5 rounded border border-[var(--glass-border)] px-3 text-[12px] text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)] disabled:cursor-not-allowed disabled:opacity-60"
                disabled={!remoteContentAvailable || isSaving}
                title={
                  remoteContentAvailable
                    ? t.mainWindow.editorConflict.viewDiff
                    : t.mainWindow.editorConflict.snapshotUnavailableShort
                }
                onClick={() => setShowDiff((value) => !value)}
              >
                <Eye size={14} />
                {showDiff
                  ? t.mainWindow.editorConflict.hideDiff
                  : t.mainWindow.editorConflict.viewDiff}
              </button>
              <div className="flex flex-wrap items-center justify-end gap-2">
                <button
                  type="button"
                  data-editor-conflict-focusable
                  className="h-8 rounded border border-[var(--glass-border)] px-3 text-[12px] text-[var(--text-secondary)] hover:bg-[var(--bg-tertiary)] disabled:cursor-not-allowed disabled:opacity-60"
                  disabled={isSaving}
                  onClick={onClose}
                >
                  {t.common.cancel}
                </button>
                <button
                  type="button"
                  data-editor-conflict-focusable
                  className="inline-flex h-8 items-center gap-1.5 rounded border border-[var(--accent-primary)] bg-[var(--accent-primary)]/15 px-3 text-[12px] text-[var(--text-primary)] hover:bg-[var(--accent-primary)]/25 disabled:cursor-not-allowed disabled:opacity-60"
                  disabled={!remoteContentAvailable || isSaving}
                  onClick={onAdoptRemote}
                >
                  <RefreshCw size={14} />
                  {t.mainWindow.editorConflict.adoptRemote}
                </button>
                <button
                  type="button"
                  data-editor-conflict-focusable
                  className="h-8 rounded border border-red-500/70 bg-red-500/15 px-3 text-[12px] text-red-600 hover:bg-red-500/25 dark:text-red-300 disabled:cursor-not-allowed disabled:opacity-60"
                  disabled={isSaving}
                  onClick={() => setConfirmingOverwrite(true)}
                >
                  {t.mainWindow.editorConflict.overwriteRemote}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
