import React, { useState, useEffect, useRef, useCallback } from 'react';
import { X, Lock, LockOpen, Folder, File, ChevronRight, ChevronDown, Download, Upload, RefreshCw, FolderOpen, FolderSymlink, FileSymlink, ArrowDownUp, ArrowUp, ArrowDown } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import './SFTPSidebar.css';

import { TransferStatusPanel } from './TransferStatusPanel';

interface SFTPSidebarProps {
  isOpen: boolean;
  onClose: () => void;
  isLocked: boolean;
  onToggleLock: () => void;
  sessionId?: string; 
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
  children?: FileEntry[];
  isExpanded?: boolean;
  isLoading?: boolean;
}

const FileTreeItem: React.FC<{
  entry: FileEntry;
  depth: number;
  onToggle: (entry: FileEntry) => void;
  onContextMenu: (e: React.MouseEvent, entry: FileEntry) => void;
}> = ({ entry, depth, onToggle, onContextMenu }) => {
  return (
    <div style={{ paddingLeft: `${depth * 12}px` }}>
      <button 
        type="button"
        className={`sftp-tree-item ${entry.isExpanded ? 'expanded' : ''} w-full text-left bg-transparent border-0 p-0 m-0 flex items-center cursor-pointer hover:bg-[var(--bg-tertiary)]`}
        onClick={() => onToggle(entry)}
        onContextMenu={(e) => onContextMenu(e, entry)}
        style={{ color: 'inherit' }}
      >
        <div className="sftp-tree-indent" style={{ width: '16px', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
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
            <FolderSymlink size={16} className="sftp-icon sftp-folder-icon" />
          ) : (
            <FileSymlink size={16} className="sftp-icon sftp-file-icon" />
          )
        ) : entry.is_dir ? (
          entry.isExpanded ? (
            <FolderOpen size={16} className="sftp-icon sftp-folder-icon" />
          ) : (
            <Folder size={16} className="sftp-icon sftp-folder-icon" />
          )
        ) : (
          <File size={16} className="sftp-icon sftp-file-icon" />
        )}
        
        <span className="truncate ml-1">
          {entry.name}
          {entry.link_target && (
            <span className="sftp-symlink-target">
              → {entry.link_target}
            </span>
          )}
        </span>
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

export const SFTPSidebar: React.FC<SFTPSidebarProps> = ({
  isOpen,
  onClose,
  isLocked,
  onToggleLock,
  sessionId
}) => {
  const [width, setWidth] = useState(300);
  const [isResizing, setIsResizing] = useState(false);
  const sidebarRef = useRef<HTMLDivElement>(null);
  const [rootFiles, setRootFiles] = useState<FileEntry[]>([]);
  const [currentPath, setCurrentPath] = useState('/');
  const [isLoading, setIsLoading] = useState(false);
  const [sortState, setSortState] = useState<SortState>({ type: 'name', order: 'asc' });
  const [showSortMenu, setShowSortMenu] = useState(false);
  
  const [contextMenu, setContextMenu] = useState<{ x: number, y: number, entry: FileEntry } | null>(null);

  // Close sort menu on click outside
  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
        if (showSortMenu && !(e.target as Element).closest('.sftp-sort-menu') && !(e.target as Element).closest('.sftp-sort-btn')) {
            setShowSortMenu(false);
        }
    };
    window.addEventListener('click', handleClick);
    return () => window.removeEventListener('click', handleClick);
  }, [showSortMenu]);

  const sortFiles = useCallback((files: FileEntry[], sort: SortState) => {
      return [...files].sort((a, b) => {
          // Always directories first
          if (a.is_dir !== b.is_dir) {
              return a.is_dir ? -1 : 1;
          }
          if (a.is_symlink && a.target_is_dir !== b.target_is_dir) {
               // Treat symlink to dir as dir
               if (a.target_is_dir) return -1;
               if (b.target_is_dir) return 1;
          }

          let comparison = 0;
          if (sort.type === 'name') {
              comparison = a.name.localeCompare(b.name);
          } else if (sort.type === 'modified') {
              comparison = a.modified - b.modified;
          }

          return sort.order === 'asc' ? comparison : -comparison;
      });
  }, []);

  const loadDirectory = useCallback(async (path: string) => {
    if (!sessionId) return;
    setIsLoading(true);
    try {
      const files = await invoke<FileEntry[]>('sftp_list_dir', { sessionId, path });
      const sortedFiles = sortFiles(files, sortState);
      
      if (path === '/' || path === '.') {
        setRootFiles(sortedFiles);
        setCurrentPath(path);
      }
      return sortedFiles;
    } catch (error) {
      console.error('Failed to load directory:', error);
      return [];
    } finally {
      setIsLoading(false);
    }
  }, [sessionId, sortFiles, sortState]); // Added sortState dependency

  // Re-sort when sort state changes
  useEffect(() => {
      setRootFiles(prev => sortFiles(prev, sortState));
  }, [sortState, sortFiles]);

  useEffect(() => {
    if (isOpen && sessionId) {
      loadDirectory('/');
    } else if (!sessionId) {
      setRootFiles([]);
    }
  }, [isOpen, sessionId, loadDirectory]);

  const handleToggle = async (entry: FileEntry) => {
    // Allow toggling if it's a directory OR a symlink to a directory
    if (!entry.is_dir && !(entry.is_symlink && entry.target_is_dir)) return;

    if (entry.isExpanded) {
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
        setRootFiles(toggleNode(rootFiles));
    } else {
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
        }
        setRootFiles(setLoading(rootFiles, true));

        try {
            const children = await invoke<FileEntry[]>('sftp_list_dir', { sessionId, path: entry.path });
            const sortedChildren = sortFiles(children, sortState);
            
            const updateChildren = (nodes: FileEntry[]): FileEntry[] => {
                return nodes.map(node => {
                    if (node.path === entry.path) {
                        return { ...node, isExpanded: true, isLoading: false, children: sortedChildren };
                    }
                    if (node.children) {
                        return { ...node, children: updateChildren(node.children) };
                    }
                    return node;
                });
            };
            setRootFiles(updateChildren(rootFiles));
        } catch (error) {
             console.error('Failed to load children:', error);
             setRootFiles(setLoading(rootFiles, false));
        }
    }
  };

  const handleSort = (type: SortType) => {
      setSortState(prev => ({
          type,
          // If clicking same type, toggle order. If new type, default to asc (or desc for date? usually desc for date is better but let's stick to simple toggle)
          order: prev.type === type && prev.order === 'asc' ? 'desc' : 'asc'
      }));
      setShowSortMenu(false);
  };

  const handleContextMenu = (e: React.MouseEvent, entry: FileEntry) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, entry });
  };

  const handleCloseContextMenu = useCallback(() => {
    setContextMenu(null);
  }, []);

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
        if (newWidth >= 200 && newWidth <= 600) {
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
    if (!contextMenu || !sessionId) return;
    try {
        const localPath = await invoke<string>('select_save_path', { defaultName: contextMenu.entry.name });
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
      const targetPath = contextMenu && contextMenu.entry.is_dir ? contextMenu.entry.path : currentPath;

      try {
          const selected = await invoke<string[] | null>('pick_files');
          
          if (selected) {
              const files = selected;
              for (const file of files) {
                  const filename = file.split(/[\\/]/).pop();
                  await invoke('sftp_upload', { sessionId, localPath: file, remotePath: `${targetPath}/${filename}` });
              }
              // Upload is async now, tracked in TransferStatusPanel
          }
      } catch (e) {
          console.error('Upload failed', e);
      }
      handleCloseContextMenu();
  };

  return (
    <>
    <div 
      ref={sidebarRef}
      className={`sftp-sidebar-panel ${isOpen ? 'open' : ''} ${isResizing ? 'resizing' : ''} ${isLocked ? 'locked' : ''}`}
      style={{ width: isOpen ? `${width}px` : '0px' }}
      aria-hidden={!isOpen}
    >
      <div 
        className="sftp-resizer" 
        onMouseDown={startResizing}
        role="none"
      />
      
      <div className="sftp-header">
        <h3 className="sftp-title">
          <Folder size={16} /> SFTP
        </h3>
        <div className="sftp-actions">
           <div className="relative">
             <button
               type="button"
               className="sftp-action-btn sftp-sort-btn"
               onClick={() => setShowSortMenu(!showSortMenu)}
               title="Sort"
             >
               <ArrowDownUp size={16} />
             </button>
             {showSortMenu && (
               <div className="sftp-sort-menu sftp-context-menu" style={{ position: 'absolute', top: '100%', right: 0, marginTop: '4px', minWidth: '150px' }}>
                 <button type="button" onClick={() => handleSort('name')}>
                   <span>Name</span>
                   {sortState.type === 'name' && (
                     sortState.order === 'asc' ? <ArrowDown size={14} /> : <ArrowUp size={14} />
                   )}
                 </button>
                 <button type="button" onClick={() => handleSort('modified')}>
                   <span>Date Modified</span>
                   {sortState.type === 'modified' && (
                     sortState.order === 'asc' ? <ArrowDown size={14} /> : <ArrowUp size={14} />
                   )}
                 </button>
               </div>
             )}
           </div>
           <button
             type="button"
             className="sftp-action-btn"
             onClick={() => loadDirectory(currentPath)}
             title="Refresh"
           >
             <RefreshCw size={16} />
           </button>
          <button
            type="button"
            className={`sftp-action-btn ${isLocked ? 'active' : ''}`}
            onClick={onToggleLock}
            title={isLocked ? "Unlock Sidebar" : "Lock Sidebar"}
          >
            {isLocked ? <Lock size={16} /> : <LockOpen size={16} />}
          </button>
          <button 
            type="button"
            className="sftp-action-btn"
            onClick={onClose}
            title="Close"
          >
            <X size={16} />
          </button>
        </div>
      </div>

      <div className="sftp-content">
          {rootFiles.map(entry => (
              <FileTreeItem 
                key={entry.path} 
                entry={entry} 
                depth={0} 
                onToggle={handleToggle}
                onContextMenu={handleContextMenu}
              />
          ))}
          {rootFiles.length === 0 && !isLoading && (
              <div className="p-4 text-center text-gray-500 text-sm">
                  {sessionId ? "No files" : "Not connected"}
              </div>
          )}
          {isLoading && rootFiles.length === 0 && (
              <div className="p-4 text-center text-gray-500 text-sm">
                  Loading...
              </div>
          )}
      </div>
      
      <TransferStatusPanel />
    </div>

    {contextMenu && (
        <div 
            className="sftp-context-menu"
            style={{ top: contextMenu.y, left: contextMenu.x }}
        >
            <button type="button" onClick={handleDownload}>
                <Download size={14} /> Download
            </button>
             <button type="button" onClick={handleUpload}>
                <Upload size={14} /> Upload
            </button>
        </div>
    )}
    </>
  );
};
