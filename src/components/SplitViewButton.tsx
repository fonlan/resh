import React, { useState, useEffect, useRef, useCallback } from 'react'
import { createPortal } from 'react-dom'
import { Columns2, Rows2, Grid2x2, PanelsTopLeft } from 'lucide-react'
import { useTranslation } from '../i18n'

export type SplitLayout = 'horizontal' | 'vertical' | 'grid'

interface SplitViewButtonProps {
  tabCount: number
  isSplitActive: boolean
  onSelectLayout: (layout: SplitLayout) => void
  onExitSplit: () => void
}

const MIN_TABS_FOR_SPLIT = 2
const MIN_TABS_FOR_GRID = 4

export const SplitViewButton: React.FC<SplitViewButtonProps> = ({
  tabCount,
  isSplitActive,
  onSelectLayout,
  onExitSplit,
}) => {
  const { t } = useTranslation()
  const [isOpen, setIsOpen] = useState(false)
  const [menuPosition, setMenuPosition] = useState<{ top: number; left: number } | null>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  const updateMenuPosition = useCallback(() => {
    if (!buttonRef.current) {
      return
    }

    const rect = buttonRef.current.getBoundingClientRect()
    const menuWidth = 220
    const estimatedMenuHeight = 164
    const left = Math.min(Math.max(rect.right - menuWidth, 8), window.innerWidth - menuWidth - 8)
    const canOpenDownward = rect.bottom + 8 + estimatedMenuHeight <= window.innerHeight - 8
    const top = canOpenDownward
      ? rect.bottom + 8
      : Math.max(8, rect.top - estimatedMenuHeight - 8)

    setMenuPosition({ top, left })
  }, [])

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        menuRef.current &&
        !menuRef.current.contains(event.target as Node) &&
        buttonRef.current &&
        !buttonRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false)
      }
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsOpen(false)
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    document.addEventListener('keydown', handleKeyDown)

    return () => {
      document.removeEventListener('mousedown', handleClickOutside)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [])

  useEffect(() => {
    if (!isOpen) {
      setMenuPosition(null)
      return
    }

    updateMenuPosition()
    window.addEventListener('resize', updateMenuPosition)
    window.addEventListener('scroll', updateMenuPosition, true)

    return () => {
      window.removeEventListener('resize', updateMenuPosition)
      window.removeEventListener('scroll', updateMenuPosition, true)
    }
  }, [isOpen, updateMenuPosition])

  const canOpenSplit = tabCount >= MIN_TABS_FOR_SPLIT
  const canUseGrid = tabCount >= MIN_TABS_FOR_GRID
  const isButtonDisabled = !isSplitActive && !canOpenSplit

  const handleButtonClick = () => {
    if (isSplitActive) {
      setIsOpen(false)
      onExitSplit()
      return
    }

    if (isButtonDisabled) {
      return
    }

    setIsOpen(prev => {
      const next = !prev
      if (next) {
        updateMenuPosition()
      }
      return next
    })
  }

  const menuOptions = [
    {
      key: 'horizontal' as const,
      label: t.mainWindow.splitLeftRight,
      icon: <Columns2 size={14} />,
      disabled: !canOpenSplit,
    },
    {
      key: 'vertical' as const,
      label: t.mainWindow.splitTopBottom,
      icon: <Rows2 size={14} />,
      disabled: !canOpenSplit,
    },
    {
      key: 'grid' as const,
      label: t.mainWindow.splitFourGrid,
      icon: <Grid2x2 size={14} />,
      disabled: !canUseGrid,
    },
  ]

  const menu =
    isOpen && menuPosition
      ? createPortal(
          <div
            ref={menuRef}
            className="fixed min-w-[220px] bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded-[var(--radius-md)] shadow-[0_18px_35px_rgba(0,0,0,0.45),0_0_0_1px_var(--glass-border)] p-1.5 flex flex-col gap-1 z-[1000] backdrop-blur-[14px]"
            style={{ top: menuPosition.top, left: menuPosition.left }}
          >
            {menuOptions.map(option => (
              <button
                key={option.key}
                type="button"
                disabled={option.disabled}
                className="w-full flex items-center gap-2.5 px-3 py-2 rounded-[var(--radius-sm)] border-0 text-[13px] text-left bg-transparent text-[var(--text-primary)] cursor-pointer transition-all disabled:text-[var(--text-muted)] disabled:cursor-not-allowed hover:enabled:bg-[var(--bg-tertiary)]"
                onClick={() => {
                  if (option.disabled) {
                    return
                  }
                  onSelectLayout(option.key)
                  setIsOpen(false)
                }}
              >
                {option.icon}
                <span>{option.label}</span>
              </button>
            ))}
          </div>,
          document.body
        )
      : null

  return (
    <>
      <button
        type="button"
        ref={buttonRef}
        disabled={isButtonDisabled}
        className={`flex items-center justify-center w-10 h-10 border-none text-[var(--text-secondary)] transition-all ${isSplitActive ? 'bg-[var(--bg-tertiary)] text-[var(--accent-primary)] hover:text-[var(--accent-primary)] hover:bg-[var(--bg-tertiary)]' : 'bg-transparent hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]'} ${isButtonDisabled ? 'opacity-50 cursor-not-allowed hover:bg-transparent hover:text-[var(--text-secondary)]' : 'cursor-pointer'}`}
        onClick={handleButtonClick}
        aria-label={isSplitActive ? t.mainWindow.exitSplitView : t.mainWindow.splitView}
        title={isSplitActive ? t.mainWindow.exitSplitView : t.mainWindow.splitView}
      >
        <PanelsTopLeft size={18} />
      </button>
      {menu}
    </>
  )
}
