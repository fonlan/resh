import React, { useEffect, useMemo, useState } from "react"
import { Check, Cloud, HardDrive, Loader2 } from "lucide-react"
import { useTranslation } from "../../i18n"
import {
  SyncConflictAttempt,
  SyncResolution,
  SyncResolutionChoice,
  TriggerSyncResult,
} from "../../types"
import { FormModal } from "../FormModal"

interface SyncConflictDialogProps {
  isOpen: boolean
  attempt: SyncConflictAttempt | null
  onClose: () => void
  onResolve: (resolutions: SyncResolution[]) => Promise<TriggerSyncResult>
}

const conflictKey = (entityType: string, id: string) => `${entityType}:${id}`

function outcomeMessage(result: TriggerSyncResult): string {
  switch (result.outcome.status) {
    case "applied":
      return ""
    case "conflicts":
      return ""
    case "concurrentRemoteChange":
      return result.outcome.message
    case "failed":
      return result.outcome.error.message
  }
}

export const SyncConflictDialog: React.FC<SyncConflictDialogProps> = ({
  isOpen,
  attempt,
  onClose,
  onResolve,
}) => {
  const { t } = useTranslation()
  const [choices, setChoices] = useState<Record<string, SyncResolutionChoice>>(
    {},
  )
  const [isSubmitting, setIsSubmitting] = useState(false)

  useEffect(() => {
    setChoices({})
  }, [attempt?.attemptToken])

  const conflicts = attempt?.conflicts ?? []
  const selectedCount = useMemo(
    () =>
      conflicts.filter((conflict) =>
        Boolean(choices[conflictKey(conflict.entityType, conflict.id)]),
      ).length,
    [choices, conflicts],
  )

  const chooseAll = (choice: SyncResolutionChoice) => {
    setChoices(
      Object.fromEntries(
        conflicts.map((conflict) => [
          conflictKey(conflict.entityType, conflict.id),
          choice,
        ]),
      ),
    )
  }

  const handleSubmit = async () => {
    if (!attempt) return
    if (selectedCount !== conflicts.length) {
      throw new Error(t.syncConflictChooseAll)
    }

    setIsSubmitting(true)
    try {
      const resolutions = conflicts.map((conflict) => ({
        entityType: conflict.entityType,
        id: conflict.id,
        choice: choices[conflictKey(conflict.entityType, conflict.id)],
        resolutionToken: conflict.resolutionToken,
      }))
      const result = await onResolve(resolutions)
      if (result.outcome.status === "applied") {
        onClose()
        return
      }
      if (result.outcome.status === "conflicts") {
        return
      }
      throw new Error(outcomeMessage(result))
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <FormModal
      isOpen={isOpen && Boolean(attempt)}
      title={t.syncConflictsTitle}
      onClose={onClose}
      onSubmit={handleSubmit}
      isLoading={isSubmitting}
      submitText={t.syncConflictsApply}
      extraFooterContent={
        <span className="mr-auto text-xs text-[var(--text-muted)]" aria-live="polite">
          {t.syncConflictsSelected.replace("{selected}", String(selectedCount)).replace(
            "{total}",
            String(conflicts.length),
          )}
        </span>
      }
    >
      <div className="space-y-4">
        <p className="m-0 text-sm leading-6 text-[var(--text-secondary)]">
          {t.syncConflictsDescription}
        </p>

        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={() => chooseAll("keepLocal")}
            className="inline-flex items-center gap-1.5 rounded border border-zinc-700/60 bg-[var(--bg-primary)] px-3 py-1.5 text-xs font-medium text-[var(--text-primary)] transition-colors hover:border-blue-500 hover:bg-blue-500/10"
          >
            <HardDrive size={13} />
            {t.syncConflictsKeepAllLocal}
          </button>
          <button
            type="button"
            onClick={() => chooseAll("useRemote")}
            className="inline-flex items-center gap-1.5 rounded border border-zinc-700/60 bg-[var(--bg-primary)] px-3 py-1.5 text-xs font-medium text-[var(--text-primary)] transition-colors hover:border-blue-500 hover:bg-blue-500/10"
          >
            <Cloud size={13} />
            {t.syncConflictsUseAllRemote}
          </button>
        </div>

        <div className="max-h-[48vh] space-y-3 overflow-y-auto pr-1">
          {conflicts.map((conflict) => {
            const key = conflictKey(conflict.entityType, conflict.id)
            const choice = choices[key]
            return (
              <section
                key={key}
                className="rounded-md border border-[var(--glass-border)] bg-[var(--bg-primary)] p-3"
                aria-label={`${conflict.entityType}: ${conflict.displayName}`}
              >
                <div className="mb-3 flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <p className="m-0 truncate text-sm font-semibold text-[var(--text-primary)]">
                      {conflict.displayName}
                    </p>
                    <p className="mt-1 mb-0 text-xs text-[var(--text-muted)]">
                      {t.syncConflictEntity}: {conflict.entityType} · {conflict.kind}
                    </p>
                  </div>
                  {choice && <Check size={16} className="shrink-0 text-blue-400" />}
                </div>

                <div className="grid gap-2 sm:grid-cols-2">
                  <button
                    type="button"
                    onClick={() =>
                      setChoices((current) => ({ ...current, [key]: "keepLocal" }))
                    }
                    className={`rounded border p-2.5 text-left transition-colors ${
                      choice === "keepLocal"
                        ? "border-blue-500 bg-blue-500/10"
                        : "border-zinc-700/60 hover:border-zinc-500"
                    }`}
                  >
                    <span className="mb-1 flex items-center gap-1.5 text-xs font-semibold text-[var(--text-primary)]">
                      <HardDrive size={13} />
                      {t.syncConflictKeepLocal}
                    </span>
                    <span className="block truncate text-xs text-[var(--text-secondary)]">
                      {conflict.local.displayName}
                    </span>
                    <span className="mt-1 block text-xs text-[var(--text-muted)]">
                      {conflict.local.present
                        ? conflict.local.details
                        : t.syncConflictDeleted}
                    </span>
                  </button>

                  <button
                    type="button"
                    onClick={() =>
                      setChoices((current) => ({ ...current, [key]: "useRemote" }))
                    }
                    className={`rounded border p-2.5 text-left transition-colors ${
                      choice === "useRemote"
                        ? "border-blue-500 bg-blue-500/10"
                        : "border-zinc-700/60 hover:border-zinc-500"
                    }`}
                  >
                    <span className="mb-1 flex items-center gap-1.5 text-xs font-semibold text-[var(--text-primary)]">
                      <Cloud size={13} />
                      {t.syncConflictUseRemote}
                    </span>
                    <span className="block truncate text-xs text-[var(--text-secondary)]">
                      {conflict.remote.displayName}
                    </span>
                    <span className="mt-1 block text-xs text-[var(--text-muted)]">
                      {conflict.remote.present
                        ? conflict.remote.details
                        : t.syncConflictDeleted}
                    </span>
                  </button>
                </div>
              </section>
            )
          })}
        </div>

        {isSubmitting && (
          <p className="m-0 flex items-center gap-2 text-xs text-[var(--text-secondary)]">
            <Loader2 size={14} className="animate-spin" />
            {t.syncing}
          </p>
        )}
      </div>
    </FormModal>
  )
}
