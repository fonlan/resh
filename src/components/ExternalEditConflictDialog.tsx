import { AlertTriangle, PauseCircle, RefreshCw, Upload } from "lucide-react"
import { useEffect, useRef, useState } from "react"
import { useTranslation } from "../i18n"
import type { ExternalEditConflictState } from "./main/types"

interface ExternalEditConflictDialogProps {
  conflict: ExternalEditConflictState
  isResolving: boolean
  onAdoptRemote: () => Promise<void>
  onOverwrite: () => Promise<void>
  onRecreate: () => Promise<void>
  onKeepPaused: () => Promise<void>
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

export function ExternalEditConflictDialog({
  conflict,
  isResolving,
  onAdoptRemote,
  onOverwrite,
  onRecreate,
  onKeepPaused,
}: ExternalEditConflictDialogProps) {
  const { t } = useTranslation()
  const dialogRef = useRef<HTMLDivElement | null>(null)
  const previouslyFocusedRef = useRef<HTMLElement | null>(null)
  const [confirmingOverwrite, setConfirmingOverwrite] = useState(false)
  const isDeleted = conflict.reason === "deleted" || !conflict.currentRevision.exists

  useEffect(() => {
    setConfirmingOverwrite(false)
  }, [conflict.conflictId, conflict.reason, conflict.currentRevision])

  useEffect(() => {
    previouslyFocusedRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null

    const focusFirstAction = () => {
      dialogRef.current
        ?.querySelector<HTMLElement>("[data-external-edit-conflict-focusable]")
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
        if (isResolving) return
        event.preventDefault()
        if (confirmingOverwrite) {
          setConfirmingOverwrite(false)
        } else {
          void onKeepPaused()
        }
        return
      }
      if (event.key !== "Tab") return

      const focusable = Array.from(
        dialog.querySelectorAll<HTMLElement>(
          "[data-external-edit-conflict-focusable]",
        ),
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
  }, [confirmingOverwrite, isResolving, onKeepPaused])

  const reason = isDeleted
    ? t.mainWindow.externalEditConflict.remoteDeleted
    : t.mainWindow.externalEditConflict.remoteChanged

  return (
    <div className="fixed inset-0 z-[1310] flex items-center justify-center bg-black/55 p-4 backdrop-blur-sm">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="external-edit-conflict-title"
        className="flex max-h-[min(720px,calc(100vh-2rem))] w-[min(720px,96vw)] flex-col overflow-hidden rounded-[var(--radius-md)] border border-[var(--glass-border)] bg-[var(--bg-secondary)] shadow-[0_24px_64px_rgba(0,0,0,0.45)]"
      >
        <div className="flex items-start gap-3 border-b border-[var(--glass-border)] px-5 py-4">
          <AlertTriangle
            className="mt-0.5 shrink-0 text-amber-500"
            size={20}
            aria-hidden="true"
          />
          <div className="min-w-0">
            <h2
              id="external-edit-conflict-title"
              className="text-[16px] font-semibold text-[var(--text-primary)]"
            >
              {t.mainWindow.externalEditConflict.title}
            </h2>
            <p className="mt-1 break-all text-[13px] leading-relaxed text-[var(--text-secondary)]">
              {t.mainWindow.externalEditConflict.description.replace(
                "{path}",
                conflict.remotePath,
              )}
            </p>
          </div>
        </div>

        <div className="min-h-0 overflow-y-auto px-5 py-4">
          <div className="rounded-[var(--radius-sm)] border border-amber-500/35 bg-amber-500/10 px-3 py-2 text-[12px] text-[var(--text-primary)]">
            {t.mainWindow.externalEditConflict.autoUploadPaused}
          </div>
          <div className="mt-3 rounded-[var(--radius-sm)] border border-[var(--glass-border)] bg-[var(--bg-primary)] px-3 py-2 text-[12px] text-[var(--text-secondary)]">
            {reason}
          </div>
          {conflict.pendingLocalChanges ? (
            <div className="mt-3 rounded-[var(--radius-sm)] border border-[var(--accent-primary)]/35 bg-[var(--accent-primary)]/10 px-3 py-2 text-[12px] text-[var(--text-primary)]">
              {t.mainWindow.externalEditConflict.pendingLocal}
            </div>
          ) : null}

          <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-[12px]">
            <dt className="text-[var(--text-secondary)]">
              {t.mainWindow.externalEditConflict.remotePath}
            </dt>
            <dd className="break-all text-[var(--text-primary)]">
              {conflict.remotePath}
            </dd>
            <dt className="text-[var(--text-secondary)]">
              {t.mainWindow.externalEditConflict.localPath}
            </dt>
            <dd className="break-all text-[var(--text-primary)]">
              {conflict.localPath}
            </dd>
            <dt className="text-[var(--text-secondary)]">
              {t.mainWindow.externalEditConflict.remoteSize}
            </dt>
            <dd className="text-[var(--text-primary)]">
              {conflict.currentRevision.exists
                ? formatSize(conflict.currentRevision.size)
                : t.mainWindow.externalEditConflict.notAvailable}
            </dd>
            <dt className="text-[var(--text-secondary)]">
              {t.mainWindow.externalEditConflict.remoteModified}
            </dt>
            <dd className="text-[var(--text-primary)]">
              {conflict.currentRevision.exists
                ? formatModified(conflict.currentRevision.mtime)
                : t.mainWindow.externalEditConflict.notAvailable}
            </dd>
          </dl>

          {conflict.snapshotError ? (
            <p className="mt-4 rounded-[var(--radius-sm)] border border-[var(--glass-border)] bg-[var(--bg-primary)] px-3 py-2 text-[12px] text-[var(--text-secondary)]">
              {t.mainWindow.externalEditConflict.snapshotUnavailable.replace(
                "{error}",
                conflict.snapshotError,
              )}
            </p>
          ) : null}

          {isDeleted ? (
            <p className="mt-4 text-[12px] leading-relaxed text-[var(--text-secondary)]">
              {t.mainWindow.externalEditConflict.deletedHint}
            </p>
          ) : (
            <p className="mt-4 text-[12px] leading-relaxed text-[var(--text-secondary)]">
              {t.mainWindow.externalEditConflict.changedHint}
            </p>
          )}
        </div>

        <div className="border-t border-[var(--glass-border)] px-5 py-4">
          {confirmingOverwrite ? (
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p className="max-w-[520px] text-[12px] leading-relaxed text-[var(--danger)]">
                {isDeleted
                  ? t.mainWindow.externalEditConflict.recreateConfirm
                  : t.mainWindow.externalEditConflict.overwriteConfirm}
              </p>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  data-external-edit-conflict-focusable
                  className="h-8 rounded border border-[var(--glass-border)] px-3 text-[12px] text-[var(--text-primary)] hover:bg-[var(--bg-tertiary)] disabled:cursor-not-allowed disabled:opacity-60"
                  disabled={isResolving}
                  onClick={() => setConfirmingOverwrite(false)}
                >
                  {t.common.cancel}
                </button>
                <button
                  type="button"
                  data-external-edit-conflict-focusable
                  className="h-8 rounded border border-red-500/70 bg-red-500/15 px-3 text-[12px] text-red-600 hover:bg-red-500/25 dark:text-red-300 disabled:cursor-not-allowed disabled:opacity-60"
                  disabled={isResolving}
                  onClick={() => void (isDeleted ? onRecreate() : onOverwrite())}
                >
                  {isResolving
                    ? t.saveStatus.saving
                    : isDeleted
                      ? t.mainWindow.externalEditConflict.recreateConfirmAction
                      : t.mainWindow.externalEditConflict.overwriteConfirmAction}
                </button>
              </div>
            </div>
          ) : (
            <div className="flex flex-wrap items-center justify-between gap-3">
              <button
                type="button"
                data-external-edit-conflict-focusable
                className="inline-flex h-8 items-center gap-1.5 rounded border border-[var(--glass-border)] px-3 text-[12px] text-[var(--text-secondary)] hover:bg-[var(--bg-tertiary)] disabled:cursor-not-allowed disabled:opacity-60"
                disabled={isResolving}
                onClick={() => void onKeepPaused()}
              >
                <PauseCircle size={14} />
                {t.mainWindow.externalEditConflict.keepPaused}
              </button>
              <div className="flex flex-wrap items-center justify-end gap-2">
                {!isDeleted ? (
                  <button
                    type="button"
                    data-external-edit-conflict-focusable
                    className="inline-flex h-8 items-center gap-1.5 rounded border border-[var(--accent-primary)] bg-[var(--accent-primary)]/15 px-3 text-[12px] text-[var(--text-primary)] hover:bg-[var(--accent-primary)]/25 disabled:cursor-not-allowed disabled:opacity-60"
                    disabled={isResolving}
                    onClick={() => void onAdoptRemote()}
                  >
                    <RefreshCw size={14} />
                    {t.mainWindow.externalEditConflict.adoptRemote}
                  </button>
                ) : null}
                <button
                  type="button"
                  data-external-edit-conflict-focusable
                  className="inline-flex h-8 items-center gap-1.5 rounded border border-red-500/70 bg-red-500/15 px-3 text-[12px] text-red-600 hover:bg-red-500/25 dark:text-red-300 disabled:cursor-not-allowed disabled:opacity-60"
                  disabled={isResolving}
                  onClick={() => setConfirmingOverwrite(true)}
                >
                  <Upload size={14} />
                  {isDeleted
                    ? t.mainWindow.externalEditConflict.recreateRemote
                    : t.mainWindow.externalEditConflict.overwriteRemote}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
