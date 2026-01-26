import React, { useState, useEffect, useRef } from 'react';
import { Plus, Settings, Server as ServerIcon } from 'lucide-react';
import { Server } from '../types/config';
import './NewTabButton.css';

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
  const [isOpen, setIsOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

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

  const handleServerClick = (serverId: string) => {
    onServerSelect(serverId);
    setIsOpen(false);
  };

  const handleSettingsClick = () => {
    onOpenSettings();
    setIsOpen(false);
  };

  return (
    <div className="new-tab-container">
      <button
        type="button"
        ref={buttonRef}
        className="new-tab-btn"
        onClick={() => setIsOpen(!isOpen)}
        aria-label="New connection"
        title="New connection"
      >
        <Plus size={16} />
      </button>

      {isOpen && (
        <div ref={menuRef} className="new-tab-dropdown">
          <div className="dropdown-header">
            <span>Connect to Server</span>
          </div>

          {servers.length === 0 ? (
            <div className="dropdown-empty">
              <ServerIcon size={32} />
              <span>No servers configured</span>
              <button
                type="button"
                className="dropdown-settings-btn"
                onClick={handleSettingsClick}
              >
                <Plus size={14} />
                Add Server
              </button>
            </div>
          ) : (
            <div className="dropdown-list">
              {servers.map((server) => (
                <button
                  type="button"
                  key={server.id}
                  className="dropdown-item"
                  onClick={() => handleServerClick(server.id)}
                >
                  <ServerIcon size={18} />
                  <div className="dropdown-item-content">
                    <span className="dropdown-item-name">{server.name}</span>
                    <span className="dropdown-item-info">
                      {server.username}@{server.host}
                    </span>
                  </div>
                </button>
              ))}
            </div>
          )}

          <div className="dropdown-footer">
            <button
              type="button"
              className="dropdown-settings-link"
              onClick={handleSettingsClick}
            >
              <Settings size={14} />
              Manage Servers
            </button>
          </div>
        </div>
      )}
    </div>
  );
};
