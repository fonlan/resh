import React, { useEffect, useMemo, useRef } from "react"
import { Loader2, Download, RefreshCw, AlertCircle, CheckCircle2 } from "lucide-react"
import { useTranslation } from "../i18n"
import {
  useUpdateStore,
  selectUpdateStatus,
  selectUpdateInfo,
  selectUpdateProgress,
  selectPreparedUpdate,
  selectUpdateErrorMessage,
  selectRestartWaitTimedOut,
} from "../stores/useUpdateStore"
import type { OperationSnapshot } from "../types/update"
import {
  formatBytes,
  updateManagerApi,
} from "../hooks/useUpdateManager"
import { getOperationSnapshot as invokeOpSnapshot } from "../utils/restartUpdate"

interface UpdateDialogProps {
  isOpen: boolean
  onClose: () => void
}

export const UpdateDialog: React.FC<UpdateDialogProps> = ({
  isOpen,
  onClose,
}) => {
  const { t } = useTranslation()
  const status = useUpdateStore(selectUpdateStatus)
  const update = useUpdateStore(selectUpdateInfo)
  const progress = useUpdateStore(selectUpdateProgress)
  const prepared = useUpdateStore(selectPreparedUpdate)
  const errorMessage = useUpdateStore(selectUpdateErrorMessage)
  const restartWaitTimedOut = useUpdateStore(selectRestartWaitTimedOut)
  const currentVersion = useUpdateStore((s) => s.currentVersion)
  const panelRef = useRef<HTMLDivElement>(null)
  const previouslyFocusedRef = useRef<HTMLElement | null>(null)
  const [drainSnapshot, setDrainSnapshot] = React.useState<
    OperationSnapshot | null
  >(null)

  const categoryLabel = (category: string) => {
    switch (category) {
      case "configWrite":
        return t.updateRestartCategoryConfig
      case "webdavSync":
        return t.updateRestartCategorySync
      case "sftpTransfer":
        return t.updateRestartCategoryTransfer
      case "sftpEditUpload":
        return t.updateRestartCategoryEditUpload
      default:
        return category
    }
  }

  // Poll backend operation snapshot while waiting for safe restart.
  useEffect(() => {
    if (status !== "waitingForSafeRestart" && status !== "restarting") {
      setDrainSnapshot(null)
      return
    }
    let cancelled = false
    const tick = async () => {
      try {
        const snap = await invokeOpSnapshot()
        if (!cancelled) setDrainSnapshot(snap)
      } catch {
        // ignore poll errors
      }
    }
    void tick()
    const id = window.setInterval(() => void tick(), 500)
    return () => {
      cancelled = true
      window.clearInterval(id)
    }
  }, [status])

  const targetVersion = update?.version ?? prepared?.version ?? "—"
  const current =
    currentVersion ?? update?.currentVersion ?? prepared?.currentVersion ?? "—"
  const size = update?.installAsset.size ?? prepared?.size ?? null

  const phaseLabel = useMemo(() => {
    if (status === "checking") return t.updateStatusChecking
    if (status === "downloading") {
      if (progress?.phase === "checksums") return t.updatePhaseChecksums
      if (progress?.phase === "verifying") return t.updatePhaseVerifying
      return t.updateStatusDownloading
    }
    if (status === "ready") return t.updateStatusReady
    if (status === "upToDate") return t.updateStatusUpToDate
    if (status === "available")
      return t.updateStatusAvailable.replace("{version}", targetVersion)
    if (status === "error") return t.updateStatusError
    if (status === "waitingForSafeRestart") return t.updateStatusWaitingRestart
    if (status === "restarting") return t.updateStatusInstalling
    return t.softwareUpdate
  }, [status, progress?.phase, t, targetVersion])

  const isChecking = status === "checking"
  const isDownloading = status === "downloading"
  const canDownload =
    (status === "available" || status === "error") && !!update && !isDownloading
  const canRetryCheck =
    status === "error" || status === "upToDate" || status === "idle"
  const showRestartConfirm = status === "ready"
  const isWaitingRestart =
    status === "waitingForSafeRestart" || status === "restarting"

  useEffect(() => {
    if (!isOpen) return

    previouslyFocusedRef.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null

    const focusFirst = () => {
      const root = panelRef.current
      if (!root) return
      const focusable = root.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      )
      ;(focusable[0] ?? root).focus()
    }
    const frame = window.requestAnimationFrame(focusFirst)

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.isComposing) return
      if (e.key === "Escape") {
        if (isDownloading || isWaitingRestart) return
        e.preventDefault()
        onClose()
        return
      }
      if (e.key !== "Tab" || !panelRef.current) return
      const focusable = Array.from(
        panelRef.current.querySelectorAll<HTMLElement>(
          'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((el) => !el.hasAttribute("disabled") && el.tabIndex !== -1)
      if (focusable.length === 0) {
        e.preventDefault()
        panelRef.current.focus()
        return
      }
      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      const active = document.activeElement as HTMLElement | null
      if (e.shiftKey) {
        if (active === first || !panelRef.current.contains(active)) {
          e.preventDefault()
          last.focus()
        }
      } else if (active === last) {
        e.preventDefault()
        first.focus()
      }
    }

    document.addEventListener("keydown", onKeyDown)
    return () => {
      window.cancelAnimationFrame(frame)
      document.removeEventListener("keydown", onKeyDown)
      previouslyFocusedRef.current?.focus?.()
      previouslyFocusedRef.current = null
    }
  }, [isOpen, isDownloading, isWaitingRestart, onClose])

  if (!isOpen) return null

  const percent = progress?.percent
  const progressText =
    progress != null
      ? `${formatBytes(progress.received)}${
          progress.total != null ? ` / ${formatBytes(progress.total)}` : ""
        }${percent != null ? ` (${percent}%)` : ""}`
      : null

  return (
    <div
      className="fixed inset-0 flex items-center justify-center z-[2000] animate-in fade-in duration-200"
      style={{
        background: "rgba(2, 6, 23, 0.6)",
        backdropFilter: "blur(8px) saturate(150%)",
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="update-dialog-title"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !isDownloading && !isWaitingRestart) {
          onClose()
        }
      }}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        className="relative bg-[var(--bg-secondary)] rounded-lg max-w-[440px] w-[calc(100%-32px)] overflow-hidden animate-in slide-in-from-bottom-2 duration-300 outline-none"
        style={{
          boxShadow:
            "0 25px 50px -12px rgba(0, 0, 0, 0.6), 0 0 0 1px var(--glass-border)",
        }}
      >
        <div
          className="px-5 py-4 border-b border-[var(--glass-border)]"
          style={{ background: "rgba(255, 255, 255, 0.02)" }}
        >
          <h3
            id="update-dialog-title"
            className="text-[16px] font-bold text-[var(--text-primary)] m-0"
          >
            {t.softwareUpdate}
          </h3>
        </div>

        <div className="p-5 text-[var(--text-secondary)] text-[14px] leading-relaxed flex flex-col gap-3">
          <div className="grid grid-cols-2 gap-2 text-[13px]">
            <div className="text-[var(--text-secondary)] opacity-80">
              {t.updateCurrentVersion}
            </div>
            <div className="text-[var(--text-primary)] font-mono text-right">
              {current}
            </div>
            <div className="text-[var(--text-secondary)] opacity-80">
              {t.updateTargetVersion}
            </div>
            <div className="text-[var(--text-primary)] font-mono text-right">
              {targetVersion}
            </div>
            <div className="text-[var(--text-secondary)] opacity-80">
              {t.updateDownloadSize}
            </div>
            <div className="text-[var(--text-primary)] font-mono text-right">
              {formatBytes(size)}
            </div>
          </div>

          <div className="flex items-center gap-2 mt-1 text-[var(--text-primary)]">
            {(isChecking || isDownloading) && (
              <Loader2
                size={16}
                className="animate-spin shrink-0 text-[var(--accent-primary)]"
              />
            )}
            {status === "ready" && (
              <CheckCircle2 size={16} className="shrink-0 text-emerald-400" />
            )}
            {status === "error" && (
              <AlertCircle size={16} className="shrink-0 text-red-400" />
            )}
            <span className="text-[13px] font-medium">{phaseLabel}</span>
          </div>

          {(isDownloading || status === "ready") && progressText && (
            <div className="flex flex-col gap-1.5">
              <div
                className="h-1.5 rounded-full overflow-hidden bg-[var(--bg-tertiary)]"
                role="progressbar"
                aria-valuenow={percent ?? undefined}
                aria-valuemin={0}
                aria-valuemax={100}
              >
                <div
                  className="h-full rounded-full bg-[var(--accent-primary)] transition-[width] duration-200"
                  style={{
                    width:
                      percent != null
                        ? `${percent}%`
                        : isDownloading
                          ? "30%"
                          : "100%",
                    opacity: percent == null && isDownloading ? 0.5 : 1,
                  }}
                />
              </div>
              <div className="text-[12px] font-mono opacity-80">
                {progressText}
              </div>
            </div>
          )}

          {errorMessage && (
            <div className="text-[12px] text-red-400/90 bg-red-500/10 border border-red-500/20 rounded px-3 py-2 break-words">
              {errorMessage}
            </div>
          )}

          {showRestartConfirm && (
            <p className="text-[12px] opacity-80 m-0">
              {t.updateRestartConfirmHint}
            </p>
          )}

          {(status === "waitingForSafeRestart" || status === "restarting") && (
            <div className="text-[12px] opacity-90 flex flex-col gap-1.5">
              <p className="m-0">{t.updateRestartWaiting}</p>
              {drainSnapshot && drainSnapshot.total > 0 && (
                <ul className="m-0 pl-4 list-disc">
                  {drainSnapshot.categories
                    .filter((c) => c.count > 0)
                    .map((c) => (
                      <li key={c.category}>
                        {categoryLabel(c.category)}: {c.count}
                      </li>
                    ))}
                </ul>
              )}
              {restartWaitTimedOut && (
                <p className="m-0 text-amber-400/90">{t.updateRestartTimeout}</p>
              )}
            </div>
          )}
        </div>

        <div
          className="px-5 py-3 border-t border-[var(--glass-border)] flex justify-end gap-3 flex-wrap"
          style={{ background: "rgba(255, 255, 255, 0.02)" }}
        >
          {isDownloading && (
            <button
              type="button"
              className="px-4 py-2 rounded text-[13px] font-semibold cursor-pointer transition-all duration-200 border border-[var(--glass-border)] bg-transparent text-[var(--text-secondary)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
              onClick={() => void updateManagerApi.cancelDownload()}
            >
              {t.common.cancel}
            </button>
          )}

          {!isDownloading && !isWaitingRestart && (
            <button
              type="button"
              className="px-4 py-2 rounded text-[13px] font-semibold cursor-pointer transition-all duration-200 border border-[var(--glass-border)] bg-transparent text-[var(--text-secondary)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
              onClick={onClose}
            >
              {t.common.cancel}
            </button>
          )}

          {canRetryCheck && (
            <button
              type="button"
              className="px-4 py-2 rounded text-[13px] font-semibold cursor-pointer transition-all duration-200 border-none text-white hover:brightness-110 flex items-center gap-1.5"
              style={{
                background: "var(--accent-primary)",
                boxShadow: "0 4px 12px rgba(59, 130, 246, 0.2)",
              }}
              onClick={() => void updateManagerApi.check(true)}
              disabled={isChecking}
            >
              {isChecking ? (
                <Loader2 size={14} className="animate-spin" />
              ) : (
                <RefreshCw size={14} />
              )}
              {t.updateCheckNow}
            </button>
          )}

          {canDownload && (
            <button
              type="button"
              className="px-4 py-2 rounded text-[13px] font-semibold cursor-pointer transition-all duration-200 border-none text-white hover:brightness-110 flex items-center gap-1.5"
              style={{
                background: "var(--accent-primary)",
                boxShadow: "0 4px 12px rgba(59, 130, 246, 0.2)",
              }}
              onClick={() => void updateManagerApi.download()}
            >
              <Download size={14} />
              {t.updateDownload}
            </button>
          )}

          {status === "waitingForSafeRestart" && (
            <>
              <button
                type="button"
                className="px-4 py-2 rounded text-[13px] font-semibold cursor-pointer transition-all duration-200 border border-[var(--glass-border)] bg-transparent text-[var(--text-secondary)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
                onClick={() => void updateManagerApi.cancelSafeRestart()}
              >
                {t.updateRestartCancelWait}
              </button>
              {restartWaitTimedOut && (
                <button
                  type="button"
                  className="px-4 py-2 rounded text-[13px] font-semibold cursor-pointer transition-all duration-200 border-none text-white hover:brightness-110"
                  style={{
                    background: "var(--accent-primary)",
                    boxShadow: "0 4px 12px rgba(59, 130, 246, 0.2)",
                  }}
                  onClick={() => void updateManagerApi.continueRestartWait()}
                >
                  {t.updateRestartKeepWaiting}
                </button>
              )}
            </>
          )}

          {showRestartConfirm && (
            <button
              type="button"
              className="px-4 py-2 rounded text-[13px] font-semibold cursor-pointer transition-all duration-200 border-none text-white hover:brightness-110"
              style={{
                background: "var(--accent-primary)",
                boxShadow: "0 4px 12px rgba(59, 130, 246, 0.2)",
              }}
              title={t.updateRestartConfirmHint}
              onClick={() => void updateManagerApi.requestSafeRestart()}
            >
              {t.updateRestartNow}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
