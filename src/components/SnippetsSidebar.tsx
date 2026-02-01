import React, { useState, useEffect, useRef, useMemo } from 'react';
import { Snippet } from '../types/config';
import { X, Code, Play, ChevronRight, ChevronDown, Plus, Lock, LockOpen } from 'lucide-react';
import { useTranslation } from '../i18n';
import './SnippetsSidebar.css';

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
      className={`snippets-sidebar-panel ${isOpen ? 'open' : ''} ${isResizing ? 'resizing' : ''} ${isLocked ? 'locked' : ''}`}
      aria-hidden={!isOpen}
      style={{ width: isOpen ? `${width}px` : '0px' }}
    >
      <div 
        className="snippets-resizer" 
        onMouseDown={startResizing}
        role="separator"
        aria-orientation="vertical"
        aria-valuenow={width}
        aria-valuemin={200}
        aria-valuemax={600}
        aria-label="Resize Sidebar"
        tabIndex={0}
      />
      
      <div className="snippets-header">
        <h3 className="snippets-title">
          <Code size={16} /> {t.snippetsTab.title}
        </h3>
        <div className="snippets-actions">
          <button
            type="button"
            onClick={onToggleLock}
            className={`snippets-close-btn ${isLocked ? 'text-accent-primary' : ''}`}
            aria-label={isLocked ? "Unlock Sidebar" : "Lock Sidebar"}
            title={isLocked ? "Unlock Sidebar" : "Lock Sidebar"}
          >
            {isLocked ? <Lock size={16} /> : <LockOpen size={16} />}
          </button>
          {onOpenSettings && (
            <button
              type="button"
              onClick={onOpenSettings}
              className="snippets-close-btn"
              aria-label={t.mainWindow.settings}
              title={t.mainWindow.settings}
            >
              <Plus size={16} />
            </button>
          )}
          <button 
              type="button"
              onClick={onClose} 
              className="snippets-close-btn"
              aria-label={t.windowControls.close}
          >
            <X size={16} />
          </button>
        </div>
      </div>
      
      <div className="snippets-list">
        {snippets.length === 0 ? (
           <p className="snippets-empty">{t.snippetsTab.emptyState}</p>
        ) : (
          groupNames.map(groupName => (
            <div key={groupName} className="snippet-group">
                <button 
                    type="button"
                    className="snippet-group-header"
                    onClick={() => toggleGroup(groupName)}
                    aria-expanded={!!expandedGroups[groupName]}
                >
                    {expandedGroups[groupName] ? (
                        <ChevronDown size={14} className="snippet-group-icon" />
                    ) : (
                        <ChevronRight size={14} className="snippet-group-icon" />
                    )}
                    <span className="snippet-group-label">{groupName}</span>
                </button>
                
                <div className={`snippet-group-content ${!expandedGroups[groupName] ? 'collapsed' : ''}`}>
                    {groupedSnippets[groupName].map(snippet => (
                        <button 
                          key={snippet.id}
                          type="button"
                          className="snippet-item group w-full text-left"
                          onClick={() => handleSnippetClick(snippet.content)}
                          onKeyDown={(e) => handleKeyDown(e, snippet.content)}
                          title={snippet.description}
                          aria-label={t.common.actions}
                        >
                          <div className="snippet-item-header">
                            <span className="snippet-name">{snippet.name}</span>
                            <Play size={10} className="snippet-play-icon" />
                          </div>
                          <div className="snippet-content">
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
