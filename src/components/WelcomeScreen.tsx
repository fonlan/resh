import React from 'react';
import { Server as ServerIcon, Plus, Terminal } from 'lucide-react';
import { Server } from '../types/config';
import './WelcomeScreen.css';

interface WelcomeScreenProps {
  servers: Server[];
  onServerClick: (serverId: string) => void;
  onOpenSettings: () => void;
}

export const WelcomeScreen: React.FC<WelcomeScreenProps> = ({
  servers,
  onServerClick,
  onOpenSettings,
}) => {
  const hasServers = servers.length > 0;

  return (
    <div className="welcome-screen">
      <div className="welcome-content">
        <div className="welcome-header">
          <div className="logo-container">
            <Terminal size={48} className="logo-icon" />
          </div>
          <h1 className="welcome-title">Welcome to Resh</h1>
          <p className="welcome-subtitle">A professional, high-performance SSH client.</p>
        </div>

        {hasServers ? (
          <div className="recent-section">
            <div className="section-header">
              <h2 className="recent-title">Recent Connections</h2>
              <button type="button" className="btn-text" onClick={onOpenSettings}>
                View All
              </button>
            </div>
            <div className="server-grid">
              {servers.map((server) => (
                <button
                  type="button"
                  key={server.id}
                  className="server-card"
                  onClick={() => onServerClick(server.id)}
                >
                  <div className="server-card-icon">
                    <ServerIcon size={20} />
                  </div>
                  <div className="server-card-content">
                    <h3 className="server-card-name">{server.name}</h3>
                    <p className="server-card-info">
                      {server.username}@{server.host}
                    </p>
                  </div>
                </button>
              ))}
              <button
                type="button"
                className="server-card add-card"
                onClick={onOpenSettings}
              >
                <div className="server-card-icon">
                  <Plus size={20} />
                </div>
                <div className="server-card-content">
                  <h3 className="server-card-name">New Connection</h3>
                  <p className="server-card-info">Configure a new server</p>
                </div>
              </button>
            </div>
          </div>
        ) : (
          <div className="empty-state">
            <div className="empty-state-icon">
              <ServerIcon size={32} />
            </div>
            <h3>No servers configured</h3>
            <p>Add your first server to get started with Resh.</p>
            <button type="button" className="btn-primary" onClick={onOpenSettings}>
              <Plus size={18} />
              <span>Add Server</span>
            </button>
          </div>
        )}
      </div>
    </div>
  );
};
