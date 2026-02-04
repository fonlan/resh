import React, { useState, useCallback, useEffect, Suspense } from 'react';
import { Settings, X, Code, Circle, MessageSquare, Folder } from 'lucide-react';
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
import { SFTPSidebar } from './SFTPSidebar';
import { TabContextMenu } from './TabContextMenu';
import { ServerContextMenu } from './ServerContextMenu';
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
  const [isSFTPOpen, setIsSFTPOpen] = useState(false);
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
      if (config.general.sftpSidebarLocked) {
        setIsSFTPOpen(true);
      }
    }
  }, [config?.general]);

  // Trigger terminal resize when SFTP sidebar locked state changes
  useEffect(() => {
    if (config?.general.sftpSidebarLocked !== undefined) {
      setTimeout(() => {
        window.dispatchEvent(new CustomEvent('resh-force-terminal-resize'));
      }, 250);
    }
  }, [config?.general.sftpSidebarLocked]);

  const [contextMenu, setContextMenu] = useState<{ x: number, y: number, tabId: string } | null>(null);
  const [serverContextMenu, setServerContextMenu] = useState<{ x: number, y: number, serverId: string } | null>(null);
  const [editServerId, setEditServerId] = useState<string | null>(null);
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

  const handleReconnect = useCallback((tabId: string) => {
    window.dispatchEvent(new CustomEvent(`reconnect:${tabId}`));
  }, []);

  const handleContextMenu = useCallback((e: React.MouseEvent, tabId: string) => {
    e.preventDefault();
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      tabId
    });
  }, []);

  const handleServerContextMenu = useCallback((e: React.MouseEvent, serverId: string) => {
    e.preventDefault();
    setServerContextMenu({
      x: e.clientX,
      y: e.clientY,
      serverId
    });
  }, []);

  const handleConnectServer = useCallback((serverId: string) => {
    handleAddTab(serverId);
    setIsSettingsOpen(false);
    setEditServerId(null);
  }, [handleAddTab]);

  const handleOpenSettings = useCallback((tab: 'servers' | 'auth' | 'proxies' | 'snippets' | 'general' = 'servers') => {
    setSettingsInitialTab(tab);
    setIsSettingsOpen(true);
    if (tab !== 'servers') {
      setEditServerId(null);
    }
  }, []);

  const handleEditServerFromMenu = useCallback((serverId: string) => {
    setEditServerId(serverId);
    handleOpenSettings('servers');
    setServerContextMenu(null);
  }, [handleOpenSettings]);

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

  const handleToggleSFTPLock = useCallback(async () => {
    if (!config) return;
    const currentLocked = config.general.sftpSidebarLocked;
    const newConfig = {
      ...config,
      general: {
        ...config.general,
        sftpSidebarLocked: !currentLocked
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
    <div className="flex flex-col h-screen w-screen overflow-hidden animate-[fadeIn_0.4s_ease-out]">
      <style>{`
        @keyframes fadeIn {
          from { opacity: 0; transform: translateY(10px); }
          to { opacity: 1; transform: translateY(0); }
        }
      `}</style>
      {/* Title Bar with drag region */}
      <div className="flex bg-[var(--bg-secondary)] h-10 border-b border-[var(--glass-border)] select-none relative shrink-0">
        {/* Tab Bar */}
        <div className="flex flex-none min-w-0 overflow-x-auto overflow-y-hidden p-0 gap-0 no-scrollbar" role="tablist">
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
              className={`flex items-center gap-2 px-4 h-10 w-[200px] bg-transparent border-0 border-r border-r-[var(--glass-border)] rounded-none text-[var(--text-secondary)] cursor-pointer whitespace-nowrap transition-all relative overflow-hidden text-[13px] font-medium tracking-tight leading-snug shrink-0 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] ${activeTabId === tab.id ? '!bg-[var(--bg-primary)] !border-t-[3px] !border-t-[var(--accent-primary)] !text-[var(--text-primary)] after:content-[""] after:absolute after:-bottom-px after:left-0 after:right-0 after:h-px after:bg-[var(--bg-primary)] after:z-10' : ''} ${
                draggedTabIndex === index ? 'opacity-40 cursor-grabbing' : ''
              } ${dropTargetIndex === index ? 'border-l-2 border-l-[var(--accent-primary)]' : ''}`}
              onClick={() => setActiveTabId(tab.id)}
              onContextMenu={(e) => handleContextMenu(e, tab.id)}
            >
              {recordingTabs.has(tab.id) && (
                <Circle size={8} fill="#ef4444" stroke="#ef4444" className="mr-2 animate-pulse" />
              )}
              <span className="flex-1 overflow-hidden text-ellipsis whitespace-nowrap">{tab.label}</span>
              <button
                type="button"
                className="flex items-center justify-center w-[18px] h-[18px] bg-transparent border-none text-[var(--text-muted)] text-[14px] cursor-pointer rounded-[4px] transition-all hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)]"
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
        <div className="flex-1 min-w-[40px]" data-tauri-drag-region></div>

        {/* Right side: Settings button + Window controls */}
        <div className="flex items-center">
          <button
            type="button"
            className={`flex items-center justify-center w-10 h-10 bg-transparent border-none text-[var(--text-secondary)] cursor-pointer transition-all hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] ${isAIOpen ? 'bg-gray-700 text-white' : ''}`}
            onClick={() => setIsAIOpen(!isAIOpen)}
            aria-label={t.mainWindow.aiAssistant}
            title={t.mainWindow.aiAssistant}
          >
            <MessageSquare size={18} />
          </button>
          <button
            type="button"
            className={`flex items-center justify-center w-10 h-10 bg-transparent border-none text-[var(--text-secondary)] cursor-pointer transition-all hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] ${isSFTPOpen ? 'bg-gray-700 text-white' : ''}`}
            onClick={() => {
              setIsSFTPOpen(!isSFTPOpen);
              // Force terminal resize after sidebar animation completes
              setTimeout(() => {
                window.dispatchEvent(new CustomEvent('resh-force-terminal-resize'));
              }, 250);
            }}
            aria-label="SFTP"
            title="SFTP"
          >
            <Folder size={18} />
          </button>
          <button
            type="button"
            className={`flex items-center justify-center w-10 h-10 bg-transparent border-none text-[var(--text-secondary)] cursor-pointer transition-all hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] ${isSnippetsOpen ? 'bg-gray-700 text-white' : ''}`}
            onMouseDown={(e) => {
              e.preventDefault();
              e.stopPropagation();
              if (e.button === 0) {
                setIsSnippetsOpen(prev => !prev);
              }
            }}
            aria-label={t.mainWindow.snippetsTooltip}
            title={t.mainWindow.snippetsTooltip}
          >
            <Code size={18} />
          </button>
          <button
            type="button"
            className="flex items-center justify-center w-10 h-10 bg-transparent border-none text-[var(--text-secondary)] cursor-pointer transition-all hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
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
      <div className="flex-1 flex flex-col bg-[var(--bg-primary)] overflow-hidden relative min-h-0" style={{ position: 'relative', display: 'flex', flexDirection: 'row' }}>
        <SFTPSidebar
          isOpen={isSFTPOpen}
          onClose={() => setIsSFTPOpen(false)}
          isLocked={config?.general.sftpSidebarLocked || false}
          onToggleLock={handleToggleSFTPLock}
          sessionId={activeTabId ? (tabSessions[activeTabId] || undefined) : undefined}
        />
        <div className="flex-1 flex flex-col min-w-0 relative h-full">
        {tabs.length === 0 ? (
          <WelcomeScreen
            servers={recentServers}
            onServerClick={handleAddTab}
            onOpenSettings={() => handleOpenSettings('servers')}
            onServerContextMenu={handleServerContextMenu}
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
            onClose={() => {
              setIsSettingsOpen(false);
              setEditServerId(null);
            }}
            onConnectServer={handleConnectServer}
            initialTab={settingsInitialTab}
            editServerId={editServerId}
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
          onReconnect={handleReconnect}
          onExport={handleExportLogs}
          onStartRecording={handleStartRecording}
          onStopRecording={handleStopRecording}
          onCloseTab={handleCloseTab}
          onCloseOthers={handleCloseOthers}
        />
      )}

      {serverContextMenu && (
        <ServerContextMenu
          x={serverContextMenu.x}
          y={serverContextMenu.y}
          serverId={serverContextMenu.serverId}
          onClose={() => setServerContextMenu(null)}
          onEdit={handleEditServerFromMenu}
          onConnect={(serverId) => handleAddTab(serverId)}
        />
      )}

      {/* Toast Notifications */}
      <ToastContainer toasts={toasts} onRemove={removeToast} />
    </div>
  );
};
