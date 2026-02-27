import React, { useActionState } from "react"
import { createPortal, useFormStatus } from "react-dom"
import { useTranslation } from "../i18n"

interface FormModalProps {
  isOpen: boolean
  title: string
  children: React.ReactNode
  onSubmit: () => void | Promise<void>
  onClose: () => void
  isLoading?: boolean
  submitText?: string
  extraFooterContent?: React.ReactNode
  noPadding?: boolean
}

interface SubmitButtonProps {
  submitText: string
  isLoading: boolean
}

const SubmitButton: React.FC<SubmitButtonProps> = ({
  submitText,
  isLoading,
}) => {
  const { pending } = useFormStatus()
  const disabled = pending || isLoading

  return (
    <button
      type="submit"
      disabled={disabled}
      className="px-5 py-2 rounded bg-[var(--accent-primary)] text-white text-[13px] font-medium border-none cursor-pointer transition-all duration-200 flex items-center gap-2 hover:-translate-y-0.5 hover:brightness-110 disabled:opacity-50 disabled:cursor-not-allowed disabled:shadow-none"
      style={{ boxShadow: "0 4px 12px rgba(59, 130, 246, 0.3)" }}
    >
      {disabled && <span className="inline-block animate-spin">⏳</span>}
      {submitText}
    </button>
  )
}

interface CancelButtonProps {
  onClose: () => void
  isLoading: boolean
  label: string
}

const CancelButton: React.FC<CancelButtonProps> = ({
  onClose,
  isLoading,
  label,
}) => {
  const { pending } = useFormStatus()

  return (
    <button
      type="button"
      onClick={onClose}
      disabled={pending || isLoading}
      className="px-4 py-2 rounded border border-[var(--glass-border)] bg-transparent text-[var(--text-secondary)] text-[13px] font-medium cursor-pointer transition-all duration-200 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] hover:border-[var(--text-muted)] disabled:opacity-50 disabled:cursor-not-allowed"
    >
      {label}
    </button>
  )
}

export const FormModal: React.FC<FormModalProps> = ({
  isOpen,
  title,
  children,
  onSubmit,
  onClose,
  isLoading = false,
  submitText,
  extraFooterContent,
  noPadding = false,
}) => {
  const { t } = useTranslation()
  const stopParentOverlayClose = (e: React.MouseEvent) => {
    e.stopPropagation()
  }

  const [submitError, submitAction] = useActionState<string | null, FormData>(
    async (_previous, _formData) => {
      try {
        await onSubmit()
        return null
      } catch (error) {
        return error instanceof Error ? error.message : t.saveStatus.error
      }
    },
    null,
  )

  if (!isOpen) {
    return null
  }

  const effectiveSubmitText = submitText || t.common.save
  const modalContent = (
    <div
      className="fixed inset-0 flex items-center justify-center z-[1100] animate-in fade-in duration-300"
      style={{
        background: "rgba(2, 6, 23, 0.6)",
        backdropFilter: "blur(10px) saturate(150%)",
      }}
      onMouseDown={stopParentOverlayClose}
      onMouseUp={stopParentOverlayClose}
      onClick={stopParentOverlayClose}
    >
      <div
        className="relative bg-[var(--bg-secondary)] rounded-lg max-w-[700px] w-[calc(100%-32px)] overflow-hidden animate-in slide-in-from-bottom-2 duration-400"
        style={{
          boxShadow:
            "0 25px 50px -12px rgba(0, 0, 0, 0.6), 0 0 0 1px var(--glass-border), inset 0 1px 1px rgba(255, 255, 255, 0.05)",
        }}
      >
        <div
          className="absolute top-0 left-0 right-0 h-0.5 opacity-50"
          style={{
            background:
              "linear-gradient(90deg, transparent, var(--accent-primary), var(--accent-secondary), transparent)",
          }}
        />

        <div
          className="px-6 py-4 border-b border-[var(--glass-border)]"
          style={{ background: "rgba(255, 255, 255, 0.02)" }}
        >
          <h2 className="text-[18px] font-bold text-[var(--text-primary)] m-0">
            {title}
          </h2>
        </div>

        <form action={submitAction} className="contents">
          <div
            className={`${!noPadding ? "p-6 overflow-y-auto" : ""} max-h-[70vh] overflow-hidden flex flex-col`}
          >
            {children}
          </div>

          <div
            className="px-6 py-4 border-t border-[var(--glass-border)] flex items-center justify-end gap-3"
            style={{ background: "rgba(255, 255, 255, 0.02)" }}
          >
            {submitError && (
              <p className="mr-auto text-[12px] text-[var(--color-danger)] mb-0">
                {submitError}
              </p>
            )}
            {extraFooterContent}
            <div className="flex gap-3">
              <CancelButton
                onClose={onClose}
                isLoading={isLoading}
                label={t.common.cancel}
              />
              <SubmitButton
                submitText={effectiveSubmitText}
                isLoading={isLoading}
              />
            </div>
          </div>
        </form>
      </div>
    </div>
  )

  if (typeof document === "undefined") {
    return modalContent
  }

  return createPortal(modalContent, document.body)
}
