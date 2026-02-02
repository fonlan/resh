import React, { useState, useCallback, useEffect, Suspense } from 'react';
import { Settings, X, Code, Circle, MessageSquare } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Config } from '../types/config';
// SettingsModal is now lazy loaded
const SettingsModal = React.lazy(() => 
  import('./settings/SettingsModal').then(module => ({ default: module.SettingsModal }))
);
import { TerminalTab } from './TerminalTab';
import { WindowControls } from './WindowControls';
import { WelcomeScreen } from './WelcomeScreen';
import { NewTabButton } from './NewTabButton';
import { SnippetsSidebar } from './SnippetsSidebar';
import { AISidebar } from './AISidebar';
import { TabContextMenu } from './TabContextMenu';
import { ToastContainer, ToastItem } from './Toast';
import { useConfig } from '../hooks/useConfig';
import { generateId } from '../utils/idGenerator';
import { addRecentServer, getRecentServers } from '../utils/recentServers';
import { useTranslation } from '../i18n';
import { useTabDragDrop } from '../hooks/useTabDragDrop';

interface Tab {
  id: string;
  label: string;
  serverId: string;
}

export const MainWindow: React.FC = () => {
  const { config, saveConfig } = useConfig();
  const { t } = useTranslation();
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [settingsInitialTab, setSettingsInitialTab] = useState<'servers' | 'auth' | 'proxies' | 'snippets' | 'general'>('servers');
  const [isSnippetsOpen, setIsSnippetsOpen] = useState(false);
  const [isAIOpen, setIsAIOpen] = useState(false);
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  // Listen for sync failed events
  useEffect(() => {
    let isMounted = true;

    const syncFailedListener = listen<string>('sync-failed', (event) => {
      if (isMounted) {
        const id = generateId();
        setToasts(prev => [...prev, { id, type: 'error', message: `同步失败: ${event.payload}` }]);
      }
    });

    return () => {
      isMounted = false;
      syncFailedListener.then(unlisten => unlisten());
    };
  }, []);

  const removeToast = (id: string) => {
    setToasts(prev => prev.filter(t => t.id !== id));
  };

  // Sync locked sidebar state from config on initial load
  React.useEffect(() => {
    if (config?.general) {
      if (config.general.aiSidebarLocked) {
        setIsAIOpen(true);
      }
      if (config.general.snippetsSidebarLocked) {
        setIsSnippetsOpen(true);
      }
    }
  }, [config?.general]);

  const [contextMenu, setContextMenu] = useState<{ x: number, y: number, tabId: string } | null>(null);
  const [recordingTabs, setRecordingTabs] = useState<Set<string>>(new Set());
  const [tabSessions, setTabSessions] = useState<Record<string, string>>({}); // tabId -> sessionId

  const {
    draggedIndex: draggedTabIndex,
    dropTargetIndex,
    handleDragStart: handleTabDragStart,
    handleDragOver: handleTabDragOver,
    handleDrop: handleTabDrop,
    handleDragEnd: handleTabDragEnd
  } = useTabDragDrop(tabs, setTabs);

  const handleTabKeyDown = useCallback((e: React.KeyboardEvent, index: number) => {
    if ((e.ctrlKey || e.metaKey)) {
        if (e.key === 'ArrowRight' && index < tabs.length - 1) {
            e.preventDefault();
            const newTabs = [...tabs];
            [newTabs[index], newTabs[index + 1]] = [newTabs[index + 1], newTabs[index]];
            setTabs(newTabs);
        } else if (e.key === 'ArrowLeft' && index > 0) {
            e.preventDefault();
            const newTabs = [...tabs];
            [newTabs[index], newTabs[index - 1]] = [newTabs[index - 1], newTabs[index]];
            setTabs(newTabs);
        }
    }
  }, [tabs]);

  const handleCloseTab = useCallback((tabId: string) => {
    const newTabs = tabs.filter(t => t.id !== tabId);
    setTabs(newTabs);
    if (activeTabId === tabId) {
      setActiveTabId(newTabs.length > 0 ? newTabs[0].id : null);
    }
    setTabSessions(prev => {
      const next = { ...prev };
      delete next[tabId];
      return next;
    });
  }, [tabs, activeTabId]);

  const handleAddTab = useCallback(async (serverId: string) => {
    // Reload config to ensure we have the latest (especially if coming from SettingsModal)
    // and to avoid using a stale 'config' object from the hook's state.
    const currentConfig = await invoke<Config>('get_config');
    const server = currentConfig.servers.find(s => s.id === serverId);
    
    if (!server) {
      return;
    }

    const newTab: Tab = {
      id: generateId(),
      label: server.name,
      serverId: server.id
    };

    setTabs(prev => [...prev, newTab]);
    setActiveTabId(newTab.id);

    // Update recent servers using the latest config
    const updatedGeneral = addRecentServer(currentConfig.general, serverId);
    await saveConfig({ ...currentConfig, general: updatedGeneral });
  }, [saveConfig]);

  const handleCloneTab = useCallback((tabId: string) => {
    const sourceTab = tabs.find(t => t.id === tabId);
    if (!sourceTab) return;

    const sourceIndex = tabs.findIndex(t => t.id === tabId);
    const newTab: Tab = {
      id: generateId(),
      label: sourceTab.label,
      serverId: sourceTab.serverId
    };

    const newTabs = [...tabs];
    newTabs.splice(sourceIndex + 1, 0, newTab);
    setTabs(newTabs);
    setActiveTabId(newTab.id);
  }, [tabs]);

  const handleCloseOthers = useCallback((tabId: string) => {
    const newTabs = tabs.filter(t => t.id === tabId);
    setTabs(newTabs);
    setActiveTabId(tabId);
  }, [tabs]);

  const handleExportLogs = useCallback((tabId: string) => {
    const event = new CustomEvent(`export-terminal-logs:${tabId}`);
    window.dispatchEvent(event);
  }, []);

  const handleStartRecording = useCallback(async (tabId: string) => {
    // Check if the tab corresponds to an active session
    // Since session_id is currently not directly mapped in Tab interface (it might be managed inside TerminalTab),
    // we need to rely on the fact that for now, let's assume tabId is the sessionId or we can get it.
    // Wait, TerminalTab generates the session ID or receives it.
    // Looking at TerminalTab usage: <TerminalTab tabId={tab.id} ... />
    // And in connection.rs: connect_to_server returns a session_id.
    // MainWindow doesn't know the session_id directly.
    // However, the TerminalTab can listen for an event or expose a ref.
    // OR, we can assume tabId IS the sessionId if we structured it that way, but we generateId() for tabs.
    
    // We need a way to map tabId to sessionId.
    // Option: Dispatch an event to the specific TerminalTab to initiate the "Start Recording" process?
    // Or simpler: TerminalTab listens for a global event/context.
    
    // Let's dispatch a custom event that TerminalTab listens to.
    // When TerminalTab receives "start-recording:<tabId>", it calls the backend.
    // But MainWindow needs to know the file path first? No, TerminalTab can handle the UI flow too?
    // No, context menu is in MainWindow.
    
    // Better: MainWindow asks the user for the path, then tells TerminalTab "Start recording to <path>".
    // TerminalTab knows its session_id.
    
    const tab = tabs.find(t => t.id === tabId);
    let defaultName = `recording-${tabId}.txt`;
    if (tab && config) {
      const server = config.servers.find(s => s.id === tab.serverId);
      if (server) {
         defaultName = `recording-${server.host.replace(/[^a-z0-9]/gi, '_')}-${new Date().toISOString().replace(/[:.]/g, '-')}.txt`;
      }
    }

    try {
      const path = await invoke<string | null>('select_save_path', { defaultName });
      if (path) {
        // Dispatch event to TerminalTab
        window.dispatchEvent(new CustomEvent(`start-recording:${tabId}`, { detail: { path } }));
        setRecordingTabs(prev => new Set(prev).add(tabId));
      }
    } catch (error) {
      // Failed to select save path
    }
  }, [tabs, config]);

  const handleStopRecording = useCallback((tabId: string) => {
    window.dispatchEvent(new CustomEvent(`stop-recording:${tabId}`));
    setRecordingTabs(prev => {
      const next = new Set(prev);
      next.delete(tabId);
      return next;
    });
  }, []);

  const handleContextMenu = useCallback((e: React.MouseEvent, tabId: string) => {
    e.preventDefault();
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      tabId
    });
  }, []);

  const handleConnectServer = useCallback((serverId: string) => {
    handleAddTab(serverId);
    setIsSettingsOpen(false);
  }, [handleAddTab]);

  const handleOpenSettings = useCallback((tab: 'servers' | 'auth' | 'proxies' | 'snippets' | 'general' = 'servers') => {
    setSettingsInitialTab(tab);
    setIsSettingsOpen(true);
  }, []);

  const handleToggleSnippetsLock = useCallback(async () => {
    if (!config) return;
    const currentLocked = config.general.snippetsSidebarLocked;
    const newConfig = {
      ...config,
      general: {
        ...config.general,
        snippetsSidebarLocked: !currentLocked
      }
    };
    await saveConfig(newConfig);
  }, [config, saveConfig]);

  const handleToggleAILock = useCallback(async () => {
    if (!config) return;
    const currentLocked = config.general.aiSidebarLocked;
    const newConfig = {
      ...config,
      general: {
        ...config.general,
        aiSidebarLocked: !currentLocked
      }
    };
    await saveConfig(newConfig);
  }, [config, saveConfig]);

  const prefetchSettings = useCallback(() => {
    import('./settings/SettingsModal');
  }, []);

  const recentServers = config ? getRecentServers(config.general.recentServerIds, config.servers, config.general.maxRecentServers) : [];

  const displayedSnippets = React.useMemo(() => {
    const globalSnippets = config?.snippets || [];
    const activeTab = tabs.find(t => t.id === activeTabId);
    const activeServer = activeTab ? config?.servers.find(s => s.id === activeTab.serverId) : null;
    const serverSnippets = activeServer?.snippets || [];
    return [...globalSnippets, ...serverSnippets];
  }, [config?.snippets, config?.servers, tabs, activeTabId]);

  return (
    <div className="main-window">
      {/* Title Bar with drag region */}
      <div className="title-bar">
        {/* Tab Bar */}
        <div className="tab-bar" role="tablist">
          {tabs.map((tab, index) => (
            <div
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
              aria-label={t.mainWindow.tabAriaLabel.replace('{index}', (index + 1).toString()).replace('{total}', tabs.length.toString())}
              className={`tab ${activeTabId === tab.id ? 'active' : ''} ${
                draggedTabIndex === index ? 'dragging' : ''
              } ${dropTargetIndex === index ? 'drop-target' : ''}`}
              onClick={() => setActiveTabId(tab.id)}
              onContextMenu={(e) => handleContextMenu(e, tab.id)}
            >
              {recordingTabs.has(tab.id) && (
                <Circle size={8} fill="#ef4444" stroke="#ef4444" className="mr-2 animate-pulse" />
              )}
              <span className="tab-label">{tab.label}</span>
              <button
                type="button"
                className="tab-close"
                onClick={(e) => {
                  e.stopPropagation();
                  handleCloseTab(tab.id);
                }}
                aria-label={t.mainWindow.closeTab}
              >
                <X size={14} />
              </button>
            </div>
          ))}
        </div>
        <NewTabButton
          servers={config?.servers || []}
          onServerSelect={handleAddTab}
          onOpenSettings={() => handleOpenSettings('servers')}
        />

        {/* Drag region spacer - empty area for dragging */}
        <div className="drag-spacer" data-tauri-drag-region></div>

        {/* Right side: Settings button + Window controls */}
        <div className="title-bar-right">
          <button
            type="button"
            className={`settings-btn ${isAIOpen ? 'bg-gray-700 text-white' : ''}`}
            onClick={() => setIsAIOpen(!isAIOpen)}
            aria-label={t.mainWindow.aiAssistant}
            title={t.mainWindow.aiAssistant}
          >
            <MessageSquare size={18} />
          </button>
          <button
            type="button"
            className={`settings-btn ${isSnippetsOpen ? 'bg-gray-700 text-white' : ''}`}
            onClick={() => setIsSnippetsOpen(!isSnippetsOpen)}
            aria-label={t.mainWindow.snippetsTooltip}
            title={t.mainWindow.snippetsTooltip}
          >
            <Code size={18} />
          </button>
          <button
            type="button"
            className="settings-btn"
            onClick={() => handleOpenSettings('servers')}
            onMouseEnter={prefetchSettings}
            onFocus={prefetchSettings}
            aria-label={t.mainWindow.settings}
            title={t.mainWindow.settings}
          >
            <Settings size={18} />
          </button>
          <WindowControls />
        </div>
      </div>

      {/* Content Area */}
      <div className="content-area" style={{ position: 'relative', display: 'flex', flexDirection: 'row' }}>
        <div className="flex-1 flex flex-col min-w-0 relative h-full">
        {tabs.length === 0 ? (
          <WelcomeScreen
            servers={recentServers}
            onServerClick={handleAddTab}
            onOpenSettings={() => handleOpenSettings('servers')}
          />
        ) : (
          tabs.map((tab) => {
            const server = config?.servers.find(s => s.id === tab.serverId);
            if (!server) return null;

            return (
              <div
                key={tab.id}
                style={{
                  display: activeTabId === tab.id ? 'flex' : 'none',
                  flex: 1,
                  minHeight: 0,
                }}
              >
                <TerminalTab
                  tabId={tab.id}
                  serverId={tab.serverId}
                  isActive={activeTabId === tab.id}
                  onClose={handleCloseTab}
                  server={server}
                  servers={config?.servers || []}
                  authentications={config?.authentications || []}
                  proxies={config?.proxies || []}
                  terminalSettings={config?.general.terminal}
                  theme={config?.general.theme}
                  onSessionChange={(sessionId) => {
                    setTabSessions(prev => ({
                      ...prev,
                      [tab.id]: sessionId || ''
                    }));
                  }}
                />
              </div>
            );
          })
        )}
        </div>
        <AISidebar
          isOpen={isAIOpen}
          onClose={() => setIsAIOpen(false)}
          isLocked={config?.general.aiSidebarLocked || false}
          onToggleLock={handleToggleAILock}
          currentServerId={tabs.find(t => t.id === activeTabId)?.serverId}
          currentTabId={activeTabId ? (tabSessions[activeTabId] || undefined) : undefined}
        />
        <SnippetsSidebar  
          isOpen={isSnippetsOpen} 
          onClose={() => setIsSnippetsOpen(false)} 
          snippets={displayedSnippets}
          onOpenSettings={() => handleOpenSettings('snippets')}
          isLocked={config?.general.snippetsSidebarLocked || false}
          onToggleLock={handleToggleSnippetsLock}
        />
      </div>

      {/* Settings Modal */}
      <Suspense fallback={null}>
        {isSettingsOpen && (
          <SettingsModal
            isOpen={isSettingsOpen}
            onClose={() => setIsSettingsOpen(false)}
            onConnectServer={handleConnectServer}
            initialTab={settingsInitialTab}
          />
        )}
      </Suspense>

      {/* Tab Context Menu */}
      {contextMenu && (
        <TabContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          tabId={contextMenu.tabId}
          isRecording={recordingTabs.has(contextMenu.tabId)}
          onClose={() => setContextMenu(null)}
          onClone={handleCloneTab}
          onExport={handleExportLogs}
          onStartRecording={handleStartRecording}
          onStopRecording={handleStopRecording}
          onCloseTab={handleCloseTab}
          onCloseOthers={handleCloseOthers}
        />
      )}

      {/* Toast Notifications */}
      <ToastContainer toasts={toasts} onRemove={removeToast} />
    </div>
  );
};
