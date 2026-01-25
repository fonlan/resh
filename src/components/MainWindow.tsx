import React, { useState } from 'react';
import { useConfig } from '../hooks/useConfig';
import { TerminalTab } from './TerminalTab';
import { WindowControls } from './WindowControls';

interface Tab {
  id: string;
  label: string;
  type: 'general' | 'local' | 'sync' | 'terminal';
  serverId?: string;
}

export const MainWindow: React.FC = () => {
  const { config, loading, error } = useConfig();
  const [currentTabId, setCurrentTabId] = useState<string>('general');
  const [draggedTabIndex, setDraggedTabIndex] = useState<number | null>(null);
  const [dropTargetIndex, setDropTargetIndex] = useState<number | null>(null);

  const [tabs, setTabs] = useState<Tab[]>([
    { id: 'general', label: 'General', type: 'general' },
    { id: 'local', label: 'Local Config', type: 'local' },
    { id: 'sync', label: 'Sync Config', type: 'sync' },
    { id: 'terminal', label: 'Terminal Test', type: 'terminal', serverId: 'loopback-server' },
  ]);

  const handleTabDragStart = (index: number) => {
    setDraggedTabIndex(index);
  };

  const handleTabDragOver = (e: React.DragEvent, index: number) => {
    e.preventDefault();
    if (draggedTabIndex !== null && draggedTabIndex !== index) {
      setDropTargetIndex(index);
    }
  };

  const handleTabDrop = (e: React.DragEvent, dropIndex: number) => {
    e.preventDefault();
    if (draggedTabIndex !== null && draggedTabIndex !== dropIndex) {
      const newTabs = [...tabs];
      const [draggedTab] = newTabs.splice(draggedTabIndex, 1);
      newTabs.splice(dropIndex, 0, draggedTab);
      setTabs(newTabs);
    }
    setDraggedTabIndex(null);
    setDropTargetIndex(null);
  };

  const handleTabDragEnd = () => {
    setDraggedTabIndex(null);
    setDropTargetIndex(null);
  };

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
        {tabs.map((tab, index) => (
          <button
            key={tab.id}
            draggable
            onDragStart={() => handleTabDragStart(index)}
            onDragOver={(e) => handleTabDragOver(e, index)}
            onDrop={(e) => handleTabDrop(e, index)}
            onDragEnd={handleTabDragEnd}
            className={`tab ${currentTabId === tab.id ? 'active' : ''} ${draggedTabIndex === index ? 'dragging' : ''} ${dropTargetIndex === index ? 'drop-target' : ''}`}
            onClick={() => setCurrentTabId(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Content Area */}
      <main className="content-area">
        {currentTabId === 'general' && (
          <div className="tab-content">
            <h2>General Settings</h2>
            <div className="config-preview">
              <h3>Merged Configuration</h3>
              <pre>{JSON.stringify(config, null, 2)}</pre>
            </div>
          </div>
        )}

        {currentTabId === 'local' && (
          <div className="tab-content">
            <h2>Local Configuration</h2>
            <p>Local settings (stored on this machine only)</p>
          </div>
        )}

        {currentTabId === 'sync' && (
          <div className="tab-content">
            <h2>Sync Configuration</h2>
            <p>Synchronized settings (shared across devices)</p>
          </div>
        )}

        <div style={{ display: currentTabId === 'terminal' ? 'block' : 'none', height: 'calc(100vh - 150px)' }}>
            <TerminalTab
                tabId="test"
                serverId="loopback-server"
                isActive={currentTabId === 'terminal'}
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
