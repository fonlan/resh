import React, { useState, useEffect, useRef, useMemo } from 'react';
import { Snippet } from '../types/config';
import { X, Code, Play, ChevronRight, ChevronDown, Plus, Lock, LockOpen } from 'lucide-react';
import { useTranslation } from '../i18n';

interface SnippetsSidebarProps {
  snippets: Snippet[];
  isOpen: boolean;
  onClose: () => void;
  onOpenSettings?: () => void;
  isLocked: boolean;
  onToggleLock: () => void;
}

export const SnippetsSidebar: React.FC<SnippetsSidebarProps> = ({
  snippets,
  isOpen,
  onClose,
  onOpenSettings,
  isLocked,
  onToggleLock,
}) => {
  const { t } = useTranslation();
  const [width, setWidth] = useState(250);
  const [isResizing, setIsResizing] = useState(false);
  const sidebarRef = useRef<HTMLDivElement>(null);
  
  // Grouping logic
  const groupedSnippets = useMemo(() => {
    const groups = snippets.reduce((acc, snippet) => {
      const groupName = snippet.group || t.snippetForm.defaultGroup;
      if (!acc[groupName]) {
        acc[groupName] = [];
      }
      acc[groupName].push(snippet);
      return acc;
    }, {} as Record<string, Snippet[]>);

    Object.keys(groups).forEach(key => {
      groups[key].sort((a, b) => a.name.localeCompare(b.name));
    });

    return groups;
  }, [snippets, t.snippetForm.defaultGroup]);

  const groupNames = useMemo(() => {
    const defaultGroup = t.snippetForm.defaultGroup;
    return Object.keys(groupedSnippets).sort((a, b) => {
      if (a === defaultGroup) return -1;
      if (b === defaultGroup) return 1;
      return a.localeCompare(b);
    });
  }, [groupedSnippets, t.snippetForm.defaultGroup]);

  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>({});

  // Initialize default group expansion
  useEffect(() => {
    setExpandedGroups(prev => {
        // If already initialized, don't override unless it's empty
        if (Object.keys(prev).length > 0) return prev;
        return { [t.snippetForm.defaultGroup]: true };
    });
  }, [t.snippetForm.defaultGroup]);

  const toggleGroup = (groupName: string) => {
    setExpandedGroups(prev => ({
      ...prev,
      [groupName]: !prev[groupName]
    }));
  };

  const handleSnippetClick = (content: string) => {
    const event = new CustomEvent('paste-snippet', { detail: content });
    window.dispatchEvent(event);
    if (!isLocked) {
      onClose();
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent, content: string) => {
    if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        handleSnippetClick(content);
    }
  }

  const startResizing = (e: React.MouseEvent) => {
    e.preventDefault();
    setIsResizing(true);
  };

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        isOpen && 
        !isLocked && 
        sidebarRef.current && 
        !sidebarRef.current.contains(event.target as Node)
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

  useEffect(() => {
    const stopResizing = () => {
      setIsResizing(false);
    };

    const resize = (e: MouseEvent) => {
      if (isResizing) {
        // Calculate new width based on mouse position from right edge of window
        // Width = Window Width - Mouse X Position
        const newWidth = window.innerWidth - e.clientX;
        
        // Apply constraints
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
    } else {
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    }

    return () => {
      window.removeEventListener('mousemove', resize);
      window.removeEventListener('mouseup', stopResizing);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
  }, [isResizing]);

  return (
    <div
      ref={sidebarRef}
      className={`absolute top-0 bottom-0 overflow-hidden bg-[var(--bg-secondary)] border-l flex flex-col z-20 transition-all duration-200 shadow-[-2px_0_8px_rgba(0,0,0,0.2)] !right-0 !left-auto ${isOpen ? 'opacity-100 visible border-l-[var(--glass-border)]' : 'opacity-0 invisible border-transparent'} ${isResizing ? 'transition-none' : ''} ${isLocked ? '!relative shadow-none z-10 !right-auto !top-auto !bottom-auto h-full' : ''}`}
      aria-hidden={!isOpen}
      style={{ width: isOpen ? `${width}px` : '0px' }}
    >
      <div
        className="absolute top-0 bottom-0 left-0 w-[5px] cursor-col-resize z-25 bg-transparent transition-colors duration-200 hover:bg-[var(--accent-primary)] hover:opacity-50"
        onMouseDown={startResizing}
        role="separator"
        aria-orientation="vertical"
        aria-valuenow={width}
        aria-valuemin={200}
        aria-valuemax={600}
        aria-label="Resize Sidebar"
        tabIndex={0}
      />

      <div className="flex items-center justify-between p-3 pl-4 border-b border-[var(--glass-border)] flex-shrink-0">
        <h3 className="text-[13px] font-semibold text-[var(--text-primary)] flex items-center gap-2 m-0 whitespace-nowrap">
          <Code size={16} /> {t.snippetsTab.title}
        </h3>
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={onToggleLock}
            className={`bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] ${isLocked ? 'text-[var(--accent-primary)]' : ''}`}
            aria-label={isLocked ? "Unlock Sidebar" : "Lock Sidebar"}
            title={isLocked ? "Unlock Sidebar" : "Lock Sidebar"}
          >
            {isLocked ? <Lock size={16} /> : <LockOpen size={16} />}
          </button>
          {onOpenSettings && (
            <button
              type="button"
              onClick={onOpenSettings}
              className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
              aria-label={t.mainWindow.settings}
              title={t.mainWindow.settings}
            >
              <Plus size={16} />
            </button>
          )}
          <button
              type="button"
              onClick={onClose}
              className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded transition-all duration-200 flex items-center justify-center hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
              aria-label={t.windowControls.close}
          >
            <X size={16} />
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-2 flex flex-col gap-2 [&::-webkit-scrollbar]:w-1.5 [&::-webkit-scrollbar-track]:bg-transparent [&::-webkit-scrollbar-thumb]:bg-[var(--bg-tertiary)] [&::-webkit-scrollbar-thumb]:rounded-[3px]">
        {snippets.length === 0 ? (
           <p className="text-[12px] text-[var(--text-muted)] text-center mt-6 px-4 leading-relaxed">{t.snippetsTab.emptyState}</p>
        ) : (
          groupNames.map(groupName => (
            <div key={groupName} className="mb-1">
                <button
                    type="button"
                    className="flex items-center w-full p-2 bg-transparent border-0 text-[var(--text-secondary)] text-[12px] font-semibold cursor-pointer rounded transition-all duration-200 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
                    onClick={() => toggleGroup(groupName)}
                    aria-expanded={!!expandedGroups[groupName]}
                >
                    {expandedGroups[groupName] ? (
                        <ChevronDown size={14} className="mr-1.5 opacity-70" />
                    ) : (
                        <ChevronRight size={14} className="mr-1.5 opacity-70" />
                    )}
                    <span className="flex-1 text-left">{groupName}</span>
                </button>

                <div className={`pl-2 flex flex-col gap-1 overflow-hidden transition-all duration-200 ${!expandedGroups[groupName] ? 'hidden' : ''}`}>
                    {groupedSnippets[groupName].map(snippet => (
                        <button
                          key={snippet.id}
                          type="button"
                          className="p-2.5 rounded bg-[var(--bg-elevated)] cursor-pointer transition-all duration-200 border border-transparent outline-none hover:bg-[var(--bg-tertiary)] hover:border-[var(--glass-border)] focus:bg-[var(--bg-tertiary)] focus:border-[var(--glass-border)] group w-full text-left"
                          onClick={() => handleSnippetClick(snippet.content)}
                          onKeyDown={(e) => handleKeyDown(e, snippet.content)}
                          title={snippet.description}
                          aria-label={t.common.actions}
                        >
                          <div className="flex justify-between items-center mb-1">
                            <span className="text-[13px] font-medium text-[var(--text-primary)]">{snippet.name}</span>
                            <Play size={10} className="text-[var(--accent-success)] opacity-0 transition-opacity duration-200 group-hover:opacity-100 group-focus:opacity-100" />
                          </div>
                          <div className="text-[11px] text-[var(--text-secondary)] font-mono whitespace-nowrap overflow-hidden text-ellipsis opacity-80">
                            {snippet.content}
                          </div>
                        </button>
                    ))}
                </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
};
