import React from 'react';
import { Server as ServerIcon, Plus, Terminal } from 'lucide-react';
import { Server } from '../types/config';
import { useTranslation } from '../i18n';
import './WelcomeScreen.css';

interface WelcomeScreenProps {
  servers: Server[];
  onServerClick: (serverId: string) => void;
  onOpenSettings: () => void;
  onServerContextMenu: (e: React.MouseEvent, serverId: string) => void;
}

export const WelcomeScreen: React.FC<WelcomeScreenProps> = ({
  servers,
  onServerClick,
  onOpenSettings,
  onServerContextMenu,
}) => {
  const { t } = useTranslation();
  const hasServers = servers.length > 0;

  return (
    <div className="welcome-screen">
      <div className="welcome-content">
        <div className="welcome-header">
          <div className="logo-container">
            <Terminal size={48} className="logo-icon" />
          </div>
          <h1 className="welcome-title">{t.welcome.title}</h1>
          <p className="welcome-subtitle">{t.welcome.subtitle}</p>
        </div>

        {hasServers ? (
          <div className="recent-section">
            <div className="section-header">
              <h2 className="recent-title">{t.welcome.recentTitle}</h2>
              <button type="button" className="btn-text" onClick={onOpenSettings}>
                {t.welcome.viewAll}
              </button>
            </div>
            <div className="server-grid">
              {servers.map((server) => (
                <button
                  type="button"
                  key={server.id}
                  className="server-card"
                  onClick={() => onServerClick(server.id)}
                  onContextMenu={(e) => onServerContextMenu(e, server.id)}
                >
                  <div className="server-card-icon">
                    <ServerIcon size={20} />
                  </div>
                  <div className="server-card-content">
                    <h3 className="server-card-name">{server.name}</h3>
                    <p className="server-card-info">
                      {server.username ? `${server.username}@` : ''}{server.host}
                    </p>
                  </div>
                </button>
              ))}
            </div>
          </div>
        ) : (
          <div className="empty-state">
            <div className="empty-state-icon">
              <ServerIcon size={32} />
            </div>
            <h3>{t.welcome.noServers}</h3>
            <p>{t.welcome.getFirstStarted}</p>
            <button type="button" className="btn-primary" onClick={onOpenSettings}>
              <Plus size={18} />
              <span>{t.serverTab.addServer}</span>
            </button>
          </div>
        )}
      </div>
    </div>
  );
};
