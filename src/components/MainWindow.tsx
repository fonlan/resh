import React, { useState } from 'react';
import { SettingsModal } from './settings/SettingsModal';
import { TerminalTab } from './TerminalTab';
import { WindowControls } from './WindowControls';

interface Tab {
  id: string;
  label: string;
  serverId: string;
}

export const MainWindow: React.FC = () => {
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

  return (
    <div className="main-window">
      {/* Title Bar with drag region */}
      <div className="title-bar" data-tauri-drag-region>
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
              data-tauri-drag-region="false"
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
        </div>

        {/* Right side: Settings button + Window controls */}
        <div className="title-bar-right">
          <button
            className="settings-btn"
            onClick={() => setIsSettingsOpen(true)}
            aria-label="Open settings"
            title="Settings"
            data-tauri-drag-region="false"
          >
            ⚙️
          </button>
          <WindowControls />
        </div>
      </div>

      {/* Content Area */}
      <div className="content-area">
        {tabs.length === 0 ? (
          <div className="welcome-screen">
            <div className="welcome-content">
              <h1>Welcome to Resh</h1>
              <p>SSH Terminal Client</p>
              <button
                className="btn-primary"
                onClick={() => setIsSettingsOpen(true)}
              >
                Open Settings
              </button>
            </div>
          </div>
        ) : (
          tabs.map((tab) => (
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
              />
            </div>
          ))
        )}
      </div>

      {/* Settings Modal */}
      <SettingsModal
        isOpen={isSettingsOpen}
        onClose={() => setIsSettingsOpen(false)}
      />
    </div>
  );
};
