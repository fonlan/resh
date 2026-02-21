import React, { useState, useEffect, useRef, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { Plus, Settings, Server as ServerIcon } from 'lucide-react';
import { Server } from '../types';
import { useTranslation } from '../i18n';
import { EmojiText } from './EmojiText';

interface NewTabButtonProps {
  servers: Server[];
  onServerSelect: (serverId: string) => void;
  onOpenSettings: () => void;
}

export const NewTabButton: React.FC<NewTabButtonProps> = ({
  servers,
  onServerSelect,
  onOpenSettings,
}) => {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const [menuPosition, setMenuPosition] = useState<{ top: number; left: number } | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

  const updateMenuPosition = useCallback(() => {
    if (!buttonRef.current) {
      return;
    }

    const rect = buttonRef.current.getBoundingClientRect();
    const menuWidth = 360;
    const estimatedMenuHeight = 420;
    const left = Math.min(Math.max(rect.left, 8), window.innerWidth - menuWidth - 8);
    const canOpenDownward = rect.bottom + 10 + estimatedMenuHeight <= window.innerHeight - 8;
    const top = canOpenDownward
      ? rect.bottom + 10
      : Math.max(8, rect.top - estimatedMenuHeight - 10);

    setMenuPosition({ top, left });
  }, []);

  // Close menu when clicking outside
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        menuRef.current &&
        !menuRef.current.contains(event.target as Node) &&
        buttonRef.current &&
        !buttonRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, []);

  useEffect(() => {
    if (!isOpen) {
      setMenuPosition(null);
      return;
    }

    updateMenuPosition();
    window.addEventListener('resize', updateMenuPosition);
    window.addEventListener('scroll', updateMenuPosition, true);

    return () => {
      window.removeEventListener('resize', updateMenuPosition);
      window.removeEventListener('scroll', updateMenuPosition, true);
    };
  }, [isOpen, updateMenuPosition]);

  const handleServerClick = (serverId: string) => {
    onServerSelect(serverId);
    setIsOpen(false);
  };

  const handleSettingsClick = () => {
    onOpenSettings();
    setIsOpen(false);
  };

  const sortedServers = [...servers].sort((a, b) => a.name.localeCompare(b.name));

  const menu =
    isOpen && menuPosition
      ? createPortal(
          <div
            ref={menuRef}
            className="fixed min-w-[260px] max-w-[360px] bg-[var(--bg-secondary)] border border-[var(--glass-border)] rounded-[var(--radius-lg)] shadow-[0_20px_40px_rgba(0,0,0,0.5),0_0_0_1px_var(--glass-border),inset_0_1px_0_rgba(255,255,255,0.05)] flex flex-col overflow-hidden z-[1000] animate-[dropdownReveal_0.3s_cubic-bezier(0.16,1,0.3,1)] backdrop-blur-[16px]"
            style={{
              top: menuPosition.top,
              left: menuPosition.left,
              animation: 'dropdownReveal 0.3s cubic-bezier(0.16, 1, 0.3, 1)'
            }}
          >
            <style>{`
              @keyframes dropdownReveal {
                from { opacity: 0; transform: translateY(-8px) scale(0.95); }
                to { opacity: 1; transform: translateY(0) scale(1); }
              }
            `}</style>
            <div className="p-[14px_20px] border-b border-[var(--glass-border)] text-[11px] font-bold text-[var(--text-muted)] uppercase  bg-[rgba(255,255,255,0.02)]">
              <span>{t.newTabButton.connectTo}</span>
            </div>

            {sortedServers.length === 0 ? (
              <div className="p-[32px_24px] flex flex-col items-center gap-3 text-center text-[var(--text-secondary)]">
                <ServerIcon size={32} />
                <span>{t.welcome.noServers}</span>
                <button
                  type="button"
                  className="inline-flex items-center gap-2 px-4 py-2 bg-[var(--accent-primary)] text-white border-none rounded-[var(--radius-sm)] text-[13px] font-semibold cursor-pointer transition-all mt-2 hover:brightness-110"
                  onClick={handleSettingsClick}
                >
                  <Plus size={14} />
                  {t.serverTab.addServer}
                </button>
              </div>
            ) : (
              <div className="p-1.5 overflow-y-auto max-h-[400px]">
                {sortedServers.map((server) => (
                  <button
                    type="button"
                    key={server.id}
                    className="w-full flex items-center gap-[14px] p-[12px_16px] bg-transparent border-none rounded-[var(--radius-md)] cursor-pointer transition-all duration-200 text-left text-[var(--text-secondary)] my-0.5 hover:bg-[rgba(255,255,255,0.05)] hover:text-[var(--text-primary)] hover:translate-x-1 group"
                    onClick={() => handleServerClick(server.id)}
                  >
                    <ServerIcon size={18} className="text-[var(--accent-primary)] opacity-60 transition-all duration-200 group-hover:opacity-100 group-hover:scale-110" />
                    <div className="flex flex-col min-w-0 flex-1">
                      <span className="text-[14px] font-bold text-[var(--text-primary)] whitespace-nowrap overflow-hidden text-ellipsis">
                        <EmojiText text={server.name} />
                      </span>
                      <span className="text-[11px] text-[var(--text-muted)] font-mono opacity-70 whitespace-nowrap overflow-hidden text-ellipsis">
                        {server.username ? `${server.username}@` : ''}{server.host}
                      </span>
                    </div>
                  </button>
                ))}
              </div>
            )}

            <div className="p-1.5 border-t border-[var(--glass-border)] bg-[var(--bg-tertiary)]">
              <button
                type="button"
                className="w-full flex items-center justify-center gap-2 p-2 bg-transparent border-none rounded-[var(--radius-sm)] text-[var(--text-secondary)] text-[13px] font-medium cursor-pointer transition-all hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)]"
                onClick={handleSettingsClick}
              >
                <Settings size={14} />
                {t.newTabButton.manageServers}
              </button>
            </div>
          </div>,
          document.body
        )
      : null;

  return (
    <>
      <div className="relative flex items-center shrink-0">
        <button
          type="button"
          ref={buttonRef}
          className="h-10 w-10 flex items-center justify-center bg-transparent border-none rounded-none text-[var(--text-secondary)] cursor-pointer transition-all shrink-0 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
          onClick={() => {
            setIsOpen(prev => {
              const next = !prev;
              if (next) {
                updateMenuPosition();
              }
              return next;
            });
          }}
          aria-label={t.welcome.newConnection}
          title={t.welcome.newConnection}
          data-tauri-drag-region="false"
        >
          <Plus size={16} />
        </button>
      </div>
      {menu}
    </>
  );
};
