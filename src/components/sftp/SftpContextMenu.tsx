import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react"
import { createPortal } from "react-dom"
import {
  ChevronRight,
  Clipboard,
  Copy,
  Download,
  Edit,
  File,
  FileCode,
  FolderPlus,
  Heart,
  Link,
  Pencil,
  Plus,
  Settings,
  Terminal,
  Trash,
  Upload,
  X,
} from "lucide-react"
import { useTranslation } from "../../i18n"
import type { SftpCustomCommand } from "../../types"
import {
  CONTEXT_MENU_VIEWPORT_PADDING,
  CONTEXT_SUBMENU_GAP,
  CONTEXT_SUBMENU_WIDTH,
  isDirectory,
} from "./helpers"
import type {
  ClipboardState,
  ContextMenuPosition,
  ContextSubmenuPosition,
  FileEntry,
} from "./types"

interface ContextMenuTrigger {
  x: number
  y: number
  entry: FileEntry | null
}

export interface SftpContextMenuProps {
  contextMenu: ContextMenuTrigger
  serverId?: string
  clipboard: ClipboardState | null
  matchedCustomCommands: SftpCustomCommand[]
  onOpenInEditor: () => void
  onDownload: () => void
  onUpload: () => void
  onNewFile: () => void
  onNewFolder: () => void
  onAddDirectoryFavorite: () => void
  onEditVim: () => void
  onEditLocal: () => void
  onCopyName: () => void
  onCopyFullPath: () => void
  onSendPath: () => void
  onTerminalJump: () => void
  onCopyForPaste: () => void
  onCut: () => void
  onPaste: () => void
  onClearClipboard: () => void
  onDelete: () => void
  onRename: () => void
  onProperties: () => void
  onExecuteCustomCommand: (cmd: SftpCustomCommand) => void
}

const ITEM_CLASS =
  "flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5"

const ITEM_MUTED_CLASS =
  "flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-muted)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"

const SUBMENU_PANEL_CLASS =
  "absolute top-[-4px] left-full ml-1 bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded shadow-[0_10px_15px_-3px_rgba(0,0,0,0.2),0_4px_6px_-2px_rgba(0,0,0,0.1)] min-w-[200px] p-1 z-[1001] overflow-visible backdrop-blur-xl animate-sftp-fade-in"

const useHoverSubmenu = (closeDelayMs = 200) => {
  const [open, setOpen] = useState(false)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const cancelClose = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
  }, [])

  const onMouseEnter = useCallback(() => {
    cancelClose()
    setOpen(true)
  }, [cancelClose])

  const onMouseLeave = useCallback(() => {
    cancelClose()
    timerRef.current = setTimeout(() => {
      setOpen(false)
    }, closeDelayMs)
  }, [cancelClose, closeDelayMs])

  useEffect(() => () => cancelClose(), [cancelClose])

  return {
    open,
    setOpen,
    onMouseEnter,
    onMouseLeave,
    cancelClose,
  }
}

