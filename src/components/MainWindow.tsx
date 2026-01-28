import React, { useState, useCallback, Suspense } from 'react';
import { Settings, X, Code } from 'lucide-react';
// SettingsModal is now lazy loaded
const SettingsModal = React.lazy(() => 
  import('./settings/SettingsModal').then(module => ({ default: module.SettingsModal }))
);
import { TerminalTab } from './TerminalTab';
import { WindowControls } from './WindowControls';
import { WelcomeScreen } from './WelcomeScreen';
import { NewTabButton } from './NewTabButton';
import { SnippetsSidebar } from './SnippetsSidebar';
import { TabContextMenu } from './TabContextMenu';
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
  const [contextMenu, setContextMenu] = useState<{ x: number, y: number, tabId: string } | null>(null);

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
  }, [tabs, activeTabId]);

  const handleAddTab = useCallback(async (serverId: string) => {
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
      await saveConfig({ ...config, general: updatedGeneral });
    }
  }, [config, saveConfig]);

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
            className={`settings-btn ${isSnippetsOpen ? 'bg-gray-700 text-white' : ''}`}
            onClick={() => setIsSnippetsOpen(!isSnippetsOpen)}
            aria-label="Toggle Snippets"
            title="Snippets"
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
      <div className="content-area" style={{ position: 'relative' }}>
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
                />
              </div>
            );
          })
        )}
        </div>
        <SnippetsSidebar 
          isOpen={isSnippetsOpen} 
          onClose={() => setIsSnippetsOpen(false)} 
          snippets={displayedSnippets}
          onOpenSettings={() => handleOpenSettings('snippets')}
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
          onClose={() => setContextMenu(null)}
          onClone={handleCloneTab}
          onExport={handleExportLogs}
          onCloseTab={handleCloseTab}
          onCloseOthers={handleCloseOthers}
        />
      )}
    </div>
  );
};
