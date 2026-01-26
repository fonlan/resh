import React, { useState } from 'react';
import { SettingsModal } from './settings/SettingsModal';
import { TerminalTab } from './TerminalTab';
import { WindowControls } from './WindowControls';
import { WelcomeScreen } from './WelcomeScreen';
import { NewTabButton } from './NewTabButton';
import { useConfig } from '../hooks/useConfig';
import { generateId } from '../utils/idGenerator';
import { addRecentServer, getRecentServers } from '../utils/recentServers';

interface Tab {
  id: string;
  label: string;
  serverId: string;
}

export const MainWindow: React.FC = () => {
  const { config, saveConfig } = useConfig();
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [draggedTabIndex, setDraggedTabIndex] = useState<number | null>(null);
  const [dropTargetIndex, setDropTargetIndex] = useState<number | null>(null);

  const handleTabDragStart = (index: number) => {
    if (tabs.length <= 1) return;
    setDraggedTabIndex(index);
  };

  const handleTabDragOver = (e: React.DragEvent, index: number) => {
    e.preventDefault();
    if (draggedTabIndex !== null && draggedTabIndex !== index && dropTargetIndex !== index) {
      setDropTargetIndex(index);
    }
  };

  const handleTabDrop = (e: React.DragEvent, dropIndex: number) => {
    e.preventDefault();
    if (
      draggedTabIndex !== null &&
      draggedTabIndex !== dropIndex &&
      draggedTabIndex >= 0 &&
      draggedTabIndex < tabs.length &&
      dropIndex >= 0 &&
      dropIndex < tabs.length
    ) {
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

  const handleTabKeyDown = (e: React.KeyboardEvent, index: number) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'ArrowRight' && index < tabs.length - 1) {
      e.preventDefault();
      const newTabs = [...tabs];
      [newTabs[index], newTabs[index + 1]] = [newTabs[index + 1], newTabs[index]];
      setTabs(newTabs);
    } else if ((e.ctrlKey || e.metaKey) && e.key === 'ArrowLeft' && index > 0) {
      e.preventDefault();
      const newTabs = [...tabs];
      [newTabs[index], newTabs[index - 1]] = [newTabs[index - 1], newTabs[index]];
      setTabs(newTabs);
    }
  };

  const handleCloseTab = (tabId: string) => {
    const newTabs = tabs.filter(t => t.id !== tabId);
    setTabs(newTabs);
    if (activeTabId === tabId) {
      setActiveTabId(newTabs.length > 0 ? newTabs[0].id : null);
    }
  };

  const handleAddTab = async (serverId: string) => {
    const server = config?.servers.find(s => s.id === serverId);
    if (!server) return;

    const newTab: Tab = {
      id: generateId(),
      label: server.name,
      serverId: server.id
    };

    setTabs(prev => [...prev, newTab]);
    setActiveTabId(newTab.id);

    if (config) {
      const updatedGeneral = addRecentServer(config.general, serverId);
      await saveConfig(config, { ...config, general: updatedGeneral });
    }
  };

  const handleConnectServer = (serverId: string) => {
    handleAddTab(serverId);
    setIsSettingsOpen(false);
  };

  const recentServers = config ? getRecentServers(config.general.recentServerIds, config.servers, 3) : [];

  return (
    <div className="main-window">
      {/* Title Bar with drag region */}
      <div className="title-bar">
        {/* Tab Bar */}
        <div className="tab-bar" role="tablist">
          {tabs.map((tab, index) => (
            <button
              key={tab.id}
              draggable
              onDragStart={() => handleTabDragStart(index)}
              onDragOver={(e) => handleTabDragOver(e, index)}
              onDrop={(e) => handleTabDrop(e, index)}
              onDragEnd={handleTabDragEnd}
              onKeyDown={(e) => handleTabKeyDown(e, index)}
              role="tab"
              tabIndex={activeTabId === tab.id ? 0 : -1}
              aria-selected={activeTabId === tab.id}
              aria-label={`${tab.label} (Tab ${index + 1} of ${tabs.length})`}
              className={`tab ${activeTabId === tab.id ? 'active' : ''} ${
                draggedTabIndex === index ? 'dragging' : ''
              } ${dropTargetIndex === index ? 'drop-target' : ''}`}
              onClick={() => setActiveTabId(tab.id)}
            >
              <span className="tab-label">{tab.label}</span>
              <button
                className="tab-close"
                onClick={(e) => {
                  e.stopPropagation();
                  handleCloseTab(tab.id);
                }}
                aria-label="Close tab"
              >
                ×
              </button>
            </button>
          ))}
          <NewTabButton
            servers={config?.servers || []}
            onServerSelect={handleAddTab}
            onOpenSettings={() => setIsSettingsOpen(true)}
          />
        </div>

        {/* Drag region spacer - empty area for dragging */}
        <div className="drag-spacer" data-tauri-drag-region></div>

        {/* Right side: Settings button + Window controls */}
        <div className="title-bar-right">
          <button
            className="settings-btn"
            onClick={() => setIsSettingsOpen(true)}
            aria-label="Open settings"
            title="Settings"
          >
            ⚙️
          </button>
          <WindowControls />
        </div>
      </div>

      {/* Content Area */}
      <div className="content-area">
        {tabs.length === 0 ? (
          <WelcomeScreen
            servers={recentServers}
            onServerClick={handleAddTab}
            onOpenSettings={() => setIsSettingsOpen(true)}
          />
        ) : (
          tabs.map((tab) => {
            const server = config?.servers.find(s => s.id === tab.serverId);
            if (!server) return null;

            return (
              <div
                key={tab.id}
                style={{
                  display: activeTabId === tab.id ? 'block' : 'none',
                  height: '100%',
                }}
              >
                <TerminalTab
                  tabId={tab.id}
                  serverId={tab.serverId}
                  isActive={activeTabId === tab.id}
                  onClose={() => handleCloseTab(tab.id)}
                  server={server}
                  authentications={config?.authentications || []}
                  proxies={config?.proxies || []}
                />
              </div>
            );
          })
        )}
      </div>

      {/* Settings Modal */}
      <SettingsModal
        isOpen={isSettingsOpen}
        onClose={() => setIsSettingsOpen(false)}
        onConnectServer={handleConnectServer}
      />
    </div>
  );
};
