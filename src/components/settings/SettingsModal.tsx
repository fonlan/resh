import React, { useState, useEffect, useRef } from 'react';
import { X, Server, Key, Globe, Settings, Loader2, Check, AlertCircle, Code, RefreshCw, Bot } from 'lucide-react';
import { Config, Server as ServerType, Authentication, ProxyConfig as ProxyType, GeneralSettings, Snippet, AIChannel, AIModel } from '../../types/config';
import { useConfig } from '../../hooks/useConfig';
import { ServerTab } from './ServerTab';
import { AuthTab } from './AuthTab';
import { ProxyTab } from './ProxyTab';
import { GeneralTab } from './GeneralTab';
import { SyncTab } from './SyncTab';
import { SnippetsTab } from './SnippetsTab';
import { AITab } from './AITab';
import { useTranslation } from '../../i18n';
import './SettingsModal.css';

export interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConnectServer?: (serverId: string) => void;
  initialTab?: TabType;
}

type TabType = 'servers' | 'auth' | 'proxies' | 'snippets' | 'general' | 'sync' | 'ai';
type SaveStatus = 'idle' | 'saving' | 'saved' | 'error';

export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, onClose, onConnectServer, initialTab = 'servers' }) => {
  const { config, loading, error, saveConfig } = useConfig();
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<TabType>(initialTab);
  const [saveStatus, setSaveStatus] = useState<SaveStatus>('idle');
  const [saveError, setSaveError] = useState<string | null>(null);

  // Local state for config
  const [localConfig, setLocalConfig] = useState<Config | null>(null);

  // Ref for debounce timer
  const saveTimeoutRef = useRef<number | null>(null);

  // Ref for saved timeout
  const savedTimeoutRef = useRef<number | null>(null);

  // Ref to store the last saved config to detect actual changes
  const lastSavedConfigRef = useRef<string | null>(null);

  // Ref to store the original config for comparison
  const originalConfigRef = useRef<Config | null>(null);

  // Ref to track if a save is in progress to prevent race conditions
  const isSavingRef = useRef(false);

  // Ref for the modal container to detect clicks outside
  const modalRef = useRef<HTMLDivElement>(null);
  // Track if mouse down was inside the modal
  const mouseDownInsideRef = useRef(false);

  // Initialize local config when config loads
  useEffect(() => {
    if (config) {
      // Only set original config if not already set
      if (!originalConfigRef.current) {
        originalConfigRef.current = config;
      }

      setLocalConfig(config);
      setSaveError(null);
      lastSavedConfigRef.current = JSON.stringify(config);
    }
  }, [config]);

  // Auto-save when localConfig changes
  useEffect(() => {
    if (!localConfig) {
      return;
    }

    // Check if config actually changed
    const currentConfigStr = JSON.stringify(localConfig);
    
    if (currentConfigStr === lastSavedConfigRef.current) {
      return; // No actual changes, skip save
    }

    // Don't start a new save if one is already in progress
    if (isSavingRef.current) {
      return;
    }

    // Clear existing save timeout
    if (saveTimeoutRef.current) {
      clearTimeout(saveTimeoutRef.current);
    }

    // Set saving status
    setSaveStatus('saving');
    setSaveError(null);

    // Debounce: wait 800ms after last change before saving
    saveTimeoutRef.current = window.setTimeout(async () => {
      isSavingRef.current = true;
      try {
        await saveConfig(localConfig);

        lastSavedConfigRef.current = JSON.stringify(localConfig);

        setSaveStatus('saved');

        // Reset to idle after 1.5 seconds
        savedTimeoutRef.current = window.setTimeout(() => {
          setSaveStatus('idle');
        }, 1500);
      } catch (err) {
        setSaveStatus('error');
        setSaveError(err instanceof Error ? err.message : 'Failed to save configuration');
      } finally {
        isSavingRef.current = false;
      }
    }, 800);

    // Cleanup timeout on unmount
    return () => {
      if (saveTimeoutRef.current) {
        clearTimeout(saveTimeoutRef.current);
      }
    };
  }, [localConfig, saveConfig]);

  const handleOverlayMouseDown = (e: React.MouseEvent) => {
    if (modalRef.current && modalRef.current.contains(e.target as Node)) {
      mouseDownInsideRef.current = true;
    } else {
      mouseDownInsideRef.current = false;
    }
  };

  const handleOverlayMouseUp = (e: React.MouseEvent) => {
    // Only close if mouse down and mouse up were both outside the modal
    if (!mouseDownInsideRef.current && modalRef.current && !modalRef.current.contains(e.target as Node)) {
      onClose();
    }
    mouseDownInsideRef.current = false;
  };

  if (!isOpen) {
    return null;
  }

  if (loading) {
    return (
      <div 
        className="settings-overlay"
        onMouseDown={handleOverlayMouseDown}
        onMouseUp={handleOverlayMouseUp}
      >
        <div className="settings-modal loading-modal" ref={modalRef}>
          <Loader2 className="spinner" size={32} />
          <p>{t.common.loading}</p>
        </div>
      </div>
    );
  }

  if (!localConfig) {
    return null;
  }

  const handleServersUpdate = (servers: ServerType[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, servers } : null));
  };

  const handleAuthUpdate = (authentications: Authentication[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, authentications } : null));
  };

  const handleProxiesUpdate = (proxies: ProxyType[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, proxies } : null));
  };

  const handleSnippetsUpdate = (snippets: Snippet[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, snippets } : null));
  };

  const handleGeneralUpdate = (general: GeneralSettings) => {
    setLocalConfig((prev) => (prev ? { ...prev, general } : null));
  };

  const handleAIChannelsUpdate = (aiChannels: AIChannel[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, aiChannels } : null));
  };

  const handleAIModelsUpdate = (aiModels: AIModel[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, aiModels } : null));
  };

  const handleConnectServer = async (serverId: string) => {
    // If there's a pending save, flush it immediately
    if (saveTimeoutRef.current) {
      clearTimeout(saveTimeoutRef.current);
      saveTimeoutRef.current = null;
      
      if (localConfig) {
        setSaveStatus('saving');
        try {
          await saveConfig(localConfig);
          setSaveStatus('saved');
          // Short delay to show "saved" status before closing/connecting
          await new Promise(resolve => setTimeout(resolve, 300));
        } catch (err) {
          setSaveStatus('error');
          // If save failed, we probably shouldn't proceed with connection
          // as the server might not exist in the backend yet
          return;
        }
      }
    } else if (isSavingRef.current) {
      // If currently saving, wait for it to complete
      let checks = 0;
      while (isSavingRef.current && checks < 20) {
        await new Promise(resolve => setTimeout(resolve, 100));
        checks++;
      }
    }

    onConnectServer?.(serverId);
  };

  const tabs: { id: TabType; label: string; icon: React.ReactNode }[] = [
    { id: 'servers', label: t.servers, icon: <Server size={18} /> },
    { id: 'auth', label: t.auth, icon: <Key size={18} /> },
    { id: 'proxies', label: t.proxies, icon: <Globe size={18} /> },
    { id: 'snippets', label: t.snippets, icon: <Code size={18} /> },
    { id: 'ai', label: t.ai.tabTitle, icon: <Bot size={18} /> },
    { id: 'sync', label: t.sync, icon: <RefreshCw size={18} /> },
    { id: 'general', label: t.general, icon: <Settings size={18} /> },
  ];

  return (
    <div 
      className="settings-overlay"
      onMouseDown={handleOverlayMouseDown}
      onMouseUp={handleOverlayMouseUp}
    >
      <div className="settings-modal" ref={modalRef}>
        {/* Header */}
        <div className="settings-header">
          <div className="settings-header-left">
            <h2>{t.settings}</h2>
            {/* Save Status Indicator */}
            {saveStatus !== 'idle' && (
              <div className={`save-status save-status-${saveStatus}`}>
                {saveStatus === 'saving' && (
                  <>
                    <Loader2 size={14} className="spinner" />
                    <span>{t.saveStatus.saving}</span>
                  </>
                )}
                {saveStatus === 'saved' && (
                  <>
                    <Check size={14} />
                    <span>{t.saveStatus.saved}</span>
                  </>
                )}
                {saveStatus === 'error' && (
                  <>
                    <AlertCircle size={14} />
                    <span>{t.saveStatus.error}</span>
                  </>
                )}
              </div>
            )}
          </div>
          <button type="button" onClick={onClose} className="settings-close-btn">
            <X size={20} />
          </button>
        </div>

        {/* Content */}
        <div className="settings-content">
          {/* Sidebar */}
          <div className="settings-sidebar">
            {tabs.map((tab) => (
              <button
                type="button"
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`settings-tab-btn ${activeTab === tab.id ? 'active' : ''}`}
              >
                {tab.icon}
                <span>{tab.label}</span>
              </button>
            ))}
          </div>

          {/* Tab Content */}
          <div className="settings-tab-content">
            {error && <div className="settings-error">{error}</div>}
            {saveError && <div className="settings-error">{saveError}</div>}

            {activeTab === 'servers' && (
              <ServerTab
                servers={localConfig.servers}
                authentications={localConfig.authentications}
                proxies={localConfig.proxies}
                snippets={localConfig.snippets}
                onServersUpdate={handleServersUpdate}
                onConnectServer={handleConnectServer}
              />
            )}
            {activeTab === 'auth' && (
              <AuthTab
                authentications={localConfig.authentications}
                onAuthUpdate={handleAuthUpdate}
                servers={localConfig.servers}
                onServersUpdate={handleServersUpdate}
              />
            )}
            {activeTab === 'proxies' && (
              <ProxyTab
                proxies={localConfig.proxies}
                onProxiesUpdate={handleProxiesUpdate}
                servers={localConfig.servers}
                onServersUpdate={handleServersUpdate}
              />
            )}
            {activeTab === 'snippets' && (
              <SnippetsTab
                snippets={localConfig.snippets || []}
                onSnippetsUpdate={handleSnippetsUpdate}
              />
            )}
            {activeTab === 'ai' && (
              <AITab
                aiChannels={localConfig.aiChannels || []}
                aiModels={localConfig.aiModels || []}
                proxies={localConfig.proxies || []}
                general={localConfig.general}
                onAIChannelsUpdate={handleAIChannelsUpdate}
                onAIModelsUpdate={handleAIModelsUpdate}
                onGeneralUpdate={handleGeneralUpdate}
              />
            )}
            {activeTab === 'general' && (
              <GeneralTab
                general={localConfig.general}
                onGeneralUpdate={handleGeneralUpdate}
              />
            )}
            {activeTab === 'sync' && (
              <SyncTab
                general={localConfig.general}
                onGeneralUpdate={handleGeneralUpdate}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
