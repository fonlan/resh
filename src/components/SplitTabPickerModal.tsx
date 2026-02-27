import React, { useEffect, useMemo, useState } from "react"
import { createPortal } from "react-dom"
import { Check } from "lucide-react"
import { useTranslation } from "../i18n"
import { EmojiText } from "./EmojiText"
import { SplitLayout } from "./SplitViewButton"

interface SplitTabOption {
  id: string
  label: string
}

interface SplitTabPickerModalProps {
  isOpen: boolean
  layout: SplitLayout | null
  tabs: SplitTabOption[]
  requiredCount: number
  initialSelectedTabIds: string[]
  onCancel: () => void
  onConfirm: (selectedTabIds: string[]) => void
}

export const SplitTabPickerModal: React.FC<SplitTabPickerModalProps> = ({
  isOpen,
  layout,
  tabs,
  requiredCount,
  initialSelectedTabIds,
  onCancel,
  onConfirm,
}) => {
  const { t } = useTranslation()
  const [selectedTabIds, setSelectedTabIds] = useState<string[]>([])

  useEffect(() => {
    if (!isOpen || !layout) {
      return
    }

    const existingIds = new Set(tabs.map((tab) => tab.id))
    const nextSelected = initialSelectedTabIds
      .filter((id) => existingIds.has(id))
      .slice(0, requiredCount)

    if (nextSelected.length < requiredCount) {
      for (const tab of tabs) {
        if (nextSelected.length >= requiredCount) {
          break
        }
        if (!nextSelected.includes(tab.id)) {
          nextSelected.push(tab.id)
        }
      }
    }

    setSelectedTabIds(nextSelected)
  }, [isOpen, layout, tabs, requiredCount, initialSelectedTabIds])

  useEffect(() => {
    if (!isOpen) {
      return
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onCancel()
      }
    }

    window.addEventListener("keydown", handleKeyDown)
    return () => {
      window.removeEventListener("keydown", handleKeyDown)
    }
  }, [isOpen, onCancel])

  const layoutLabel = useMemo(() => {
    if (layout === "horizontal") return t.mainWindow.splitLeftRight
    if (layout === "vertical") return t.mainWindow.splitTopBottom
    if (layout === "grid") return t.mainWindow.splitFourGrid
    return ""
  }, [
    layout,
    t.mainWindow.splitLeftRight,
    t.mainWindow.splitTopBottom,
    t.mainWindow.splitFourGrid,
  ])

  const isSelectionComplete = selectedTabIds.length === requiredCount

  const handleToggleTab = (tabId: string) => {
    setSelectedTabIds((prev) => {
      if (prev.includes(tabId)) {
        return prev.filter((id) => id !== tabId)
      }

      if (prev.length >= requiredCount) {
        return prev
      }

      return [...prev, tabId]
    })
  }

  if (!isOpen || !layout) {
    return null
  }

  return createPortal(
    <div
      className="fixed inset-0 z-[1200] bg-black/55 backdrop-blur-[1px] flex items-center justify-center p-4"
      onMouseDown={onCancel}
    >
      <div
        className="w-full max-w-[520px] bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded-[var(--radius-lg)] shadow-[0_25px_50px_rgba(0,0,0,0.45)] overflow-hidden"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="px-5 py-4 border-b border-[var(--glass-border)] bg-[rgba(255,255,255,0.02)]">
          <h3 className="m-0 text-[16px] font-semibold text-[var(--text-primary)]">
            {t.mainWindow.splitSelectTabsTitle}
          </h3>
          <p className="m-0 mt-1 text-[13px] text-[var(--text-secondary)]">
            {t.mainWindow.splitSelectTabsHint
              .replace("{layout}", layoutLabel)
              .replace("{count}", requiredCount.toString())}
          </p>
        </div>

        <div className="p-4 max-h-[380px] overflow-y-auto flex flex-col gap-2">
          {tabs.map((tab) => {
            const selected = selectedTabIds.includes(tab.id)
            const disableUnchecked =
              !selected && selectedTabIds.length >= requiredCount

            return (
              <button
                key={tab.id}
                type="button"
                disabled={disableUnchecked}
                onClick={() => handleToggleTab(tab.id)}
                className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-[var(--radius-sm)] border text-left transition-all ${selected ? "border-[var(--accent-primary)] bg-[rgba(59,130,246,0.14)] text-[var(--text-primary)]" : "border-[var(--glass-border)] bg-[var(--bg-primary)] text-[var(--text-secondary)] hover:border-[var(--accent-primary)] hover:text-[var(--text-primary)]"} ${disableUnchecked ? "opacity-50 cursor-not-allowed" : "cursor-pointer"}`}
              >
                <span
                  className={`w-[18px] h-[18px] rounded-[5px] border flex items-center justify-center ${selected ? "border-[var(--accent-primary)] bg-[var(--accent-primary)] text-white" : "border-[var(--glass-border)] bg-[var(--bg-secondary)] text-transparent"}`}
                >
                  <Check size={12} />
                </span>
                <span className="min-w-0 flex-1 text-[13px] font-medium whitespace-nowrap overflow-hidden text-ellipsis">
                  <EmojiText text={tab.label} />
                </span>
              </button>
            )
          })}
        </div>

        <div className="px-5 py-3 border-t border-[var(--glass-border)] bg-[var(--bg-tertiary)] flex items-center justify-between gap-3">
          <span className="text-[12px] text-[var(--text-muted)]">
            {t.mainWindow.splitSelectedCount
              .replace("{selected}", selectedTabIds.length.toString())
              .replace("{required}", requiredCount.toString())}
          </span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              className="px-3 py-1.5 rounded-[var(--radius-sm)] border border-[var(--glass-border)] bg-transparent text-[var(--text-secondary)] text-[13px] cursor-pointer hover:bg-[var(--bg-primary)]"
              onClick={onCancel}
            >
              {t.common.cancel}
            </button>
            <button
              type="button"
              disabled={!isSelectionComplete}
              className="px-3 py-1.5 rounded-[var(--radius-sm)] border-0 bg-[var(--accent-primary)] text-white text-[13px] cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
              onClick={() => onConfirm(selectedTabIds)}
            >
              {t.common.apply}
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  )
}
