import React from 'react';
import { Server } from '../types/config';
import './WelcomeScreen.css';

interface WelcomeScreenProps {
  servers: Server[];
  onServerClick: (serverId: string) => void;
  onOpenSettings: () => void;
}

// Plus icon
const PlusIcon: React.FC = () => (
  <svg
    className="plus-icon"
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

// Server icon
const ServerIcon: React.FC = () => (
  <svg
    className="server-icon"
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

export const WelcomeScreen: React.FC<WelcomeScreenProps> = ({
  servers,
  onServerClick,
  onOpenSettings,
}) => {
  const hasServers = servers.length > 0;

  return (
    <div className="welcome-screen">
      <div className="welcome-content">
        <h1 className="welcome-title">Welcome to Resh</h1>
        <p className="welcome-subtitle">SSH Terminal Client</p>

        {hasServers ? (
          <>
            <div className="recent-section">
              <h2 className="recent-title">Recent Servers</h2>
              <div className="server-grid">
                {servers.map((server) => (
                  <button
                    key={server.id}
                    className="server-card"
                    onClick={() => onServerClick(server.id)}
                  >
                    <div className="server-card-icon">
                      <ServerIcon />
                    </div>
                    <div className="server-card-content">
                      <h3 className="server-card-name">{server.name}</h3>
                      <p className="server-card-info">
                        {server.username}@{server.host}:{server.port}
                      </p>
                    </div>
                  </button>
                ))}
              </div>
            </div>
          </>
        ) : (
          <div className="empty-state">
            <div className="empty-state-icon">
              <ServerIcon />
            </div>
            <h3>No servers configured</h3>
            <p>Add your first server to get started</p>
            <button className="btn-primary" onClick={onOpenSettings}>
              <PlusIcon />
              <span>Add Server</span>
            </button>
          </div>
        )}

        <div className="welcome-actions">
          <button className="btn-secondary" onClick={onOpenSettings}>
            Settings
          </button>
        </div>
      </div>
    </div>
  );
};