export const SftpContextMenu: React.FC<SftpContextMenuProps> = ({
  contextMenu,
  serverId,
  clipboard,
  matchedCustomCommands,
  onOpenInEditor,
  onDownload,
  onUpload,
  onNewFile,
  onNewFolder,
  onAddDirectoryFavorite,
  onEditVim,
  onEditLocal,
  onCopyName,
  onCopyFullPath,
  onSendPath,
  onTerminalJump,
  onCopyForPaste,
  onCut,
  onPaste,
  onClearClipboard,
  onDelete,
  onRename,
  onProperties,
  onExecuteCustomCommand,
}) => {
  const { t } = useTranslation()
  const menuRef = useRef<HTMLDivElement>(null)
  const customCommandsTriggerRef = useRef<HTMLDivElement>(null)

  const editSubmenu = useHoverSubmenu()
  const pathSubmenu = useHoverSubmenu()
  const customCommandsSubmenu = useHoverSubmenu()

  const [position, setPosition] = useState<ContextMenuPosition | null>(null)
  const [submenuPosition, setSubmenuPosition] =
    useState<ContextSubmenuPosition | null>(null)

  const updateMenuPosition = useCallback(() => {
    const node = menuRef.current
    if (!node) return

    const menuRect = node.getBoundingClientRect()
    const viewportTop = CONTEXT_MENU_VIEWPORT_PADDING
    const viewportLeft = CONTEXT_MENU_VIEWPORT_PADDING
    const viewportBottom = window.innerHeight - CONTEXT_MENU_VIEWPORT_PADDING
    const viewportRight = window.innerWidth - CONTEXT_MENU_VIEWPORT_PADDING
    const menuHeight = menuRect.height
    const menuWidth = menuRect.width
    const canOpenDown = contextMenu.y + menuHeight <= viewportBottom
    const canOpenUp = contextMenu.y - menuHeight >= viewportTop

    let top = contextMenu.y
    if (!canOpenDown && canOpenUp) {
      top = contextMenu.y - menuHeight
    } else if (!canOpenDown) {
      top = viewportBottom - menuHeight
    }
    top = Math.min(
      Math.max(top, viewportTop),
      Math.max(viewportTop, viewportBottom - menuHeight),
    )

    const left = Math.min(
      Math.max(contextMenu.x, viewportLeft),
      Math.max(viewportLeft, viewportRight - menuWidth),
    )

    setPosition({ top, left })
  }, [contextMenu.x, contextMenu.y])

  useLayoutEffect(() => {
    updateMenuPosition()
  }, [updateMenuPosition])

  useEffect(() => {
    window.addEventListener("resize", updateMenuPosition)
    return () => window.removeEventListener("resize", updateMenuPosition)
  }, [updateMenuPosition])

  const updateSubmenuPosition = useCallback(() => {
    const trigger = customCommandsTriggerRef.current
    if (!trigger) return

    const triggerRect = trigger.getBoundingClientRect()
    const preferredHeight = Math.min(
      320,
      Math.max(120, matchedCustomCommands.length * 36 + 12),
    )
    const top = Math.min(
      Math.max(triggerRect.top - 4, CONTEXT_MENU_VIEWPORT_PADDING),
      Math.max(
        CONTEXT_MENU_VIEWPORT_PADDING,
        window.innerHeight - preferredHeight - CONTEXT_MENU_VIEWPORT_PADDING,
      ),
    )

    const rightSideLeft = triggerRect.right + CONTEXT_SUBMENU_GAP
    const left =
      rightSideLeft + CONTEXT_SUBMENU_WIDTH <=
      window.innerWidth - CONTEXT_MENU_VIEWPORT_PADDING
        ? rightSideLeft
        : Math.max(
            CONTEXT_MENU_VIEWPORT_PADDING,
            triggerRect.left - CONTEXT_SUBMENU_WIDTH - CONTEXT_SUBMENU_GAP,
          )
    const maxHeight = Math.max(
      120,
      window.innerHeight - top - CONTEXT_MENU_VIEWPORT_PADDING,
    )

    setSubmenuPosition({ top, left, maxHeight })
  }, [matchedCustomCommands.length])

  // Recompute submenu position whenever it opens / on resize / on scroll.
  useEffect(() => {
    if (!customCommandsSubmenu.open) {
      setSubmenuPosition(null)
      return
    }
    updateSubmenuPosition()
    window.addEventListener("resize", updateSubmenuPosition)
    window.addEventListener("scroll", updateSubmenuPosition, true)
    return () => {
      window.removeEventListener("resize", updateSubmenuPosition)
      window.removeEventListener("scroll", updateSubmenuPosition, true)
    }
  }, [customCommandsSubmenu.open, updateSubmenuPosition])

  const entry = contextMenu.entry
  const isDir = entry ? isDirectory(entry) : false

  return (
    <div
      ref={menuRef}
      className="sftp-context-menu fixed bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded shadow-[0_10px_15px_-3px_rgba(0,0,0,0.1),0_4px_6px_-2px_rgba(0,0,0,0.05)] min-w-[180px] p-1 z-50 overflow-visible animate-sftp-slide-in backdrop-blur-xl"
      style={{
        top: position?.top ?? contextMenu.y,
        left: position?.left ?? contextMenu.x,
        visibility: position ? "visible" : "hidden",
      }}
    >
      {entry && !isDir && (
        <button type="button" onClick={onOpenInEditor} className={ITEM_CLASS}>
          <File size={14} /> {t.sftp.contextMenu.open}
        </button>
      )}
      <button type="button" onClick={onDownload} className={ITEM_CLASS}>
        <Download size={14} /> {t.sftp.contextMenu.download}
      </button>
      <button type="button" onClick={onUpload} className={ITEM_CLASS}>
        <Upload size={14} /> {t.sftp.contextMenu.upload}
      </button>
      {entry && isDir && (
        <>
          <button type="button" onClick={onNewFile} className={ITEM_CLASS}>
            <Plus size={14} /> {t.sftp.contextMenu.newFile}
          </button>
          <button type="button" onClick={onNewFolder} className={ITEM_CLASS}>
            <FolderPlus size={14} /> {t.sftp.contextMenu.newFolder}
          </button>
          <button
            type="button"
            onClick={onAddDirectoryFavorite}
            disabled={!serverId}
            className={`${ITEM_CLASS} disabled:opacity-50 disabled:cursor-not-allowed`}
          >
            <Heart size={14} /> {t.sftp.contextMenu.addFavorite}
          </button>
        </>
      )}
      {entry && !isDir && (
        <div className="relative">
          <div
            onMouseEnter={editSubmenu.onMouseEnter}
            onMouseLeave={editSubmenu.onMouseLeave}
          >
            <button
              type="button"
              onClick={() => editSubmenu.setOpen(!editSubmenu.open)}
              className={ITEM_CLASS}
            >
              <Edit size={14} /> {t.sftp.contextMenu.edit}
              <ChevronRight
                size={14}
                style={{ marginLeft: "auto", opacity: 0.5 }}
              />
            </button>
            {editSubmenu.open && (
              <div className={SUBMENU_PANEL_CLASS}>
                <button
                  type="button"
                  onClick={onEditVim}
                  className={ITEM_CLASS}
                >
                  <Terminal size={14} /> {t.sftp.contextMenu.editServerVim}
                </button>
                <button
                  type="button"
                  onClick={onEditLocal}
                  className={ITEM_CLASS}
                >
                  <FileCode size={14} /> {t.sftp.contextMenu.editLocal}
                </button>
              </div>
            )}
          </div>
        </div>
      )}
      <div className="relative">
        <div
          onMouseEnter={pathSubmenu.onMouseEnter}
          onMouseLeave={pathSubmenu.onMouseLeave}
        >
          <button
            type="button"
            onClick={() => pathSubmenu.setOpen(!pathSubmenu.open)}
            className={ITEM_CLASS}
          >
            <Link size={14} /> {t.sftp.contextMenu.path}
            <ChevronRight
              size={14}
              style={{ marginLeft: "auto", opacity: 0.5 }}
            />
          </button>
          {pathSubmenu.open && entry && (
            <div className={SUBMENU_PANEL_CLASS}>
              <button
                type="button"
                onClick={onCopyName}
                className={ITEM_CLASS}
              >
                <Copy size={14} />{" "}
                {isDir
                  ? t.sftp.contextMenu.copyFolderName
                  : t.sftp.contextMenu.copyFileName}
              </button>
              <button
                type="button"
                onClick={onCopyFullPath}
                className={ITEM_CLASS}
              >
                <Copy size={14} /> {t.sftp.contextMenu.copyFullPath}
              </button>
              <button
                type="button"
                onClick={onSendPath}
                className={ITEM_CLASS}
              >
                <Terminal size={14} /> {t.sftp.contextMenu.sendPathToTerminal}
              </button>
              {isDir && (
                <button
                  type="button"
                  onClick={onTerminalJump}
                  className={ITEM_CLASS}
                >
                  <Terminal size={14} /> {t.sftp.contextMenu.terminalJump}
                </button>
              )}
            </div>
          )}
        </div>
      </div>
      {entry && (
        <>
          <button
            type="button"
            onClick={onCopyForPaste}
            className={ITEM_CLASS}
          >
            <Copy size={14} /> {t.sftp.contextMenu.copy}
          </button>
          <button type="button" onClick={onCut} className={ITEM_CLASS}>
            <Pencil size={14} /> {t.sftp.contextMenu.cut}
          </button>
        </>
      )}
      {clipboard && (
        <>
          <button type="button" onClick={onPaste} className={ITEM_CLASS}>
            <Clipboard size={14} />{" "}
            {clipboard.isCut
              ? t.sftp.contextMenu.pasteMove
              : t.sftp.contextMenu.pasteCopy}
          </button>
          <button
            type="button"
            onClick={onClearClipboard}
            className={ITEM_MUTED_CLASS}
          >
            <X size={14} /> {t.sftp.contextMenu.cancel}
          </button>
        </>
      )}
      <button type="button" onClick={onDelete} className={ITEM_CLASS}>
        <Trash size={14} /> {t.sftp.contextMenu.delete}
      </button>
      {entry && (
        <>
          <button type="button" onClick={onRename} className={ITEM_CLASS}>
            <Pencil size={14} /> {t.sftp.contextMenu.rename}
          </button>
          <button type="button" onClick={onProperties} className={ITEM_CLASS}>
            <Settings size={14} /> {t.sftp.contextMenu.properties}
          </button>
        </>
      )}
      {entry && matchedCustomCommands.length > 0 && (
        <div className="relative border-t border-[var(--glass-border)] mt-1 pt-1">
          <div
            ref={customCommandsTriggerRef}
            role="menuitem"
            onMouseEnter={customCommandsSubmenu.onMouseEnter}
            onMouseLeave={customCommandsSubmenu.onMouseLeave}
          >
            <button
              type="button"
              onClick={() => {
                if (customCommandsSubmenu.open) {
                  customCommandsSubmenu.setOpen(false)
                  return
                }
                customCommandsSubmenu.setOpen(true)
              }}
              className={ITEM_CLASS}
            >
              <Terminal size={14} /> {t.sftp.contextMenu.commands}
              <ChevronRight
                size={14}
                style={{ marginLeft: "auto", opacity: 0.5 }}
              />
            </button>
            {customCommandsSubmenu.open &&
              submenuPosition &&
              createPortal(
                <div
                  className="sftp-context-submenu fixed bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded shadow-[0_10px_15px_-3px_rgba(0,0,0,0.2),0_4px_6px_-2px_rgba(0,0,0,0.1)] p-1 z-[1001] overflow-y-auto backdrop-blur-xl animate-sftp-fade-in"
                  style={{
                    top: submenuPosition.top,
                    left: submenuPosition.left,
                    width: CONTEXT_SUBMENU_WIDTH,
                    maxHeight: submenuPosition.maxHeight,
                  }}
                  onMouseEnter={customCommandsSubmenu.onMouseEnter}
                  onMouseLeave={customCommandsSubmenu.onMouseLeave}
                >
                  {matchedCustomCommands.map((cmd) => (
                    <button
                      key={cmd.id}
                      type="button"
                      onClick={() => onExecuteCustomCommand(cmd)}
                      className={ITEM_CLASS}
                    >
                      <Terminal size={14} /> {cmd.name}
                    </button>
                  ))}
                </div>,
                document.body,
              )}
          </div>
        </div>
      )}
    </div>
  )
}
