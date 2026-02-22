import React, { useState, useCallback, useEffect, useMemo, Suspense } from 'react';
import { Settings, X, Code, Circle, MessageSquare, Folder } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Config } from '../types';
// SettingsModal is now lazy loaded
const SettingsModal = React.lazy(() => 
  import('./settings/SettingsModal').then(module => ({ default: module.SettingsModal }))
);
const AISidebar = React.lazy(() =>
  import('./AISidebar').then(module => ({ default: module.AISidebar }))
);
const SFTPSidebar = React.lazy(() =>
  import('./SFTPSidebar').then(module => ({ default: module.SFTPSidebar }))
);
const SnippetsSidebar = React.lazy(() =>
  import('./SnippetsSidebar').then(module => ({ default: module.SnippetsSidebar }))
);
const TerminalTab = React.lazy(() =>
  import('./TerminalTab').then(module => ({ default: module.TerminalTab }))
)
import { WindowControls } from './WindowControls';
import { WelcomeScreen } from './WelcomeScreen';
import { NewTabButton } from './NewTabButton';
import { SplitViewButton, SplitLayout } from './SplitViewButton';
import { SplitTabPickerModal } from './SplitTabPickerModal';
import { TabContextMenu } from './TabContextMenu';
import { ServerContextMenu } from './ServerContextMenu';
import { ToastContainer, ToastItem } from './Toast';
import { useConfig } from '../hooks/useConfig';
import { generateId } from '../utils/idGenerator';
import { addRecentServer, getRecentServers } from '../utils/recentServers';
import { useTranslation } from '../i18n';
import { useTabDragDrop } from '../hooks/useTabDragDrop';
import { EmojiText } from './EmojiText';
import { useTransferStore } from '../stores/transferStore';

interface Tab {
  id: string;
  label: string;
  serverId: string;
}

interface SplitViewState {
  layout: SplitLayout;
  tabIds: string[];
}

