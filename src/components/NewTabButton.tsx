import React, { useState, useEffect, useRef } from 'react';
import { Server } from '../types/config';
import './NewTabButton.css';

interface NewTabButtonProps {
  servers: Server[];
  onServerSelect: (serverId: string) => void;
  onOpenSettings: () => void;
}

// Plus icon
const PlusIcon: React.FC = () => (
  <svg
    className="new-tab-icon"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <line x1="12" y1="5" x2="12" y2="19"></line>
    <line x1="5" y1="12" x2="19" y2="12"></line>
  </svg>
);

// Settings icon
const SettingsIcon: React.FC = () => (
  <svg
    className="dropdown-icon"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <circle cx="12" cy="12" r="3"></circle>
    <path d="M12 1v6m0 6v6M4.22 4.22l4.24 4.24m5.08 5.08l4.24 4.24M1 12h6m6 0h6M4.22 19.78l4.24-4.24m5.08-5.08l4.24-4.24"></path>
  </svg>
);

// Server icon
const ServerSmallIcon: React.FC = () => (
  <svg
    className="server-small-icon"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <rect x="2" y="2" width="20" height="8" rx="2" ry="2"></rect>
    <rect x="2" y="14" width="20" height="8" rx="2" ry="2"></rect>
    <line x1="6" y1="6" x2="6.01" y2="6"></line>
    <line x1="6" y1="18" x2="6.01" y2="18"></line>
  </svg>
);

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
        ref={buttonRef}
        className="new-tab-btn"
        onClick={() => setIsOpen(!isOpen)}
        aria-label="New connection"
        title="New connection"
      >
        <PlusIcon />
      </button>

      {isOpen && (
        <div ref={menuRef} className="new-tab-dropdown">
          <div className="dropdown-header">
            <span>Connect to Server</span>
          </div>

          {servers.length === 0 ? (
            <div className="dropdown-empty">
              <ServerSmallIcon />
              <span>No servers configured</span>
              <button className="dropdown-settings-btn" onClick={handleSettingsClick}>
                <SettingsIcon />
                Add Server
              </button>
            </div>
          ) : (
            <div className="dropdown-list">
              {servers.map((server) => (
                <button
                  key={server.id}
                  className="dropdown-item"
                  onClick={() => handleServerClick(server.id)}
                >
                  <ServerSmallIcon />
                  <div className="dropdown-item-content">
                    <span className="dropdown-item-name">{server.name}</span>
                    <span className="dropdown-item-info">
                      {server.username}@{server.host}:{server.port}
                    </span>
                  </div>
                </button>
              ))}
            </div>
          )}

          {servers.length > 0 && (
            <div className="dropdown-footer">
              <button className="dropdown-settings-link" onClick={handleSettingsClick}>
                <SettingsIcon />
                Manage Servers
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
};
