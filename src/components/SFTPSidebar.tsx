import React, { useState, useEffect, useRef, useCallback } from 'react';
import { X, Lock, LockOpen, Folder, File, ChevronRight, ChevronDown, Download, Upload, RefreshCw, FolderOpen, FolderSymlink, FileSymlink, ArrowDownUp, ArrowUp, ArrowDown, Trash, Settings, Plus, FolderPlus, Pencil, Copy, Terminal, Link, Edit, FileCode, Clipboard } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { v4 as uuidv4 } from 'uuid';
import { useTranslation } from '../i18n';
import { useConfig } from '../hooks/useConfig';
import { quotePathForTerminalInput } from '../utils/terminalUtils';

import { TransferStatusPanel } from './TransferStatusPanel';
import { ConfirmationModal } from './ConfirmationModal';
import { FormModal } from './FormModal';
import FileConflictDialog from './FileConflictDialog';
import { useTransferStore } from '../stores/transferStore';
import type { ConflictResolution, FileConflict, TransferTask, SftpCustomCommand } from '../types';

interface SFTPSidebarProps {
  isOpen: boolean;
  onClose: () => void;
  isLocked: boolean;
  onToggleLock: () => void;
  sessionId?: string;
  zIndex?: number;
}

interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_symlink?: boolean;
  target_is_dir?: boolean;
  link_target?: string;
  size: number;
  modified: number;
  permissions?: number;
  children?: FileEntry[];
  isExpanded?: boolean;
  isLoading?: boolean;
}

interface DirectoryListResult {
  path: string;
  files: FileEntry[];
  error: string | null;
}

const SFTP_PATH_MIME_TYPE = 'application/x-resh-sftp-path';
const SFTP_ENTRY_MIME_TYPE = 'application/x-resh-sftp-entry';
const COPY_DATA_UNSUPPORTED_ERROR = 'SFTP_COPY_DATA_UNSUPPORTED';

interface CopyFallbackModalState {
  isOpen: boolean;
  sessionId: string;
  sourcePath: string;
  destPath: string;
  targetPath: string;
}

const formatPermissions = (entry: FileEntry): string => {
  const mode = entry.permissions;
  if (mode === undefined) return '';
  
  let type = '-';
  if (entry.is_dir) type = 'd';
  else if (entry.is_symlink) type = 'l';
  
  const r = (m: number) => (m & 4 ? 'r' : '-');
  const w = (m: number) => (m & 2 ? 'w' : '-');
  const x = (m: number) => (m & 1 ? 'x' : '-');
  const part = (m: number) => r(m) + w(m) + x(m);
  
  return type + part((mode >> 6) & 7) + part((mode >> 3) & 7) + part(mode & 7);
};