const EMPTY_SERVERS: Config['servers'] = []
const EMPTY_AUTHENTICATIONS: Config['authentications'] = []
const EMPTY_PROXIES: Config['proxies'] = []
const SPLIT_LAYOUT_REQUIRED_TABS: Record<SplitLayout, number> = {
  horizontal: 2,
  vertical: 2,
  grid: 4,
}
const MIN_FIXED_TAB_WIDTH = 120
const MAX_FIXED_TAB_WIDTH = 400
const DEFAULT_FIXED_TAB_WIDTH = 200

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
  const [isSidebarsInitialized, setIsSidebarsInitialized] = useState(false);
  const [hasLoadedSnippetsSidebar, setHasLoadedSnippetsSidebar] = useState(false);
  const [hasLoadedSFTPSidebar, setHasLoadedSFTPSidebar] = useState(false);
  const [hasLoadedAISidebar, setHasLoadedAISidebar] = useState(false);

  const initListener = useTransferStore(state => state.initListener);

  // Initialize transfer store listener
  useEffect(() => {
    let cleanup: (() => void) | undefined;
    initListener().then(unlisten => {
      cleanup = unlisten;
    });
    return () => {
      cleanup?.();
    };
  }, [initListener]);

  useEffect(() => {
    if (config && !isSidebarsInitialized) {
      if (config.general.sftpSidebarLocked) {
        setHasLoadedSFTPSidebar(true);
        setIsSFTPOpen(true);
      }
      if (config.general.aiSidebarLocked) {
        setHasLoadedAISidebar(true);
        setIsAIOpen(true);
      }
      if (config.general.snippetsSidebarLocked) {
        setHasLoadedSnippetsSidebar(true);
        setIsSnippetsOpen(true);
      }
      setIsSidebarsInitialized(true);
    }
  }, [config, isSidebarsInitialized]);

  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const showToast = useCallback((message: string, type: ToastItem['type'] = 'info', duration?: number) => {
    const id = generateId();
    setToasts(prev => [...prev, { id, type, message, duration }]);
  }, []);

  // Listen for sync failed events
  useEffect(() => {
    let isMounted = true;

    const syncFailedListener = listen<string>('sync-failed', (event) => {
      if (isMounted) {
        showToast(`同步失败: ${event.payload}`, 'error');
      }
    });

    return () => {
      isMounted = false;
      syncFailedListener.then(unlisten => unlisten());
    };
  }, [showToast]);

  const removeToast = (id: string) => {
    setToasts(prev => prev.filter(t => t.id !== id));
  };


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
  const [splitView, setSplitView] = useState<SplitViewState | null>(null);
  const [pendingSplitLayout, setPendingSplitLayout] = useState<SplitLayout | null>(null);

  const servers = config?.servers || EMPTY_SERVERS
  const authentications = config?.authentications || EMPTY_AUTHENTICATIONS
  const proxies = config?.proxies || EMPTY_PROXIES

  const serverById = new Map<string, Config['servers'][number]>()
  servers.forEach(server => {
    serverById.set(server.id, server)
  })

  const activeTab = tabs.find(tab => tab.id === activeTabId) || null

  const activeServerId = activeTab?.serverId

  const handleTabSessionChange = (tabId: string, sessionId: string | null) => {
    const normalizedSessionId = sessionId || ''
    setTabSessions(prev => {
      if (prev[tabId] === normalizedSessionId) {
        return prev
      }

      return {
        ...prev,
        [tabId]: normalizedSessionId,
      }
    })
  }

  const triggerTerminalResize = useCallback(() => {
    setTimeout(() => {
      window.dispatchEvent(new CustomEvent('resh-force-terminal-resize'));
    }, 40);
  }, []);

  const handleExitSplitView = useCallback(() => {
    setSplitView(null);
    setPendingSplitLayout(null);
    triggerTerminalResize();
  }, [triggerTerminalResize]);

  const handleStartSplitSelection = useCallback((layout: SplitLayout) => {
    setPendingSplitLayout(layout);
  }, []);

  const handleConfirmSplitSelection = useCallback((selectedTabIds: string[]) => {
    if (!pendingSplitLayout) {
      return;
    }

    const requiredTabs = SPLIT_LAYOUT_REQUIRED_TABS[pendingSplitLayout];
    if (selectedTabIds.length !== requiredTabs) {
      return;
    }

    const nextActiveTabId = selectedTabIds.includes(activeTabId || '') && activeTabId
      ? activeTabId
      : selectedTabIds[0];

    setSplitView({
      layout: pendingSplitLayout,
      tabIds: selectedTabIds,
    });
    setActiveTabId(nextActiveTabId);
    setPendingSplitLayout(null);
    triggerTerminalResize();
  }, [pendingSplitLayout, activeTabId, triggerTerminalResize]);

  const handleTabSelect = useCallback((tabId: string) => {
    if (!splitView) {
      setActiveTabId(tabId);
      return;
    }

    if (splitView.tabIds.includes(tabId)) {
      setActiveTabId(tabId);
      return;
    }

    const nextSplitTabIds = [...splitView.tabIds];
    const replaceIndex = activeTabId ? nextSplitTabIds.indexOf(activeTabId) : -1;

    if (replaceIndex >= 0) {
      nextSplitTabIds[replaceIndex] = tabId;
    } else {
      nextSplitTabIds[0] = tabId;
    }

    setSplitView({
      ...splitView,
      tabIds: nextSplitTabIds,
    });
    setActiveTabId(tabId);
    triggerTerminalResize();
  }, [splitView, activeTabId, triggerTerminalResize]);

  useEffect(() => {
    if (tabs.length === 0) {
      if (splitView) {
        setSplitView(null);
      }
      if (activeTabId !== null) {
        setActiveTabId(null);
      }
      return;
    }

    const validTabIds = new Set(tabs.map(tab => tab.id));

    if (activeTabId && !validTabIds.has(activeTabId)) {
      setActiveTabId(tabs[0].id);
    }

    if (!splitView) {
      return;
    }

    const filteredTabIds = splitView.tabIds.filter(tabId => validTabIds.has(tabId));
    const requiredTabs = SPLIT_LAYOUT_REQUIRED_TABS[splitView.layout];

    if (filteredTabIds.length < requiredTabs) {
      setSplitView(null);
      if (filteredTabIds.length > 0) {
        setActiveTabId(filteredTabIds[0]);
      }
      triggerTerminalResize();
      return;
    }

    const splitTabsChanged = filteredTabIds.join('|') !== splitView.tabIds.join('|');
    if (splitTabsChanged) {
      setSplitView({
        ...splitView,
        tabIds: filteredTabIds,
      });
    }

    if (activeTabId && !filteredTabIds.includes(activeTabId)) {
      setActiveTabId(filteredTabIds[0]);
    }
  }, [tabs, activeTabId, splitView, triggerTerminalResize]);

  useEffect(() => {
    if (servers.length === 0) return;

    setTabs(prevTabs => {
      let hasChanges = false;
      const newTabs = prevTabs.map(tab => {
        const server = serverById.get(tab.serverId)
        if (server && server.name !== tab.label) {
          hasChanges = true;
          return { ...tab, label: server.name };
        }
        return tab;
      });
      return hasChanges ? newTabs : prevTabs;
    });
  }, [servers.length, servers]);

  const {
    draggedIndex: draggedTabIndex,
    dropTargetIndex,
    handleDragStart: handleTabDragStart,
    handleDragOver: handleTabDragOver,
    handleDrop: handleTabDrop,
    handleDragEnd: handleTabDragEnd
  } = useTabDragDrop(tabs, setTabs);

  const handleTabKeyDown = (e: React.KeyboardEvent, index: number) => {
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
  };

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

  const handleExportLogs = (tabId: string) => {
    const event = new CustomEvent(`export-terminal-logs:${tabId}`);
    window.dispatchEvent(event);
  };

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

  const handleStopRecording = (tabId: string) => {
    window.dispatchEvent(new CustomEvent(`stop-recording:${tabId}`));
    setRecordingTabs(prev => {
      const next = new Set(prev);
      next.delete(tabId);
      return next;
    });
  };

  const handleReconnect = (tabId: string) => {
    window.dispatchEvent(new CustomEvent(`reconnect:${tabId}`));
  };

  const handleContextMenu = (e: React.MouseEvent, tabId: string) => {
    e.preventDefault();
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      tabId
    });
  };

  const handleServerContextMenu = (e: React.MouseEvent, serverId: string) => {
    e.preventDefault();
    setServerContextMenu({
      x: e.clientX,
      y: e.clientY,
      serverId
    });
  };

  const handleConnectServer = (serverId: string) => {
    handleAddTab(serverId);
    setIsSettingsOpen(false);
    setEditServerId(null);
  };

  const handleOpenSettings = (tab: 'servers' | 'auth' | 'proxies' | 'snippets' | 'general' = 'servers') => {
    setSettingsInitialTab(tab);
    setIsSettingsOpen(true);
    if (tab !== 'servers') {
      setEditServerId(null);
    }
  };

  const handleEditServerFromMenu = (serverId: string) => {
    setEditServerId(serverId);
    handleOpenSettings('servers');
    setServerContextMenu(null);
  };

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

  const prefetchSettings = () => {
    import('./settings/SettingsModal');
  };

  const prefetchSFTPSidebar = () => {
    void import('./SFTPSidebar');
  };

  const prefetchAISidebar = () => {
    void import('./AISidebar');
  };

  const prefetchSnippetsSidebar = () => {
    void import('./SnippetsSidebar');
  };

  const recentServers = config ? getRecentServers(config.general.recentServerIds, servers, config.general.maxRecentServers) : [];
  const tabWidthMode = config?.general.tabWidthMode === 'adaptive' ? 'adaptive' : 'fixed'
  const tabFixedWidthRaw = config?.general.tabFixedWidth
  const tabFixedWidth = typeof tabFixedWidthRaw === 'number' && Number.isFinite(tabFixedWidthRaw)
    ? Math.max(MIN_FIXED_TAB_WIDTH, Math.min(MAX_FIXED_TAB_WIDTH, tabFixedWidthRaw))
    : DEFAULT_FIXED_TAB_WIDTH

  const globalSnippets = config?.snippets || [];
  const activeServer = activeServerId ? serverById.get(activeServerId) || null : null
  const serverSnippets = activeServer?.snippets || [];
  const displayedSnippets = [...globalSnippets, ...serverSnippets];
  const isSplitMode = splitView !== null;
  const splitLayoutClassName = !splitView
    ? 'flex-1 flex flex-col min-w-0 relative h-full'
    : splitView.layout === 'horizontal'
      ? 'flex-1 min-w-0 relative h-full grid grid-cols-2 gap-2 p-2'
      : splitView.layout === 'vertical'
        ? 'flex-1 min-w-0 relative h-full grid grid-rows-2 gap-2 p-2'
        : 'flex-1 min-w-0 relative h-full grid grid-cols-2 grid-rows-2 gap-2 p-2';
  const pendingSplitRequiredCount = pendingSplitLayout ? SPLIT_LAYOUT_REQUIRED_TABS[pendingSplitLayout] : 0;
  const initialSplitSelectedTabIds = useMemo(() => {
    if (!pendingSplitLayout) {
      return [];
    }

    const requiredTabs = SPLIT_LAYOUT_REQUIRED_TABS[pendingSplitLayout];
    const candidateIds: string[] = [];

    if (activeTabId) {
      candidateIds.push(activeTabId);
    }

    if (splitView?.layout === pendingSplitLayout) {
      splitView.tabIds.forEach(id => {
        if (!candidateIds.includes(id)) {
          candidateIds.push(id);
        }
      });
    }

    tabs.forEach(tab => {
      if (!candidateIds.includes(tab.id)) {
        candidateIds.push(tab.id);
      }
    });

    return candidateIds.slice(0, requiredTabs);
  }, [pendingSplitLayout, splitView, tabs, activeTabId]);

  // Calculate z-index for sidebars based on lock state and open order
  // Rule: unlocked sidebars always appear above locked ones
  // If both unlocked, later-opened appears on top (rendering order determines this naturally)
  const aiZIndex = config?.general.aiSidebarLocked ? 10 : 50;
  const snippetsZIndex = config?.general.snippetsSidebarLocked ? 10 : 50;
  const sftpZIndex = config?.general.sftpSidebarLocked ? 10 : 50;

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden animate-[fadeIn_0.4s_ease-out]">
      <style>{`
        @keyframes fadeIn {
          from { opacity: 0; transform: translateY(10px); }
          to { opacity: 1; transform: translateY(0); }
        }
      `}</style>
      {/* Title Bar with drag region */}
      <div className="flex min-w-0 bg-[var(--bg-secondary)] h-10 border-b border-[var(--glass-border)] select-none relative shrink-0">
        {/* Tab Bar */}
        <div className="flex flex-[0_1_auto] min-w-0 overflow-x-auto overflow-y-hidden p-0 gap-0 no-scrollbar" role="tablist">
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
              className={`flex items-center gap-2 px-4 h-10 ${tabWidthMode === 'adaptive' ? 'w-auto min-w-[120px] max-w-[320px]' : 'w-auto'} bg-transparent border-0 border-r border-r-[var(--glass-border)] rounded-none text-[var(--text-secondary)] cursor-pointer whitespace-nowrap transition-all relative overflow-hidden text-[13px] font-medium leading-snug shrink-0 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] ${activeTabId === tab.id ? '!bg-[var(--bg-primary)] !border-t-[3px] !border-t-[var(--accent-primary)] !text-[var(--text-primary)] after:content-[""] after:absolute after:-bottom-px after:left-0 after:right-0 after:h-px after:bg-[var(--bg-primary)] after:z-10' : ''} ${
                draggedTabIndex === index ? 'opacity-40 cursor-grabbing' : ''
              } ${dropTargetIndex === index ? 'border-l-2 border-l-[var(--accent-primary)]' : ''}`}
              style={tabWidthMode === 'fixed' ? { width: `${tabFixedWidth}px` } : undefined}
              onClick={() => handleTabSelect(tab.id)}
              onContextMenu={(e) => handleContextMenu(e, tab.id)}
            >
              {recordingTabs.has(tab.id) && (
                <Circle size={8} fill="#ef4444" stroke="#ef4444" className="mr-2 animate-pulse" />
              )}
              <span className="flex-1 overflow-hidden text-ellipsis whitespace-nowrap">
                <EmojiText text={tab.label} />
              </span>
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
          <div className="shrink-0" role="presentation">
            <NewTabButton
              servers={config?.servers || []}
              onServerSelect={handleAddTab}
              onOpenSettings={() => handleOpenSettings('servers')}
            />
          </div>
        </div>

        {/* Drag region spacer - empty area for dragging */}
        <div className="flex-1 min-w-[40px] basis-0" data-tauri-drag-region></div>

        {/* Right side: Settings button + Window controls */}
        <div className="flex items-center shrink-0">
          <button
            type="button"
            className={`flex items-center justify-center w-10 h-10 border-none text-[var(--text-secondary)] cursor-pointer transition-all ${isSFTPOpen ? 'bg-[var(--bg-tertiary)] text-[var(--accent-primary)]' : 'bg-transparent hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]'}`}
            onMouseDown={(e) => {
              e.stopPropagation();
              if (!isSFTPOpen) {
                setHasLoadedSFTPSidebar(true);
                setIsSFTPOpen(true);
              } else if (config?.general.sftpSidebarLocked) {
                handleToggleSFTPLock();
              } else {
                setIsSFTPOpen(false);
              }
            }}
            onMouseEnter={prefetchSFTPSidebar}
            onFocus={prefetchSFTPSidebar}
            aria-label="SFTP"
            title="SFTP"
          >
            <Folder size={18} />
          </button>
          <button
            type="button"
            className={`flex items-center justify-center w-10 h-10 border-none text-[var(--text-secondary)] cursor-pointer transition-all ${isAIOpen ? 'bg-[var(--bg-tertiary)] text-[var(--accent-primary)]' : 'bg-transparent hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]'}`}
            onMouseDown={(e) => {
              e.stopPropagation();
              if (!isAIOpen) {
                setHasLoadedAISidebar(true);
                setIsAIOpen(true);
              } else if (config?.general.aiSidebarLocked) {
                handleToggleAILock();
              } else {
                setIsAIOpen(false);
              }
            }}
            onMouseEnter={prefetchAISidebar}
            onFocus={prefetchAISidebar}
            aria-label={t.mainWindow.aiAssistant}
            title={t.mainWindow.aiAssistant}
          >
            <MessageSquare size={18} />
          </button>
          <button
            type="button"
            className={`flex items-center justify-center w-10 h-10 border-none text-[var(--text-secondary)] cursor-pointer transition-all ${isSnippetsOpen ? 'bg-[var(--bg-tertiary)] text-[var(--accent-primary)]' : 'bg-transparent hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]'}`}
            onMouseDown={(e) => {
              e.stopPropagation();
              if (!isSnippetsOpen) {
                setHasLoadedSnippetsSidebar(true);
                setIsSnippetsOpen(true);
              } else if (config?.general.snippetsSidebarLocked) {
                handleToggleSnippetsLock();
              } else {
                setIsSnippetsOpen(false);
              }
            }}
            onMouseEnter={prefetchSnippetsSidebar}
            onFocus={prefetchSnippetsSidebar}
            aria-label={t.mainWindow.snippetsTooltip}
            title={t.mainWindow.snippetsTooltip}
          >
            <Code size={18} />
          </button>
          <SplitViewButton
            tabCount={tabs.length}
            isSplitActive={isSplitMode}
            onSelectLayout={handleStartSplitSelection}
            onExitSplit={handleExitSplitView}
          />
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
        <Suspense fallback={null}>
          {hasLoadedSFTPSidebar && (
            <SFTPSidebar
              isOpen={isSFTPOpen}
              onClose={() => setIsSFTPOpen(false)}
              isLocked={config?.general.sftpSidebarLocked || false}
              onToggleLock={handleToggleSFTPLock}
              sessionId={activeTabId ? (tabSessions[activeTabId] || undefined) : undefined}
              zIndex={sftpZIndex}
            />
          )}
        </Suspense>
        <div className={splitLayoutClassName}>
          {tabs.length === 0 ? (
            <WelcomeScreen
              servers={recentServers}
              onServerClick={handleAddTab}
              onOpenSettings={() => handleOpenSettings('servers')}
              onServerContextMenu={handleServerContextMenu}
            />
          ) : (
            tabs.map((tab) => {
              const server = serverById.get(tab.serverId)
              if (!server) return null;

              const isVisibleInLayout = splitView ? splitView.tabIds.includes(tab.id) : activeTabId === tab.id;
              const isFocusedTab = activeTabId === tab.id;

              return (
                <div
                  key={tab.id}
                  className={isSplitMode ? 'min-w-0 min-h-0 flex' : 'min-h-0 flex-1 flex'}
                  style={{
                    display: isVisibleInLayout ? 'flex' : 'none',
                    minHeight: 0,
                    minWidth: 0,
                  }}
                >
                  <div
                    className={`w-full h-full min-w-0 min-h-0 overflow-hidden ${isSplitMode ? `rounded-[var(--radius-md)] border ${isFocusedTab ? 'border-[var(--accent-primary)] ring-2 ring-[var(--accent-primary)]/40' : 'border-[var(--glass-border)]'}` : ''}`}
                  >
                    <Suspense fallback={<div className="flex-1 bg-[var(--bg-primary)]" />}>
                      <TerminalTab
                        tabId={tab.id}
                        serverId={tab.serverId}
                        isActive={isFocusedTab}
                        isVisible={isVisibleInLayout}
                        onClose={handleCloseTab}
                        onActivate={() => handleTabSelect(tab.id)}
                        server={server}
                        servers={servers}
                        authentications={authentications}
                        proxies={proxies}
                        terminalSettings={config?.general.terminal}
                        theme={config?.general.theme}
                        onSessionChange={(sessionId) => handleTabSessionChange(tab.id, sessionId)}
                        onShowToast={showToast}
                      />
                    </Suspense>
                  </div>
                </div>
              );
            })
          )}
        </div>
        <Suspense fallback={null}>
          {hasLoadedAISidebar && (
            <AISidebar
              isOpen={isAIOpen}
              onClose={() => setIsAIOpen(false)}
              isLocked={config?.general.aiSidebarLocked || false}
              onToggleLock={handleToggleAILock}
              onShowToast={showToast}
              currentServerId={activeServerId}
              currentTabId={activeTabId ? (tabSessions[activeTabId] || undefined) : undefined}
              zIndex={aiZIndex}
            />
          )}
        </Suspense>
        <Suspense fallback={null}>
          {hasLoadedSnippetsSidebar && (
            <SnippetsSidebar
              isOpen={isSnippetsOpen}
              onClose={() => setIsSnippetsOpen(false)}
              snippets={displayedSnippets}
              onOpenSettings={() => handleOpenSettings('snippets')}
              isLocked={config?.general.snippetsSidebarLocked || false}
              onToggleLock={handleToggleSnippetsLock}
              zIndex={snippetsZIndex}
            />
          )}
        </Suspense>
      </div>

      <SplitTabPickerModal
        isOpen={pendingSplitLayout !== null}
        layout={pendingSplitLayout}
        tabs={tabs.map(tab => ({ id: tab.id, label: tab.label }))}
        requiredCount={pendingSplitRequiredCount}
        initialSelectedTabIds={initialSplitSelectedTabIds}
        onCancel={() => setPendingSplitLayout(null)}
        onConfirm={handleConfirmSplitSelection}
      />

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
