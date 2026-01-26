import React, { useState, useEffect, useRef } from 'react';
import { X, Server, Key, Globe, Settings, Loader2, Check, AlertCircle } from 'lucide-react';
import { Config, Server as ServerType, Authentication, Proxy as ProxyType, GeneralSettings } from '../../types/config';
import { useConfig } from '../../hooks/useConfig';
import { ServerTab } from './ServerTab';
import { AuthTab } from './AuthTab';
import { ProxyTab } from './ProxyTab';
import { GeneralTab } from './GeneralTab';
import { useTranslation } from '../../i18n';
import './SettingsModal.css';

export interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConnectServer?: (serverId: string) => void;
}

type TabType = 'servers' | 'auth' | 'proxies' | 'general';
type SaveStatus = 'idle' | 'saving' | 'saved' | 'error';

export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, onClose, onConnectServer }) => {
  const { config, loading, error, saveConfig } = useConfig();
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<TabType>('servers');
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

    // Clear existing timeouts
    if (saveTimeoutRef.current) {
      clearTimeout(saveTimeoutRef.current);
    }
    if (savedTimeoutRef.current) {
      clearTimeout(savedTimeoutRef.current);
    }

    // Set saving status
    setSaveStatus('saving');
    setSaveError(null);

    // Debounce: wait 800ms after last change before saving
    saveTimeoutRef.current = window.setTimeout(async () => {
      isSavingRef.current = true;
      try {
        // For general settings (theme, language), they should ONLY be saved to local config
        // The sync part should NOT include updated general settings
        // So we use the original general for sync (from when modal opened)
        const syncPart = originalConfigRef.current
          ? { ...localConfig, general: originalConfigRef.current.general }
          : localConfig;
        const localPart = localConfig;

        await saveConfig(syncPart, localPart);

        lastSavedConfigRef.current = JSON.stringify(localPart);

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
      if (savedTimeoutRef.current) {
        clearTimeout(savedTimeoutRef.current);
      }
    };
  }, [localConfig, saveConfig]);

  if (!isOpen) {
    return null;
  }

  if (loading) {
    return (
      <div className="settings-overlay">
        <div className="settings-modal loading-modal">
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

  const handleGeneralUpdate = (general: GeneralSettings) => {
    setLocalConfig((prev) => (prev ? { ...prev, general } : null));
  };

  const tabs: { id: TabType; label: string; icon: React.ReactNode }[] = [
    { id: 'servers', label: t.servers, icon: <Server size={18} /> },
    { id: 'auth', label: t.auth, icon: <Key size={18} /> },
    { id: 'proxies', label: t.proxies, icon: <Globe size={18} /> },
    { id: 'general', label: t.general, icon: <Settings size={18} /> },
  ];

  return (
    <div className="settings-overlay">
      <div className="settings-modal">
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
                onServersUpdate={handleServersUpdate}
                onConnectServer={onConnectServer}
              />
            )}
            {activeTab === 'auth' && (
              <AuthTab
                authentications={localConfig.authentications}
                onAuthUpdate={handleAuthUpdate}
              />
            )}
            {activeTab === 'proxies' && (
              <ProxyTab
                proxies={localConfig.proxies}
                onProxiesUpdate={handleProxiesUpdate}
              />
            )}
            {activeTab === 'general' && (
              <GeneralTab
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