const FileTreeItem: React.FC<{
  entry: FileEntry;
  depth: number;
  onToggle: (entry: FileEntry) => void;
  onContextMenu: (e: React.MouseEvent, entry: FileEntry) => void;
  onDragStart: (e: React.DragEvent<HTMLButtonElement>, entry: FileEntry) => void;
  clipboardSourcePath?: string;
  clipboardIsCut?: boolean;
}> = ({ entry, depth, onToggle, onContextMenu, onDragStart, clipboardSourcePath, clipboardIsCut }) => {
  const isInClipboard = clipboardSourcePath === entry.path;
  const clipboardTextClass = isInClipboard
    ? (clipboardIsCut ? 'line-through opacity-60' : 'italic opacity-60')
    : '';
  const rowRef = useRef<HTMLButtonElement>(null);
  const nameRef = useRef<HTMLSpanElement>(null);
  const [showFullNameTooltip, setShowFullNameTooltip] = useState(false);

  const updateTooltipVisibility = useCallback(() => {
    const rowElement = rowRef.current;
    const nameElement = nameRef.current;
    if (!rowElement || !nameElement) {
      setShowFullNameTooltip(false);
      return;
    }

    const treeContainer = rowElement.closest('[data-sftp-tree-scroll]');
    if (!(treeContainer instanceof HTMLElement)) {
      setShowFullNameTooltip(nameElement.scrollWidth > nameElement.clientWidth);
      return;
    }

    const containerRect = treeContainer.getBoundingClientRect();
    const nameRect = nameElement.getBoundingClientRect();
    const isPartiallyHidden = nameRect.left < containerRect.left || nameRect.right > containerRect.right;
    const isTextOverflowed = nameElement.scrollWidth > nameElement.clientWidth;
    setShowFullNameTooltip(isPartiallyHidden || isTextOverflowed);
  }, []);

  return (
    <div>
      <button
        ref={rowRef}
        type="button"
        draggable
        className={`flex items-center gap-2 py-0.5 px-0.75 !important cursor-pointer text-[14px] leading-normal text-[var(--text-primary)] whitespace-nowrap select-none border-0 !important bg-transparent min-w-full w-max text-left hover:bg-[var(--bg-tertiary)] ${isInClipboard ? 'opacity-50' : ''}`}
        onClick={() => onToggle(entry)}
        onContextMenu={(e) => onContextMenu(e, entry)}
        onDragStart={(e) => onDragStart(e, entry)}
        onMouseEnter={updateTooltipVisibility}
        onFocus={updateTooltipVisibility}
        style={{ paddingLeft: `${depth * 12 + 4}px` }}
      >
        <div className="w-4 flex-shrink-0 flex items-center justify-center">
           {(entry.is_dir || (entry.is_symlink && entry.target_is_dir)) && (
             entry.isLoading ? (
               <RefreshCw size={10} className="animate-spin text-gray-500" />
             ) : entry.isExpanded ? (
               <ChevronDown size={14} className="text-gray-500" />
             ) : (
               <ChevronRight size={14} className="text-gray-500" />
             )
           )}
        </div>

        {entry.is_symlink ? (
          entry.target_is_dir ? (
            <FolderSymlink size={16} className="text-[var(--text-muted)] flex-shrink-0 !text-amber-400 !stroke-amber-400" />
          ) : (
            <FileSymlink size={16} className="text-[var(--text-muted)] flex-shrink-0" />
          )
        ) : entry.is_dir ? (
          entry.isExpanded ? (
            <FolderOpen size={16} className="text-[var(--text-muted)] flex-shrink-0 !text-amber-400 !stroke-amber-400" />
          ) : (
            <Folder size={16} className="text-[var(--text-muted)] flex-shrink-0 !text-amber-400 !stroke-amber-400" />
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
          {entry.children.map((child) => (
            <FileTreeItem
              key={child.path}
              entry={child}
              depth={depth + 1}
              onToggle={onToggle}
              onContextMenu={onContextMenu}
              onDragStart={onDragStart}
              clipboardSourcePath={clipboardSourcePath}
              clipboardIsCut={clipboardIsCut}
            />
          ))}
        </div>
      )}
    </div>
  );
};

type SortType = 'name' | 'modified';
type SortOrder = 'asc' | 'desc';

interface SortState {
  type: SortType;
  order: SortOrder;
}

const DEFAULT_SORT_STATE: SortState = { type: 'name', order: 'asc' };

interface ClipboardState {
  sourcePath: string;
  sourceName: string;
  isDir: boolean;
  isCut: boolean;
  sessionId: string;
}

interface SessionState {
  rootFiles: FileEntry[];
  currentPath: string;
  sortState: SortState;
  isLoading: boolean;
}

export const SFTPSidebar: React.FC<SFTPSidebarProps> = ({
  isOpen,
  onClose,
  isLocked,
  onToggleLock,
  sessionId,
  zIndex,
}) => {
  const { t } = useTranslation();
  const { config } = useConfig();
  const [width, setWidth] = useState(300);
  const [isResizing, setIsResizing] = useState(false);
  const sidebarRef = useRef<HTMLDivElement>(null);
  const overwriteAllRef = useRef(false);

  const conflicts = useTransferStore(state => state.conflicts);
  const removeConflict = useTransferStore(state => state.removeConflict);

  // Auto-resolve conflicts if overwrite all is active
  useEffect(() => {
    if (overwriteAllRef.current && conflicts.length > 0) {
      conflicts.forEach(conflict => {
        invoke('sftp_resolve_conflict', {
          taskId: conflict.task_id,
          resolution: 'overwrite'
        }).catch(console.error);
        removeConflict(conflict.task_id);
      });
    }
  }, [conflicts, removeConflict]);

  // Ensure width doesn't exceed 50% on window resize
  useEffect(() => {
    const handleResize = () => {
      setWidth(prev => Math.min(prev, window.innerWidth * 0.5));
    };
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  const [sessions, setSessions] = useState<Record<string, SessionState>>({});

  const currentSession = sessionId ? sessions[sessionId] : undefined;
  const rootFiles = currentSession?.rootFiles || [];
  const currentPath = currentSession?.currentPath || '/';
  const isLoading = currentSession?.isLoading || false;
  const sortState = currentSession?.sortState || DEFAULT_SORT_STATE;

  const [clipboard, setClipboard] = useState<ClipboardState | null>(null);

  const [showSortMenu, setShowSortMenu] = useState(false);

  const [contextMenu, setContextMenu] = useState<{ x: number, y: number, entry: FileEntry | null } | null>(null);

  const [deleteModal, setDeleteModal] = useState<{ isOpen: boolean, entry: FileEntry | null }>({ isOpen: false, entry: null });
  const [copyFallbackModal, setCopyFallbackModal] = useState<CopyFallbackModalState>({
    isOpen: false,
    sessionId: '',
    sourcePath: '',
    destPath: '',
    targetPath: ''
  });
  const [newFileModal, setNewFileModal] = useState<{ isOpen: boolean, parentPath: string }>({ isOpen: false, parentPath: '' });
  const [newFolderModal, setNewFolderModal] = useState<{ isOpen: boolean, parentPath: string }>({ isOpen: false, parentPath: '' });
  const [renameModal, setRenameModal] = useState<{ isOpen: boolean, entry: FileEntry | null }>({ isOpen: false, entry: null });
  const [propertiesModal, setPropertiesModal] = useState<{ isOpen: boolean, entry: FileEntry | null, permissions: string }>({ isOpen: false, entry: null, permissions: '' });

  const [newItemName, setNewItemName] = useState('');
  const [permissionInput, setPermissionInput] = useState('');

  const [showPathSubmenu, setShowPathSubmenu] = useState(false);
  const [showEditSubmenu, setShowEditSubmenu] = useState(false);
  const [showCustomCommandsSubmenu, setShowCustomCommandsSubmenu] = useState(false);
  const [editSubmenuTimeout, setEditSubmenuTimeout] = useState<ReturnType<typeof setTimeout> | null>(null);
  const [pathSubmenuTimeout, setPathSubmenuTimeout] = useState<ReturnType<typeof setTimeout> | null>(null);
  const [customCommandsSubmenuTimeout, setCustomCommandsSubmenuTimeout] = useState<ReturnType<typeof setTimeout> | null>(null);

  // Trigger terminal resize when locked state changes
  useEffect(() => {
    setTimeout(() => {
      window.dispatchEvent(new CustomEvent('resh-force-terminal-resize'));
    }, 250);
  }, [isLocked]);

  const updateSession = useCallback((sid: string, updates: Partial<SessionState> | ((prev: SessionState) => Partial<SessionState>)) => {
      setSessions(prev => {
          const current = prev[sid] || { rootFiles: [], currentPath: '/', sortState: DEFAULT_SORT_STATE, isLoading: false };
          const newValues = typeof updates === 'function' ? updates(current) : updates;
          return {
              ...prev,
              [sid]: { ...current, ...newValues }
          };
      });
  }, []);

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
        if (showSortMenu && !(e.target as Element).closest('.sftp-sort-menu') && !(e.target as Element).closest('.sftp-sort-btn')) {
            setShowSortMenu(false);
        }
    };
    window.addEventListener('click', handleClick);
    return () => window.removeEventListener('click', handleClick);
  }, [showSortMenu]);

  const getAllExpandedPaths = (nodes: FileEntry[]): Set<string> => {
    const paths = new Set<string>();
    const traverse = (list: FileEntry[]) => {
      list.forEach(node => {
        if (node.isExpanded) {
          paths.add(node.path);
          if (node.children) {
            traverse(node.children);
          }
        }
      });
    };
    traverse(nodes);
    return paths;
  };

  const loadDirectory = useCallback(async (
    path: string,
    targetSessionId?: string,
    keepExpandedPaths?: Set<string>,
    sortOverride?: SortState
  ) => {
    const sid = targetSessionId || sessionId;
    if (!sid) return;
    const requestedSort = sortOverride || DEFAULT_SORT_STATE;

    updateSession(sid, { isLoading: true });
    
    try {
      const files = await invoke<FileEntry[]>('sftp_list_dir_sorted', {
        sessionId: sid,
        path,
        sortType: requestedSort.type,
        sortOrder: requestedSort.order
      });
      
      setSessions(prev => {
          const session = prev[sid];
          const sortedFiles = files;

          // Apply expanded state if provided
          if (keepExpandedPaths) {
              sortedFiles.forEach(f => {
                  if (keepExpandedPaths.has(f.path)) {
                      f.isExpanded = true;
                      // We mark it as loading because we will trigger a reload for it shortly
                      f.isLoading = true; 
                  }
              });
          }
          
          let newRootFiles: FileEntry[];
          
          if (path === '/' || path === '.') {
            newRootFiles = sortedFiles;
          } else {
            const targetPath = path.replace(/\\/g, '/');
            const updateNode = (nodes: FileEntry[]): FileEntry[] => {
              return nodes.map(node => {
                if (node.path.replace(/\\/g, '/') === targetPath) {
                  return { ...node, children: sortedFiles, isLoading: false };
                }
                if (node.children) {
                  return { ...node, children: updateNode(node.children) };
                }
                return node;
              });
            };
            newRootFiles = updateNode(session?.rootFiles || []);
          }

          return {
              ...prev,
              [sid]: { 
                  ...session, 
                  rootFiles: newRootFiles,
                  currentPath: (path === '/' || path === '.') ? path : session?.currentPath || '/',
                  isLoading: false,
                  sortState: requestedSort
              }
          };
      });

      return files;
    } catch (error) {
      console.error('Failed to load directory:', error);
      
      if (path !== '/' && path !== '.') {
          setSessions(prev => {
              const session = prev[sid];
              if (!session) return prev;
              
              const targetPath = path.replace(/\\/g, '/');
              const resetLoading = (nodes: FileEntry[]): FileEntry[] => {
                  return nodes.map(node => {
                      if (node.path.replace(/\\/g, '/') === targetPath) {
                          return { ...node, isLoading: false };
                      }
                      if (node.children) {
                          return { ...node, children: resetLoading(node.children) };
                      }
                      return node;
                  });
              };
              
              return {
                  ...prev,
                  [sid]: { ...session, rootFiles: resetLoading(session.rootFiles) }
              };
          });
      }

      updateSession(sid, { isLoading: false });
      return [];
    }
  }, [sessionId, updateSession]);

  const refreshDirectoryTree = useCallback(async (
    sid: string,
    basePath: string,
    expandedPaths: Set<string>,
    sort: SortState
  ) => {
    // Load root/current path first so subtree updates have a stable parent.
    await loadDirectory(basePath, sid, expandedPaths, sort);

    if (expandedPaths.size === 0) {
      return;
    }

    // Ensure parent directories are refreshed before children.
    const sortedPaths = Array.from(expandedPaths).sort((a, b) => {
      return a.split(/[\\/]/).length - b.split(/[\\/]/).length;
    });
    const childPathsToLoad = sortedPaths.filter(expandedPath => expandedPath !== '/' && expandedPath !== '.' && expandedPath !== basePath);
    if (childPathsToLoad.length === 0) {
      return;
    }

    try {
      const batchResults = await invoke<DirectoryListResult[]>('sftp_list_dirs_sorted', {
        sessionId: sid,
        paths: childPathsToLoad,
        sortType: sort.type,
        sortOrder: sort.order
      });

      setSessions(prev => {
        const session = prev[sid];
        if (!session) return prev;

        const updateChildren = (nodes: FileEntry[], targetPath: string, children?: FileEntry[], resetLoadingOnly = false): FileEntry[] => {
          return nodes.map(node => {
            if (node.path.replace(/\\/g, '/') === targetPath) {
              if (resetLoadingOnly) {
                return { ...node, isLoading: false };
              }
              return { ...node, isExpanded: true, isLoading: false, children };
            }
            if (node.children) {
              return { ...node, children: updateChildren(node.children, targetPath, children, resetLoadingOnly) };
            }
            return node;
          });
        };

        let nextRootFiles = session.rootFiles;

        for (const result of batchResults) {
          const normalizedPath = result.path.replace(/\\/g, '/');
          if (result.error) {
            console.error(`Failed to refresh directory ${result.path}:`, result.error);
            nextRootFiles = updateChildren(nextRootFiles, normalizedPath, undefined, true);
            continue;
          }

          const children = result.files.map(file => {
            if (!expandedPaths.has(file.path)) {
              return file;
            }
            return { ...file, isExpanded: true, isLoading: true };
          });

          nextRootFiles = updateChildren(nextRootFiles, normalizedPath, children, false);
        }

        return {
          ...prev,
          [sid]: {
            ...session,
            rootFiles: nextRootFiles,
            sortState: sort,
            isLoading: false
          }
        };
      });
    } catch (error) {
      console.error('Failed to refresh expanded directories in batch:', error);
    }
  }, [loadDirectory]);

  const handleRefresh = async () => {
    if (!sessionId) return;
    const currentSession = sessions[sessionId];
    const expandedPaths = currentSession ? getAllExpandedPaths(currentSession.rootFiles) : new Set<string>();
    const currentSort = currentSession?.sortState || DEFAULT_SORT_STATE;
    await refreshDirectoryTree(sessionId, currentPath, expandedPaths, currentSort);
  };

  useEffect(() => {
    if (isOpen && sessionId) {
      setSessions(prev => {
          if (!prev[sessionId]) {
              return {
                  ...prev,
                  [sessionId]: { 
                      rootFiles: [], 
                      currentPath: '/', 
                      sortState: DEFAULT_SORT_STATE, 
                      isLoading: true 
                  }
              };
          }
          return prev;
      });

      const sessionState = sessions[sessionId];
      const shouldLoad = !sessionState || (sessionState.rootFiles.length === 0 && !sessionState.isLoading);
      
      if (shouldLoad) {
          const initialSort = sessionState?.sortState || DEFAULT_SORT_STATE;
          loadDirectory('/', sessionId, undefined, initialSort);
      }
    }
  }, [isOpen, sessionId, loadDirectory, sessions]);


  const handleToggle = async (entry: FileEntry) => {
    // Allow toggling if it's a directory OR a symlink to a directory
    if (!entry.is_dir && !(entry.is_symlink && entry.target_is_dir)) return;
    if (!sessionId) return;

    if (entry.isExpanded) {
        // Collapse
        updateSession(sessionId, (prev) => {
            const toggleNode = (nodes: FileEntry[]): FileEntry[] => {
                return nodes.map(node => {
                    if (node.path === entry.path) {
                        return { ...node, isExpanded: false };
                    }
                    if (node.children) {
                        return { ...node, children: toggleNode(node.children) };
                    }
                    return node;
                });
            };
            return { rootFiles: toggleNode(prev.rootFiles) };
        });
    } else {
        // Expand
        // Set loading state for this node
        updateSession(sessionId, (prev) => {
             const setLoading = (nodes: FileEntry[], loading: boolean): FileEntry[] => {
                 return nodes.map(node => {
                    if (node.path === entry.path) {
                        return { ...node, isLoading: loading };
                    }
                    if (node.children) {
                        return { ...node, children: setLoading(node.children, loading) };
                    }
                    return node;
                });
            };
            return { rootFiles: setLoading(prev.rootFiles, true) };
        });

        try {
            const children = await invoke<FileEntry[]>('sftp_list_dir_sorted', {
              sessionId,
              path: entry.path,
              sortType: sortState.type,
              sortOrder: sortState.order
            });
            
            setSessions(prev => {
                const session = prev[sessionId];
                if (!session) return prev;

                const updateChildren = (nodes: FileEntry[]): FileEntry[] => {
                    return nodes.map(node => {
                        if (node.path === entry.path) {
                            return { ...node, isExpanded: true, isLoading: false, children };
                        }
                        if (node.children) {
                            return { ...node, children: updateChildren(node.children) };
                        }
                        return node;
                    });
                };
                
                return {
                    ...prev,
                    [sessionId]: { ...session, rootFiles: updateChildren(session.rootFiles) }
                };
            });
        } catch (error) {
             console.error('Failed to load children:', error);
             updateSession(sessionId, (prev) => {
                 const setLoading = (nodes: FileEntry[], loading: boolean): FileEntry[] => {
                     return nodes.map(node => {
                        if (node.path === entry.path) {
                            return { ...node, isLoading: loading }; // Should probably set false
                        }
                        if (node.children) {
                            return { ...node, children: setLoading(node.children, loading) };
                        }
                        return node;
                    });
                };
                return { rootFiles: setLoading(prev.rootFiles, false) };
             });
        }
    }
  };

  const handleSort = (type: SortType) => {
      if (!sessionId) return;
      const session = sessions[sessionId];
      if (!session) return;

      const newOrder: SortOrder = session.sortState.type === type && session.sortState.order === 'asc' ? 'desc' : 'asc';
      const newSortState: SortState = { type, order: newOrder };
      const expandedPaths = getAllExpandedPaths(session.rootFiles);

      setSessions(prev => ({
        ...prev,
        [sessionId]: {
          ...session,
          ...prev[sessionId],
          sortState: newSortState
        }
      }));

      void refreshDirectoryTree(sessionId, currentPath, expandedPaths, newSortState);
      setShowSortMenu(false);
  };

  const handleContextMenu = (e: React.MouseEvent, entry: FileEntry) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, entry });
  };

  const handleTreeItemDragStart = useCallback((e: React.DragEvent<HTMLButtonElement>, entry: FileEntry) => {
    e.dataTransfer.effectAllowed = 'copy';
    e.dataTransfer.setData(SFTP_PATH_MIME_TYPE, entry.path);
    e.dataTransfer.setData(
      SFTP_ENTRY_MIME_TYPE,
      JSON.stringify({
        path: entry.path,
        isDir: entry.is_dir || Boolean(entry.is_symlink && entry.target_is_dir === true)
      })
    );
    e.dataTransfer.setData('text/plain', entry.path);
  }, []);

  const handleCloseContextMenu = useCallback(() => {
    setContextMenu(null);
    setShowPathSubmenu(false);
    setShowEditSubmenu(false);
    setShowCustomCommandsSubmenu(false);
  }, []);

  const handleEditVim = () => {
    if (!contextMenu || !sessionId || !contextMenu.entry) return;
    const path = contextMenu.entry.path.replace(/"/g, '\\"');
    const command = `vim "${path}"\r`;
    window.dispatchEvent(new CustomEvent('paste-snippet', { detail: command }));
    handleCloseContextMenu();
  };

  const matchPattern = (filename: string, pattern: string) => {
      const regex = new RegExp('^' + pattern.replace(/\./g, '\\.').replace(/\*/g, '.*') + '$');
      return regex.test(filename);
  };

  const matchCustomCommand = (entry: FileEntry, cmd: SftpCustomCommand) => {
      if (cmd.pattern === '*/') return entry.is_dir;
      if (cmd.pattern.endsWith('/')) {
           return entry.is_dir && matchPattern(entry.name, cmd.pattern.slice(0, -1)); 
      }
      return matchPattern(entry.name, cmd.pattern);
  };

  const handleExecuteCustomCommand = (cmd: SftpCustomCommand) => {
      if (!contextMenu || !contextMenu.entry) return;
      const entry = contextMenu.entry;
      let command = cmd.command;
      
      command = command.replace(/{fpath}/g, entry.path);
      
      const parentPath = getParentPath(entry.path);
      command = command.replace(/{dpath}/g, parentPath);

      const lastDotIndex = entry.name.lastIndexOf('.');
      let fname = entry.name;
      if (lastDotIndex > 0) {
          fname = entry.name.substring(0, lastDotIndex);
      }
      command = command.replace(/{fname}/g, fname);

      window.dispatchEvent(new CustomEvent('paste-snippet', { detail: command + '\r' }));
      handleCloseContextMenu();
  };

  const handleEditLocal = async () => {
    if (!contextMenu || !sessionId || !config || !contextMenu.entry) return;
    const entry = contextMenu.entry;
    const entryName = entry.name;
    const remotePath = entry.path;
    handleCloseContextMenu();

    try {
        console.log('[handleEditLocal] Starting edit', remotePath);
        const localPath = await invoke<string>('sftp_edit_file', {
            sessionId,
            remotePath
        });
        console.log('[handleEditLocal] Downloaded to', localPath);

        let editorCmd = undefined;
        if (config.general.sftp?.editors) {
            const rule = config.general.sftp.editors.find(r => matchPattern(entryName, r.pattern));
            if (rule) {
                editorCmd = rule.editor;
                console.log('[handleEditLocal] Found custom editor rule', rule);
            }
        }

        console.log('[handleEditLocal] Invoking open_local_editor', { path: localPath, editor: editorCmd });
        await invoke('open_local_editor', { path: localPath, editor: editorCmd });
        console.log('[handleEditLocal] Editor opened successfully');
    } catch (e) {
        console.error('[handleEditLocal] Edit failed', e);
    }
  };

  useEffect(() => {
    const handleClick = () => handleCloseContextMenu();
    window.addEventListener('click', handleClick);
    return () => window.removeEventListener('click', handleClick);
  }, [handleCloseContextMenu]);

  const startResizing = (e: React.MouseEvent) => {
    e.preventDefault();
    setIsResizing(true);
  };

  useEffect(() => {
    const stopResizing = () => setIsResizing(false);
    const resize = (e: MouseEvent) => {
      if (isResizing) {
        const newWidth = e.clientX; 
        if (newWidth >= 200 && newWidth <= window.innerWidth * 0.5) {
          setWidth(newWidth);
        }
      }
    };

    if (isResizing) {
      window.addEventListener('mousemove', resize);
      window.addEventListener('mouseup', stopResizing);
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
    }

    return () => {
      window.removeEventListener('mousemove', resize);
      window.removeEventListener('mouseup', stopResizing);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
  }, [isResizing]);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        isOpen && 
        !isLocked && 
        sidebarRef.current && 
        !sidebarRef.current.contains(event.target as Node) &&
        !(event.target as Element).closest('.sftp-context-menu')
      ) {
        onClose();
      }
    };

    if (isOpen && !isLocked) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [isOpen, isLocked, onClose]);

  const handleDownload = async () => {
    if (!contextMenu || !sessionId || !config || !contextMenu.entry) return;
    try {
        const localPath = await invoke<string>('select_save_path', {
            defaultName: contextMenu.entry.name,
            initialDir: config.general.sftp?.defaultDownloadPath
        });
        if (localPath) {
             await invoke('sftp_download', { sessionId, remotePath: contextMenu.entry.path, localPath });
        }
    } catch (e) {
        console.error('Download failed', e);
    }
    handleCloseContextMenu();
  };

  const handleUpload = async () => {
      if (!sessionId) return;
      const targetPath = contextMenu && contextMenu.entry?.is_dir ? contextMenu.entry.path : currentPath;
      
      // Reset overwrite all flag for new upload batch
      overwriteAllRef.current = false;

      try {
          const selected = await invoke<string[] | null>('pick_files');

          if (selected && selected.length > 0) {
              const files = selected;
              
              // We will create a promise for each file upload
              const uploadPromises = files.map(async (file) => {
                  const filename = file.split(/[\\/]/).pop();
                  const taskId = uuidv4(); // Generate ID in frontend

                  // 1. Create a promise that waits for this specific task completion
                  const completionPromise = new Promise<void>((resolve, reject) => {
                      let unlisten: (() => void) | undefined;
                      let resolved = false;

                      // Start listening BEFORE invoking the backend
                      listen<TransferTask>('transfer-progress', (event) => {
                          const task = event.payload;
                          if (task.task_id === taskId) {
                              resolved = true;
                              // Don't unlisten immediately if we want to catch all events? 
                              // Actually we only care about terminal states.
                              if (task.status === 'completed') {
                                  unlisten?.();
                                  resolve();
                              } else if (task.status === 'cancelled') {
                                  unlisten?.();
                                  if (task.error && task.error.includes('Skipped')) {
                                      reject(new Error('Skipped'));
                                  } else {
                                      reject(new Error('Cancelled'));
                                  }
                              } else if (task.status === 'failed') {
                                  unlisten?.();
                                  reject(new Error(task.error || 'Failed'));
                              }
                          }
                      }).then(fn => { unlisten = fn; });

                      // Timeout safety net (60 seconds)
                      setTimeout(() => {
                          if (!resolved) {
                              unlisten?.();
                              reject(new Error('Timeout waiting for transfer'));
                          }
                      }, 60000);
                  });

                  try {
                      // 2. Start the upload with our pre-generated ID
                      await invoke('sftp_upload', { 
                          sessionId, 
                          localPath: file, 
                          remotePath: `${targetPath}/${filename}`,
                          taskId 
                      });
                      
                      // 3. Wait for completion
                      await completionPromise;
                  } catch (e: any) {
                      console.error('Upload failed/cancelled for file:', file, e);
                      // Rethrow if cancelled by user to stop other uploads?
                      // With Promise.all, others are already started.
                      // We just let them finish or fail independently.
                  }
              });

              // Run all uploads in parallel (backend manages concurrency queue)
              await Promise.all(uploadPromises);
          }
      } catch (e) {
          console.error('File selection failed', e);
      }
      handleCloseContextMenu();
  };

  const normalizeRemotePath = (path: string): string => {
    const normalized = path.replace(/\\/g, '/').replace(/\/+/g, '/');
    if (normalized === '') return '/';
    if (normalized.length > 1 && normalized.endsWith('/')) {
      return normalized.slice(0, -1);
    }
    return normalized;
  };

  const getParentPath = (path: string): string => {
    const normalized = normalizeRemotePath(path);
    if (normalized === '/' || normalized === '.') return '/';
    const lastSlash = normalized.lastIndexOf('/');
    return lastSlash <= 0 ? '/' : normalized.substring(0, lastSlash);
  };

  const joinRemotePath = (parentPath: string, itemName: string): string => {
    const normalizedParentPath = normalizeRemotePath(parentPath);
    const normalizedItemName = itemName.replace(/^\/+/, '').replace(/\/+$/, '');
    if (normalizedParentPath === '/') {
      return `/${normalizedItemName}`;
    }
    return `${normalizedParentPath}/${normalizedItemName}`;
  };

  const quoteForShell = (input: string): string => {
    return `'${input.replace(/'/g, `'\"'\"'`)}'`;
  };

  const remapExpandedPathsAfterRename = (
    expandedPaths: Set<string>,
    oldPath: string,
    newPath: string
  ): Set<string> => {
    const normalizedOldPath = normalizeRemotePath(oldPath);
    const normalizedNewPath = normalizeRemotePath(newPath);
    const nextExpandedPaths = new Set<string>();

    expandedPaths.forEach(path => {
      const normalizedPath = normalizeRemotePath(path);
      if (normalizedPath === normalizedOldPath) {
        nextExpandedPaths.add(normalizedNewPath);
        return;
      }
      if (normalizedPath.startsWith(`${normalizedOldPath}/`)) {
        nextExpandedPaths.add(`${normalizedNewPath}${normalizedPath.slice(normalizedOldPath.length)}`);
        return;
      }
      nextExpandedPaths.add(normalizedPath);
    });

    return nextExpandedPaths;
  };

  const filterExpandedPathsBySubtree = (expandedPaths: Set<string>, basePath: string): Set<string> => {
    const normalizedBasePath = normalizeRemotePath(basePath);
    if (normalizedBasePath === '/') {
      return expandedPaths;
    }

    const subtreeExpandedPaths = new Set<string>();
    expandedPaths.forEach(path => {
      if (path === normalizedBasePath || path.startsWith(`${normalizedBasePath}/`)) {
        subtreeExpandedPaths.add(path);
      }
    });
    return subtreeExpandedPaths;
  };

  const isDirectory = (entry: FileEntry): boolean => {
    return entry.is_dir || Boolean(entry.is_symlink && entry.target_is_dir === true);
  };

  const refreshPasteTarget = async (sid: string, targetPath: string) => {
    const currentSessionState = sessions[sid];
    if (currentSessionState) {
      const expandedPaths = getAllExpandedPaths(currentSessionState.rootFiles);
      await loadDirectory(targetPath, sid, expandedPaths, currentSessionState.sortState);
    } else {
      await loadDirectory(targetPath, sid, undefined, sortState);
    }
  };

  const reloadParentDirectory = async (entry: FileEntry) => {
    if (!sessionId) return;
    const parentPath = getParentPath(entry.path);
    if (parentPath === '/' || parentPath === '.') {
      await loadDirectory('/', sessionId, undefined, sortState);
    } else {
      await loadDirectory(parentPath, sessionId, undefined, sortState);
    }
  };

  const refreshRenamedTreeNode = async (oldPath: string, newPath: string) => {
    if (!sessionId) return;
    const currentSessionState = sessions[sessionId];
    const currentSortState = currentSessionState?.sortState || sortState;
    const parentPath = getParentPath(newPath);
    const normalizedParentPath = parentPath === '.' ? '/' : parentPath;

    if (!currentSessionState) {
      await loadDirectory(normalizedParentPath, sessionId, undefined, currentSortState);
      return;
    }

    const expandedPaths = getAllExpandedPaths(currentSessionState.rootFiles);
    const remappedExpandedPaths = remapExpandedPathsAfterRename(expandedPaths, oldPath, newPath);
    const subtreeExpandedPaths = filterExpandedPathsBySubtree(remappedExpandedPaths, normalizedParentPath);

    await refreshDirectoryTree(sessionId, normalizedParentPath, subtreeExpandedPaths, currentSortState);
  };

  const permissionsToOctal = (perm: number | undefined): string => {
    if (perm === undefined) return '755';
    return (perm & 0o777).toString(8).padStart(3, '0');
  };

  const handleDelete = () => {
    if (!contextMenu || !contextMenu.entry) return;
    setDeleteModal({ isOpen: true, entry: contextMenu.entry });
    handleCloseContextMenu();
  };

  const confirmDelete = async () => {
    if (!deleteModal.entry || !sessionId) return;
    try {
      await invoke('sftp_delete', { sessionId, path: deleteModal.entry.path, isDir: deleteModal.entry.is_dir });
      await reloadParentDirectory(deleteModal.entry);
    } catch (e) {
      console.error('Delete failed', e);
    }
    setDeleteModal({ isOpen: false, entry: null });
  };

  const handleNewFile = () => {
    if (!contextMenu || !contextMenu.entry) return;
    const parentPath = isDirectory(contextMenu.entry) ? contextMenu.entry.path : getParentPath(contextMenu.entry.path);
    setNewFileModal({ isOpen: true, parentPath });
    setNewItemName('');
    handleCloseContextMenu();
  };

  const handleNewFolder = () => {
    if (!contextMenu || !contextMenu.entry) return;
    const parentPath = isDirectory(contextMenu.entry) ? contextMenu.entry.path : getParentPath(contextMenu.entry.path);
    setNewFolderModal({ isOpen: true, parentPath });
    setNewItemName('');
    handleCloseContextMenu();
  };

  const confirmNewFile = async () => {
    if (!sessionId || !newItemName.trim()) return;
    try {
      const fullPath = `${newFileModal.parentPath}/${newItemName.trim()}`;
      await invoke('sftp_create_file', { sessionId, path: fullPath });
      await loadDirectory(newFileModal.parentPath, sessionId, undefined, sortState);
    } catch (e) {
      console.error('Create file failed', e);
    }
    setNewFileModal({ isOpen: false, parentPath: '' });
    setNewItemName('');
  };

  const confirmNewFolder = async () => {
    if (!sessionId || !newItemName.trim()) return;
    try {
      const fullPath = `${newFolderModal.parentPath}/${newItemName.trim()}`;
      await invoke('sftp_create_folder', { sessionId, path: fullPath });
      await loadDirectory(newFolderModal.parentPath, sessionId, undefined, sortState);
    } catch (e) {
      console.error('Create folder failed', e);
    }
    setNewFolderModal({ isOpen: false, parentPath: '' });
    setNewItemName('');
  };

  const handleRename = () => {
    if (!contextMenu || !contextMenu.entry) return;
    setRenameModal({ isOpen: true, entry: contextMenu.entry });
    setNewItemName(contextMenu.entry.name);
    handleCloseContextMenu();
  };

  const confirmRename = async () => {
    if (!sessionId || !renameModal.entry || !newItemName.trim()) return;
    if (newItemName.trim() === renameModal.entry.name) {
      setRenameModal({ isOpen: false, entry: null });
      setNewItemName('');
      return;
    }

    try {
      const oldPath = normalizeRemotePath(renameModal.entry.path);
      const parentPath = getParentPath(oldPath);
      const newPath = joinRemotePath(parentPath, newItemName.trim());
      await invoke('sftp_rename', { sessionId, oldPath, newPath });
      await refreshRenamedTreeNode(oldPath, newPath);
    } catch (e) {
      console.error('Rename failed', e);
    }
    setRenameModal({ isOpen: false, entry: null });
    setNewItemName('');
  };

  const handleProperties = () => {
    if (!contextMenu || !contextMenu.entry) return;
    const octalPerms = permissionsToOctal(contextMenu.entry.permissions);
    setPropertiesModal({ isOpen: true, entry: contextMenu.entry, permissions: octalPerms });
    setPermissionInput(octalPerms);
    handleCloseContextMenu();
  };

  const confirmProperties = async () => {
    if (!propertiesModal.entry || !sessionId) return;
    try {
      const permNum = parseInt(permissionInput, 8);
      await invoke('sftp_chmod', { sessionId, path: propertiesModal.entry.path, mode: permNum });
      await reloadParentDirectory(propertiesModal.entry);
    } catch (e) {
      console.error('Chmod failed', e);
    }
    setPropertiesModal({ isOpen: false, entry: null, permissions: '' });
  };

  const handleCopyName = () => {
    if (!contextMenu || !contextMenu.entry) return;
    navigator.clipboard.writeText(contextMenu.entry.name);
    handleCloseContextMenu();
  };

  const handleCopyFullPath = () => {
    if (!contextMenu || !contextMenu.entry) return;
    navigator.clipboard.writeText(contextMenu.entry.path);
    handleCloseContextMenu();
  };

  const handleSendPath = () => {
    if (!contextMenu || !contextMenu.entry) return;
    window.dispatchEvent(new CustomEvent('paste-snippet', { detail: quotePathForTerminalInput(contextMenu.entry.path) }));
    handleCloseContextMenu();
  };

  const handleTerminalJump = () => {
    if (!contextMenu || !contextMenu.entry) return;
    const path = contextMenu.entry.path.replace(/"/g, '\\"');
    const command = `cd "${path}"\r`;
    window.dispatchEvent(new CustomEvent('paste-snippet', { detail: command }));
    handleCloseContextMenu();
  };

  const handleCopyForPaste = () => {
    if (!contextMenu || !sessionId || !contextMenu.entry) return;
    setClipboard({
      sourcePath: contextMenu.entry.path,
      sourceName: contextMenu.entry.name,
      isDir: contextMenu.entry.is_dir,
      isCut: false,
      sessionId
    });
    handleCloseContextMenu();
  };

  const handleCut = () => {
    if (!contextMenu || !sessionId || !contextMenu.entry) return;
    setClipboard({
      sourcePath: contextMenu.entry.path,
      sourceName: contextMenu.entry.name,
      isDir: contextMenu.entry.is_dir,
      isCut: true,
      sessionId
    });
    handleCloseContextMenu();
  };

  const handlePaste = async () => {
    if (!contextMenu || !sessionId || !clipboard) return;

    const targetPath = (contextMenu.entry && contextMenu.entry.is_dir)
      ? contextMenu.entry.path
      : currentPath;

    const sourceParentPath = clipboard.sourcePath.substring(0, clipboard.sourcePath.lastIndexOf('/')) || '/';
    const isSameDirectory = sourceParentPath === targetPath;

    const destName = clipboard.isCut
      ? clipboard.sourceName
      : (isSameDirectory ? `copy_of_${clipboard.sourceName}` : clipboard.sourceName);

    const destPath = `${targetPath}/${destName}`;

    try {
      if (clipboard.isCut) {
        await invoke('sftp_rename', {
          sessionId,
          oldPath: clipboard.sourcePath,
          newPath: destPath
        });
      } else {
        try {
          await invoke('sftp_copy', {
            sessionId,
            sourcePath: clipboard.sourcePath,
            destPath
          });
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          if (message.includes(COPY_DATA_UNSUPPORTED_ERROR)) {
            setCopyFallbackModal({
              isOpen: true,
              sessionId,
              sourcePath: clipboard.sourcePath,
              destPath,
              targetPath
            });
            handleCloseContextMenu();
            return;
          }

          throw error;
        }
      }

      await refreshPasteTarget(sessionId, targetPath);
    } catch (e) {
      console.error('Paste failed', e);
    }

    setClipboard(null);
    handleCloseContextMenu();
  };

  const handleClearClipboard = () => {
    setClipboard(null);
    handleCloseContextMenu();
  };

  const closeCopyFallbackModal = () => {
    setCopyFallbackModal({
      isOpen: false,
      sessionId: '',
      sourcePath: '',
      destPath: '',
      targetPath: ''
    });
  };

  const handleUseTerminalCpFallback = async () => {
    if (!copyFallbackModal.isOpen || !copyFallbackModal.sessionId) return;

    const cpCommand = `cp -a -- ${quoteForShell(copyFallbackModal.sourcePath)} ${quoteForShell(copyFallbackModal.destPath)}`;
    try {
      await invoke('send_command', {
        params: {
          session_id: copyFallbackModal.sessionId,
          command: `${cpCommand}\r`
        }
      });
    } catch (error) {
      console.error('Failed to execute terminal cp fallback', error);
    } finally {
      closeCopyFallbackModal();
      setClipboard(null);
      handleCloseContextMenu();
    }
  };

  const handleUseStreamingFallback = async () => {
    if (!copyFallbackModal.isOpen || !copyFallbackModal.sessionId) return;

    const { sessionId: fallbackSessionId, sourcePath, destPath, targetPath } = copyFallbackModal;
    const taskId = uuidv4();
    try {
      await invoke('sftp_copy_streaming', {
        sessionId: fallbackSessionId,
        sourcePath,
        destPath,
        taskId
      });
      await refreshPasteTarget(fallbackSessionId, targetPath);
    } catch (error) {
      console.error('Streaming fallback copy failed', error);
    } finally {
      closeCopyFallbackModal();
      setClipboard(null);
      handleCloseContextMenu();
    }
  };

  const handleResolveConflict = async (conflict: FileConflict, resolution: ConflictResolution | "overwrite-all") => {
    try {
      if (resolution === 'overwrite-all') {
        overwriteAllRef.current = true;
        resolution = 'overwrite';
      }
      
      await invoke('sftp_resolve_conflict', {
        taskId: conflict.task_id,
        resolution: resolution
      });
      removeConflict(conflict.task_id);
    } catch (error) {
      console.error('Failed to resolve conflict:', error);
    }
  };

  return (
    <>
    <div
      ref={sidebarRef}
      className={`absolute top-0 bottom-0 overflow-hidden bg-[var(--bg-secondary)] border-r flex flex-col transition-all duration-200 shadow-[2px_0_8px_rgba(0,0,0,0.2)] ${isOpen ? 'opacity-100 visible border-r-[var(--glass-border)]' : 'opacity-0 invisible border-transparent'} ${isResizing ? 'transition-none' : ''} ${isLocked ? '!relative shadow-none !left-auto !top-auto !bottom-auto h-full' : ''}`}
      style={{ width: isOpen ? `${width}px` : '0px', zIndex }}
      aria-hidden={!isOpen}
    >
      <div
        className="absolute top-0 bottom-0 right-0 w-[5px] cursor-col-resize bg-transparent transition-colors duration-200 hover:bg-[var(--accent-primary)] hover:opacity-50"
        onMouseDown={startResizing}
        role="none"
        style={{ zIndex: zIndex ? zIndex + 1 : undefined }}
      />

      <div className="flex items-center justify-between p-3 pr-4 border-b border-[var(--glass-border)] flex-shrink-0">
        <h3 className="text-[14px] font-semibold text-[var(--text-primary)] flex items-center gap-2 m-0 whitespace-nowrap">
          <Folder size={16} /> SFTP
        </h3>
        <div className="flex items-center gap-1">
           <div className="relative">
              <button
                type="button"
                className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] opacity-100"
                onClick={() => setShowSortMenu(!showSortMenu)}
                title={t.sftp.tooltips.sort}
              >
                <ArrowDownUp size={16} />
              </button>
              {showSortMenu && (
                <div className="fixed bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded shadow-[0_10px_15px_-3px_rgba(0,0,0,0.1),0_4px_6px_-2px_rgba(0,0,0,0.05)] min-w-[180px] p-1 z-50 overflow-visible animate-sftp-slide-in backdrop-blur-xl" style={{ position: 'absolute', top: '100%', right: 0, marginTop: '4px', minWidth: '150px' }}>
                 <button type="button" onClick={() => handleSort('name')}>
                   <span>{t.sftp.sort.name}</span>
                   {sortState.type === 'name' && (
                     sortState.order === 'asc' ? <ArrowDown size={14} /> : <ArrowUp size={14} />
                   )}
                  </button>
                  <button type="button" onClick={() => handleSort('modified')} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                    <span>{t.sftp.sort.dateModified}</span>
                    {sortState.type === 'modified' && (
                      sortState.order === 'asc' ? <ArrowDown size={14} /> : <ArrowUp size={14} />
                    )}
                  </button>
                </div>
              )}
            </div>
            <button
              type="button"
              className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] opacity-100"
              onClick={handleRefresh}
              title={t.sftp.tooltips.refresh}
            >
              <RefreshCw size={16} />
            </button>
           <button
             type="button"
             className={`bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] opacity-100 ${isLocked ? 'text-[var(--accent-primary)]' : ''}`}
            onClick={onToggleLock}
            title={isLocked ? t.sftp.tooltips.unlock : t.sftp.tooltips.lock}
          >
            {isLocked ? <Lock size={16} /> : <LockOpen size={16} />}
           </button>
           <button
             type="button"
             className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] opacity-100"
             onClick={onClose}
             title={t.sftp.tooltips.close}
           >
             <X size={16} />
           </button>
         </div>
       </div>

      <div
        className="flex-1 overflow-y-auto overflow-x-auto py-0 px-2"
        data-sftp-tree-scroll
      >
          {rootFiles.map(entry => (
              <FileTreeItem
                key={entry.path}
                entry={entry}
                depth={0}
                onToggle={handleToggle}
                onContextMenu={handleContextMenu}
                onDragStart={handleTreeItemDragStart}
                clipboardSourcePath={clipboard?.sourcePath}
                clipboardIsCut={clipboard?.isCut}
              />
          ))}
          {rootFiles.length === 0 && !isLoading && (
              <div className="p-4 text-center text-gray-500 text-sm">
                  {sessionId ? t.sftp.noFiles : t.sftp.notConnected}
              </div>
          )}
          {isLoading && rootFiles.length === 0 && (
              <div className="p-4 text-center text-gray-500 text-sm">
                  {t.sftp.loading}
              </div>
          )}
      </div>
      
      <TransferStatusPanel />
    </div>

      {contextMenu && (
        <div
            className="fixed bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded shadow-[0_10px_15px_-3px_rgba(0,0,0,0.1),0_4px_6px_-2px_rgba(0,0,0,0.05)] min-w-[180px] p-1 z-50 overflow-visible animate-sftp-slide-in backdrop-blur-xl"
            style={{ 
              top: contextMenu.y > window.innerHeight - 350 ? 'auto' : contextMenu.y, 
              bottom: contextMenu.y > window.innerHeight - 350 ? window.innerHeight - contextMenu.y : 'auto',
              left: contextMenu.x 
            }}
        >
            <button type="button" onClick={handleDownload} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                <Download size={14} /> {t.sftp.contextMenu.download}
            </button>
            <button type="button" onClick={handleUpload} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                <Upload size={14} /> {t.sftp.contextMenu.upload}
            </button>
            {contextMenu.entry && isDirectory(contextMenu.entry) && (
                <>
                    <button type="button" onClick={handleNewFile} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                        <Plus size={14} /> {t.sftp.contextMenu.newFile}
                    </button>
                    <button type="button" onClick={handleNewFolder} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                        <FolderPlus size={14} /> {t.sftp.contextMenu.newFolder}
                    </button>
                </>
            )}
            {contextMenu.entry && !isDirectory(contextMenu.entry) && (
                <div className="relative">
                    <div
                        onMouseEnter={() => {
                            if (editSubmenuTimeout) {
                                clearTimeout(editSubmenuTimeout);
                                setEditSubmenuTimeout(null);
                            }
                            setShowEditSubmenu(true);
                        }}
                        onMouseLeave={() => {
                            const timeout = setTimeout(() => {
                                setShowEditSubmenu(false);
                            }, 200);
                            setEditSubmenuTimeout(timeout);
                        }}
                    >
                        <button type="button" onClick={() => setShowEditSubmenu(!showEditSubmenu)} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                            <Edit size={14} /> {t.sftp.contextMenu.edit}
                            <ChevronRight size={14} style={{ marginLeft: 'auto', opacity: 0.5 }} />
                        </button>
                        {showEditSubmenu && (
                            <div
                                className="absolute top-[-4px] left-full ml-1 bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded shadow-[0_10px_15px_-3px_rgba(0,0,0,0.2),0_4px_6px_-2px_rgba(0,0,0,0.1)] min-w-[200px] p-1 z-[1001] overflow-visible backdrop-blur-xl animate-sftp-fade-in"
                            >
                                <button type="button" onClick={handleEditVim} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                                    <Terminal size={14} /> {t.sftp.contextMenu.editServerVim}
                                </button>
                                <button type="button" onClick={handleEditLocal} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                                    <FileCode size={14} /> {t.sftp.contextMenu.editLocal}
                                </button>
                            </div>
                        )}
                    </div>
                </div>
            )}
            <div className="relative">
                <div
                    onMouseEnter={() => {
                        if (pathSubmenuTimeout) {
                            clearTimeout(pathSubmenuTimeout);
                            setPathSubmenuTimeout(null);
                        }
                        setShowPathSubmenu(true);
                    }}
                    onMouseLeave={() => {
                        const timeout = setTimeout(() => {
                            setShowPathSubmenu(false);
                        }, 200);
                        setPathSubmenuTimeout(timeout);
                    }}
                >
                    <button type="button" onClick={() => setShowPathSubmenu(!showPathSubmenu)} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                        <Link size={14} /> {t.sftp.contextMenu.path}
                        <ChevronRight size={14} style={{ marginLeft: 'auto', opacity: 0.5 }} />
                    </button>
                    {showPathSubmenu && contextMenu.entry && (
                        <div
                            className="absolute top-[-4px] left-full ml-1 bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded shadow-[0_10px_15px_-3px_rgba(0,0,0,0.2),0_4px_6px_-2px_rgba(0,0,0,0.1)] min-w-[200px] p-1 z-[1001] overflow-visible backdrop-blur-xl animate-sftp-fade-in"
                        >
                            <button type="button" onClick={handleCopyName} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                                <Copy size={14} /> {isDirectory(contextMenu.entry) ? t.sftp.contextMenu.copyFolderName : t.sftp.contextMenu.copyFileName}
                            </button>
                            <button type="button" onClick={handleCopyFullPath} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                                <Copy size={14} /> {t.sftp.contextMenu.copyFullPath}
                            </button>
                            <button type="button" onClick={handleSendPath} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                                <Terminal size={14} /> {t.sftp.contextMenu.sendPathToTerminal}
                            </button>
                            {isDirectory(contextMenu.entry) && (
                                <button type="button" onClick={handleTerminalJump} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                                    <Terminal size={14} /> {t.sftp.contextMenu.terminalJump}
                                </button>
                            )}
                        </div>
                    )}
                </div>
            </div>
            {contextMenu.entry && (
                <>
                    <button type="button" onClick={handleCopyForPaste} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                        <Copy size={14} /> {t.sftp.contextMenu.copy}
                    </button>
                    <button type="button" onClick={handleCut} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                        <Pencil size={14} /> {t.sftp.contextMenu.cut}
                    </button>
                </>
            )}
            {clipboard && (
                <>
                    <button type="button" onClick={handlePaste} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                        <Clipboard size={14} /> {clipboard.isCut ? t.sftp.contextMenu.pasteMove : t.sftp.contextMenu.pasteCopy}
                    </button>
                    <button type="button" onClick={handleClearClipboard} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-muted)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]">
                        <X size={14} /> {t.sftp.contextMenu.cancel}
                    </button>
                </>
            )}
            <button type="button" onClick={handleDelete} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                <Trash size={14} /> {t.sftp.contextMenu.delete}
            </button>
            {contextMenu.entry && (
                <>
                    <button type="button" onClick={handleRename} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                        <Pencil size={14} /> {t.sftp.contextMenu.rename}
                    </button>
                    <button type="button" onClick={handleProperties} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                        <Settings size={14} /> {t.sftp.contextMenu.properties}
                    </button>
                </>
            )}
            
            {contextMenu.entry && config?.sftpCustomCommands && config.sftpCustomCommands.some(cmd => matchCustomCommand(contextMenu.entry!, cmd)) && (
                 <div className="relative border-t border-[var(--glass-border)] mt-1 pt-1">
                    <div
                        role="menuitem"
                        onMouseEnter={() => {
                            if (customCommandsSubmenuTimeout) {
                                clearTimeout(customCommandsSubmenuTimeout);
                                setCustomCommandsSubmenuTimeout(null);
                            }
                            setShowCustomCommandsSubmenu(true);
                        }}
                        onMouseLeave={() => {
                            const timeout = setTimeout(() => {
                                setShowCustomCommandsSubmenu(false);
                            }, 200);
                            setCustomCommandsSubmenuTimeout(timeout);
                        }}
                    >
                        <button type="button" onClick={() => setShowCustomCommandsSubmenu(!showCustomCommandsSubmenu)} className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5">
                            <Terminal size={14} /> {t.sftp.contextMenu.commands}
                            <ChevronRight size={14} style={{ marginLeft: 'auto', opacity: 0.5 }} />
                        </button>
                        {showCustomCommandsSubmenu && (
                            <div
                                className="absolute top-[-4px] left-full ml-1 bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded shadow-[0_10px_15px_-3px_rgba(0,0,0,0.2),0_4px_6px_-2px_rgba(0,0,0,0.1)] min-w-[200px] p-1 z-[1001] overflow-visible backdrop-blur-xl animate-sftp-fade-in"
                            >
                                {config.sftpCustomCommands
                                    .filter(cmd => matchCustomCommand(contextMenu.entry!, cmd))
                                    .sort((a, b) => a.name.localeCompare(b.name))
                                    .map(cmd => (
                                    <button 
                                        key={cmd.id}
                                        type="button" 
                                        onClick={() => handleExecuteCustomCommand(cmd)} 
                                        className="flex items-center gap-2.5 w-full px-3 py-2 border-0 bg-transparent text-[var(--text-primary)] text-[14px] cursor-pointer rounded text-left transition-all duration-150 font-inherit relative hover:bg-[var(--bg-tertiary)] hover:text-[var(--accent-primary)] hover:translate-x-0.5"
                                    >
                                        <Terminal size={14} /> {cmd.name}
                                    </button>
                                ))}
                            </div>
                        )}
                    </div>
                </div>
            )}
        </div>
      )}

    <ConfirmationModal
      isOpen={copyFallbackModal.isOpen}
      title={t.sftp.modals.copyFallbackTitle}
      message={t.sftp.modals.copyFallbackMessage}
      confirmText={t.sftp.modals.copyFallbackUseCp}
      cancelText={t.sftp.modals.copyFallbackUseStreaming}
      onConfirm={handleUseTerminalCpFallback}
      onCancel={handleUseStreamingFallback}
      type="warning"
    />

    <ConfirmationModal
      isOpen={deleteModal.isOpen}
      title={t.sftp.modals.deleteConfirmTitle}
      message={t.sftp.modals.deleteConfirmMessage.replace('this item', `"${deleteModal.entry?.name}"`).replace('此项', `"${deleteModal.entry?.name}"`)}
      confirmText={t.common.delete}
      onConfirm={confirmDelete}
      onCancel={() => setDeleteModal({ isOpen: false, entry: null })}
      type="danger"
    />

    <FormModal
      isOpen={newFileModal.isOpen}
      title={t.sftp.modals.newFileTitle}
      onSubmit={confirmNewFile}
      onClose={() => setNewFileModal({ isOpen: false, parentPath: '' })}
      submitText={t.common.create}
    >
      <input
        type="text"
        autoFocus
        value={newItemName}
        onChange={(e) => setNewItemName(e.target.value)}
        placeholder={t.sftp.modals.itemNameLabel}
        className="sftp-input"
        style={{
          width: '100%',
          padding: '8px',
          background: 'var(--bg-tertiary)',
          border: '1px solid var(--border-color)',
          color: 'var(--text-primary)'
        }}
      />
    </FormModal>

    <FormModal
      isOpen={newFolderModal.isOpen}
      title={t.sftp.modals.newFolderTitle}
      onSubmit={confirmNewFolder}
      onClose={() => setNewFolderModal({ isOpen: false, parentPath: '' })}
      submitText={t.common.create}
    >
      <input
        type="text"
        autoFocus
        value={newItemName}
        onChange={(e) => setNewItemName(e.target.value)}
        placeholder={t.sftp.modals.itemNameLabel}
        className="sftp-input"
        style={{
          width: '100%',
          padding: '8px',
          background: 'var(--bg-tertiary)',
          border: '1px solid var(--border-color)',
          color: 'var(--text-primary)'
        }}
      />
    </FormModal>

    <FormModal
      isOpen={renameModal.isOpen}
      title={t.sftp.modals.renameTitle}
      onSubmit={confirmRename}
      onClose={() => setRenameModal({ isOpen: false, entry: null })}
      submitText={t.common.save}
    >
      <input
        type="text"
        value={newItemName}
        onChange={(e) => setNewItemName(e.target.value)}
        placeholder={t.sftp.modals.itemNameLabel}
        className="sftp-input"
        style={{
          width: '100%',
          padding: '8px',
          background: 'var(--bg-tertiary)',
          border: '1px solid var(--border-color)',
          color: 'var(--text-primary)'
        }}
      />
    </FormModal>

    <FormModal
      isOpen={propertiesModal.isOpen}
      title={t.sftp.modals.propertiesTitle}
      onSubmit={confirmProperties}
      onClose={() => setPropertiesModal({ isOpen: false, entry: null, permissions: '' })}
      submitText={t.common.apply}
    >
      <div style={{ marginBottom: '16px' }}>
        <div style={{ display: 'block', marginBottom: '8px', fontSize: '14px', color: 'var(--text-secondary)' }}>
          {t.common.name}: {propertiesModal.entry?.name}
        </div>
        <div style={{ display: 'block', marginBottom: '8px', fontSize: '14px', color: 'var(--text-secondary)' }}>
          {t.common.path}: {propertiesModal.entry?.path}
        </div>
        <label htmlFor="permissions-input" style={{ display: 'block', marginBottom: '8px', fontSize: '14px', color: 'var(--text-secondary)' }}>
          {t.sftp.modals.permissionsLabel}
        </label>
        <input
          id="permissions-input"
          type="text"
          value={permissionInput}
          onChange={(e) => setPermissionInput(e.target.value)}
          placeholder={t.sftp.modals.permissionsPlaceholder}
          maxLength={3}
          className="sftp-input"
          style={{
            width: '100%',
            padding: '8px',
            background: 'var(--bg-tertiary)',
            border: '1px solid var(--border-color)',
            color: 'var(--text-primary)'
          }}
        />
      </div>
    </FormModal>

    {conflicts.map((conflict) => (
      <FileConflictDialog
        key={conflict.task_id}
        conflict={conflict}
        onResolve={(resolution) => handleResolveConflict(conflict, resolution)}
      />
    ))}
    </>
  );
};
