import React, { useState } from 'react';
import { useConfig } from '../hooks/useConfig';
import { TerminalTab } from './TerminalTab';
import { WindowControls } from './WindowControls';

export const MainWindow: React.FC = () => {
  const { config, loading, error } = useConfig();
  const [currentTab, setCurrentTab] = useState<'general' | 'local' | 'sync' | 'terminal'>('general');

  if (loading) {
    return (
      <div className="loading-container">
        <div className="loading-spinner">Loading configuration...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="error-container">
        <div className="error-message">
          <h2>Error loading configuration</h2>
          <p>{error}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="main-window">
      {/* Header with drag region and window controls */}
      <header className="app-header" data-tauri-drag-region>
        <h1>Resh Configuration Manager</h1>
        <div className="header-actions">
          <button className="btn btn-secondary">Reload</button>
          <button className="btn btn-primary">Save</button>
        </div>
        <WindowControls />
      </header>

      {/* Tab Navigation */}
      <div className="tab-navigation">
        <button
          className={`tab ${currentTab === 'general' ? 'active' : ''}`}
          onClick={() => setCurrentTab('general')}
        >
          General
        </button>
        <button
          className={`tab ${currentTab === 'local' ? 'active' : ''}`}
          onClick={() => setCurrentTab('local')}
        >
          Local Config
        </button>
        <button
          className={`tab ${currentTab === 'sync' ? 'active' : ''}`}
          onClick={() => setCurrentTab('sync')}
        >
          Sync Config
        </button>
        <button
          className={`tab ${currentTab === 'terminal' ? 'active' : ''}`}
          onClick={() => setCurrentTab('terminal')}
        >
          Terminal Test
        </button>
      </div>

      {/* Content Area */}
      <main className="content-area">
        {currentTab === 'general' && (
          <div className="tab-content">
            <h2>General Settings</h2>
            <div className="config-preview">
              <h3>Merged Configuration</h3>
              <pre>{JSON.stringify(config, null, 2)}</pre>
            </div>
          </div>
        )}

        {currentTab === 'local' && (
          <div className="tab-content">
            <h2>Local Configuration</h2>
            <p>Local settings (stored on this machine only)</p>
          </div>
        )}

        {currentTab === 'sync' && (
          <div className="tab-content">
            <h2>Sync Configuration</h2>
            <p>Synchronized settings (shared across devices)</p>
          </div>
        )}
        
        <div style={{ display: currentTab === 'terminal' ? 'block' : 'none', height: 'calc(100vh - 150px)' }}>
            <TerminalTab 
                tabId="test" 
                serverId="loopback-server" 
                isActive={currentTab === 'terminal'} 
                onClose={() => {}}
            />
        </div>
      </main>

      {/* Footer */}
      <footer className="app-footer">
        <div className="footer-info">
          <span>Resh v0.1.0</span>
          <span>•</span>
          <span>Config loaded successfully</span>
        </div>
      </footer>
    </div>
  );
};
