import React, { useEffect, useRef } from 'react';
import { Copy, X, XCircle, FileDown, Circle, Square, RefreshCw } from 'lucide-react';
import { useTranslation } from '../i18n';

interface TabContextMenuProps {
  x: number;
  y: number;
  tabId: string;
  isRecording: boolean;
  onClose: () => void;
  onClone: (tabId: string) => void;
  onReconnect: (tabId: string) => void;
  onExport: (tabId: string) => void;
  onStartRecording: (tabId: string) => void;
  onStopRecording: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onCloseOthers: (tabId: string) => void;
}

export const TabContextMenu: React.FC<TabContextMenuProps> = ({
  x,
  y,
  tabId,
  isRecording,
  onClose,
  onClone,
  onReconnect,
  onExport,
  onStartRecording,
  onStopRecording,
  onCloseTab,
  onCloseOthers,
}) => {
  const { t } = useTranslation();
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        onClose();
      }
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose();
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [onClose]);

  // Adjust position if menu goes off screen
  const adjustPosition = () => {
    if (!menuRef.current) return { left: x, top: y };
    
    const menuRect = menuRef.current.getBoundingClientRect();
    const screenWidth = window.innerWidth;
    const screenHeight = window.innerHeight;
    
    let left = x;
    let top = y;
    
    if (x + menuRect.width > screenWidth) {
      left = screenWidth - menuRect.width - 5;
    }
    
    if (y + menuRect.height > screenHeight) {
      top = screenHeight - menuRect.height - 5;
    }
    
    return { left, top };
  };

  const pos = adjustPosition();

  return (
    <div
      ref={menuRef}
      className="fixed z-[1000] bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded-[var(--radius-sm)] shadow-[0_10px_25px_-5px_rgba(0,0,0,0.3),0_8px_10px_-6px_rgba(0,0,0,0.3)] p-1 min-w-[180px] backdrop-blur-[12px] animate-[contextMenuFadeIn_0.15s_ease-out]"
      style={{ 
        left: pos.left, 
        top: pos.top,
        animation: 'contextMenuFadeIn 0.15s ease-out'
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      <style>{`
        @keyframes contextMenuFadeIn {
          from { opacity: 0; transform: scale(0.95); }
          to { opacity: 1; transform: scale(1); }
        }
      `}</style>
      <button
        type="button"
        className="w-full flex items-center gap-2.5 p-[8px_12px] bg-transparent border-none rounded-[4px] text-[var(--text-primary)] text-[13px] font-[var(--font-ui)] cursor-pointer transition-all text-left hover:bg-[var(--accent-primary)] hover:text-white"
        onClick={() => {
          onClone(tabId);
          onClose();
        }}
      >
        <Copy size={14} />
        <span>{t.mainWindow.cloneTab}</span>
      </button>

      <button
        type="button"
        className="w-full flex items-center gap-2.5 p-[8px_12px] bg-transparent border-none rounded-[4px] text-[var(--text-primary)] text-[13px] font-[var(--font-ui)] cursor-pointer transition-all text-left hover:bg-[var(--accent-primary)] hover:text-white"
        onClick={() => {
          onReconnect(tabId);
          onClose();
        }}
      >
        <RefreshCw size={14} />
        <span>{t.mainWindow.reconnect}</span>
      </button>

      <button
        type="button"
        className="w-full flex items-center gap-2.5 p-[8px_12px] bg-transparent border-none rounded-[4px] text-[var(--text-primary)] text-[13px] font-[var(--font-ui)] cursor-pointer transition-all text-left hover:bg-[var(--accent-primary)] hover:text-white"
        onClick={() => {
          onExport(tabId);
          onClose();
        }}
      >
        <FileDown size={14} />
        <span>{t.mainWindow.exportLogs}</span>
      </button>

      {isRecording ? (
        <button
          type="button"
          className="w-full flex items-center gap-2.5 p-[8px_12px] bg-transparent border-none rounded-[4px] text-[var(--text-primary)] text-[13px] font-[var(--font-ui)] cursor-pointer transition-all text-left hover:bg-[var(--accent-primary)] hover:text-white"
          onClick={() => {
            onStopRecording(tabId);
            onClose();
          }}
        >
          <Square size={14} fill="currentColor" className="text-red-500" />
          <span>{t.mainWindow.stopRecording}</span>
        </button>
      ) : (
        <button
          type="button"
          className="w-full flex items-center gap-2.5 p-[8px_12px] bg-transparent border-none rounded-[4px] text-[var(--text-primary)] text-[13px] font-[var(--font-ui)] cursor-pointer transition-all text-left hover:bg-[var(--accent-primary)] hover:text-white"
          onClick={() => {
            onStartRecording(tabId);
            onClose();
          }}
        >
          <Circle size={14} fill="currentColor" className="text-red-500" />
          <span>{t.mainWindow.startRecording}</span>
        </button>
      )}

      <div className="h-px bg-[var(--glass-border)] m-1" />
      
      <button
        type="button"
        className="w-full flex items-center gap-2.5 p-[8px_12px] bg-transparent border-none rounded-[4px] text-[var(--text-primary)] text-[13px] font-[var(--font-ui)] cursor-pointer transition-all text-left hover:bg-[var(--accent-primary)] hover:text-white"
        onClick={() => {
          onCloseTab(tabId);
          onClose();
        }}
      >
        <X size={14} />
        <span>{t.mainWindow.closeTab}</span>
      </button>
      
      <button
        type="button"
        className="w-full flex items-center gap-2.5 p-[8px_12px] bg-transparent border-none rounded-[4px] text-[var(--text-primary)] text-[13px] font-[var(--font-ui)] cursor-pointer transition-all text-left hover:bg-[var(--accent-primary)] hover:text-white"
        onClick={() => {
          onCloseOthers(tabId);
          onClose();
        }}
      >
        <XCircle size={14} />
        <span>{t.mainWindow.closeOthers}</span>
      </button>
    </div>
  );
};
