import React, { useEffect, useRef } from 'react';
import { Edit2, Play } from 'lucide-react';
import { useTranslation } from '../i18n';

interface ServerContextMenuProps {
  x: number;
  y: number;
  serverId: string;
  onClose: () => void;
  onEdit: (serverId: string) => void;
  onConnect: (serverId: string) => void;
}

export const ServerContextMenu: React.FC<ServerContextMenuProps> = ({
  x,
  y,
  serverId,
  onClose,
  onEdit,
  onConnect,
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
      className="tab-context-menu"
      style={{ left: pos.left, top: pos.top }}
      onContextMenu={(e) => e.preventDefault()}
    >
      <button
        type="button"
        className="tab-context-menu-item"
        onClick={() => {
          onConnect(serverId);
          onClose();
        }}
      >
        <Play size={14} />
        <span>{t.serverTab.connectTooltip}</span>
      </button>

      <button
        type="button"
        className="tab-context-menu-item"
        onClick={() => {
          onEdit(serverId);
          onClose();
        }}
      >
        <Edit2 size={14} />
        <span>{t.serverTab.editTooltip}</span>
      </button>
    </div>
  );
};
