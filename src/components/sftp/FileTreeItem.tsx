import React, { useCallback, useMemo, useRef, useState } from "react"
import {
  ChevronDown,
  ChevronRight,
  File,
  FileSymlink,
  Folder,
  FolderOpen,
  FolderSymlink,
  RefreshCw,
} from "lucide-react"
import type { FileEntry } from "./types"
import { formatPermissions } from "./helpers"
import { useIncrementalRenderCount } from "./useIncrementalRenderCount"

interface FileTreeItemProps {
  entry: FileEntry
  depth: number
  onToggle: (entry: FileEntry) => void
  onOpen: (entry: FileEntry) => void
  onContextMenu: (e: React.MouseEvent, entry: FileEntry) => void
  onDragStart: (e: React.DragEvent<HTMLButtonElement>, entry: FileEntry) => void
  clipboardSourcePath?: string
  clipboardIsCut?: boolean
}

export const FileTreeItem: React.FC<FileTreeItemProps> = ({
  entry,
  depth,
  onToggle,
  onOpen,
  onContextMenu,
  onDragStart,
  clipboardSourcePath,
  clipboardIsCut,
}) => {
  const isInClipboard = clipboardSourcePath === entry.path
  const clipboardTextClass = isInClipboard
    ? clipboardIsCut
      ? "line-through opacity-60"
      : "italic opacity-60"
    : ""
  const rowRef = useRef<HTMLButtonElement>(null)
  const nameRef = useRef<HTMLSpanElement>(null)
  const [showFullNameTooltip, setShowFullNameTooltip] = useState(false)
  const totalChildren = entry.children?.length || 0
  const visibleChildrenCount = useIncrementalRenderCount(
    totalChildren,
    Boolean(entry.isExpanded) && totalChildren > 0,
  )
  const visibleChildren = useMemo(() => {
    if (!entry.children) {
      return []
    }
    if (visibleChildrenCount >= entry.children.length) {
      return entry.children
    }
    return entry.children.slice(0, visibleChildrenCount)
  }, [entry.children, visibleChildrenCount])

  const updateTooltipVisibility = useCallback(() => {
    const rowElement = rowRef.current
    const nameElement = nameRef.current
    if (!rowElement || !nameElement) {
      setShowFullNameTooltip(false)
      return
    }

    const treeContainer = rowElement.closest("[data-sftp-tree-scroll]")
    if (!(treeContainer instanceof HTMLElement)) {
      setShowFullNameTooltip(nameElement.scrollWidth > nameElement.clientWidth)
      return
    }

    const containerRect = treeContainer.getBoundingClientRect()
    const nameRect = nameElement.getBoundingClientRect()
    const isPartiallyHidden =
      nameRect.left < containerRect.left || nameRect.right > containerRect.right
    const isTextOverflowed = nameElement.scrollWidth > nameElement.clientWidth
    setShowFullNameTooltip(isPartiallyHidden || isTextOverflowed)
  }, [])

  return (
    <div>
      <button
        ref={rowRef}
        type="button"
        draggable
        data-sftp-path={entry.path}
        className={`flex items-center gap-2 py-0.5 px-0.75 !important cursor-pointer text-[14px] leading-normal text-[var(--text-primary)] whitespace-nowrap select-none border-0 !important bg-transparent min-w-full w-max text-left hover:bg-[var(--bg-tertiary)] ${isInClipboard ? "opacity-50" : ""}`}
        onClick={() => onToggle(entry)}
        onDoubleClick={() => onOpen(entry)}
        onContextMenu={(e) => onContextMenu(e, entry)}
        onDragStart={(e) => onDragStart(e, entry)}
        onMouseEnter={updateTooltipVisibility}
        onFocus={updateTooltipVisibility}
        style={{ paddingLeft: `${depth * 12 + 4}px` }}
      >
        <div className="w-4 flex-shrink-0 flex items-center justify-center">
          {(entry.is_dir || (entry.is_symlink && entry.target_is_dir)) &&
            (entry.isLoading ? (
              <RefreshCw size={10} className="animate-spin text-gray-500" />
            ) : entry.isExpanded ? (
              <ChevronDown size={14} className="text-gray-500" />
            ) : (
              <ChevronRight size={14} className="text-gray-500" />
            ))}
        </div>

        {entry.is_symlink ? (
          entry.target_is_dir ? (
            <FolderSymlink
              size={16}
              className="text-[var(--text-muted)] flex-shrink-0 !text-amber-400 !stroke-amber-400"
            />
          ) : (
            <FileSymlink
              size={16}
              className="text-[var(--text-muted)] flex-shrink-0"
            />
          )
        ) : entry.is_dir ? (
          entry.isExpanded ? (
            <FolderOpen
              size={16}
              className="text-[var(--text-muted)] flex-shrink-0 !text-amber-400 !stroke-amber-400"
            />
          ) : (
            <Folder
              size={16}
              className="text-[var(--text-muted)] flex-shrink-0 !text-amber-400 !stroke-amber-400"
            />
          )
        ) : (
          <File size={16} className="text-[var(--text-muted)] flex-shrink-0" />
        )}

        <span
          ref={nameRef}
          className={`ml-0.25 flex-1 whitespace-nowrap ${clipboardTextClass}`}
          title={showFullNameTooltip ? entry.name : undefined}
        >
          {entry.name}
          {entry.link_target && (
            <span className="text-[var(--text-muted)] opacity-60 ml-2 text-[12px]">
              → {entry.link_target}
            </span>
          )}
        </span>

        {entry.permissions !== undefined && (
          <span className="ml-auto text-[10px] text-[var(--text-muted)] opacity-65 font-mono pr-1 flex-shrink-0">
            {formatPermissions(entry)}
          </span>
        )}
      </button>

      {entry.isExpanded && entry.children && (
        <div className="sftp-tree-children">
          {visibleChildren.map((child) => (
            <FileTreeItem
              key={child.path}
              entry={child}
              depth={depth + 1}
              onToggle={onToggle}
              onOpen={onOpen}
              onContextMenu={onContextMenu}
              onDragStart={onDragStart}
              clipboardSourcePath={clipboardSourcePath}
              clipboardIsCut={clipboardIsCut}
            />
          ))}
          {visibleChildrenCount < totalChildren && (
            <div
              className="flex items-center py-0.5 text-[var(--text-muted)]"
              style={{ paddingLeft: `${(depth + 1) * 12 + 8}px` }}
            >
              <RefreshCw size={10} className="animate-spin opacity-65" />
            </div>
          )}
        </div>
      )}
    </div>
  )
}
