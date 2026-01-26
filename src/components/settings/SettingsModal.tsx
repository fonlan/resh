import React, { useState, useEffect, useRef } from 'react';
import { Config, Server, Authentication, Proxy, GeneralSettings } from '../../types/config';
import { useConfig } from '../../hooks/useConfig';
import { ServerTab } from './ServerTab';
import { AuthTab } from './AuthTab';
import { ProxyTab } from './ProxyTab';
import { GeneralTab } from './GeneralTab';
import './SettingsModal.css';

export interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConnectServer?: (serverId: string) => void;
}

type TabType = 'servers' | 'auth' | 'proxies' | 'general';
type SaveStatus = 'idle' | 'saving' | 'saved' | 'error';

// Simple X icon component
const XIcon: React.FC = () => (
  <svg
    width="20"
    height="20"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <line x1="18" y1="6" x2="6" y2="18"></line>
    <line x1="6" y1="6" x2="18" y2="18"></line>
  </svg>
);

export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, onClose, onConnectServer }) => {
  const { config, loading, error, saveConfig } = useConfig();
  const [activeTab, setActiveTab] = useState<TabType>('servers');
  const [saveStatus, setSaveStatus] = useState<SaveStatus>('idle');
  const [saveError, setSaveError] = useState<string | null>(null);

  // Local state for config
  const [localConfig, setLocalConfig] = useState<Config | null>(null);

  // Ref to track if this is the initial load
  const isInitialLoad = useRef(true);

  // Ref for debounce timer
  const saveTimeoutRef = useRef<number | null>(null);

  // Ref for saved timeout
  const savedTimeoutRef = useRef<number | null>(null);

  // Ref to store the last saved config to detect actual changes
  const lastSavedConfigRef = useRef<string | null>(null);

  // Initialize local config when config loads
  useEffect(() => {
    if (config) {
      setLocalConfig(config);
      setSaveError(null);
      isInitialLoad.current = true;
      lastSavedConfigRef.current = JSON.stringify(config);
    }
  }, [config]);

  // Auto-save when localConfig changes
  useEffect(() => {
    // Skip auto-save on initial load
    if (isInitialLoad.current) {
      isInitialLoad.current = false;
      return;
    }

    if (!localConfig) {
      return;
    }

    // Check if config actually changed
    const currentConfigStr = JSON.stringify(localConfig);
    console.log('SettingsModal: Checking for changes...');
    if (currentConfigStr === lastSavedConfigRef.current) {
      console.log('SettingsModal: No changes detected.');
      return; // No actual changes, skip save
    }
    console.log('SettingsModal: Changes detected, scheduling save.');

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
      try {
        await saveConfig(localConfig, localConfig);
        lastSavedConfigRef.current = JSON.stringify(localConfig);
        setSaveStatus('saved');

        // Reset to idle after 1.5 seconds
        savedTimeoutRef.current = window.setTimeout(() => {
          setSaveStatus('idle');
        }, 1500);
      } catch (err) {
        setSaveStatus('error');
        setSaveError(err instanceof Error ? err.message : 'Failed to save configuration');
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
        <div className="settings-modal" style={{ padding: '24px' }}>
          <p style={{ color: '#cccccc' }}>Loading configuration...</p>
        </div>
      </div>
    );
  }

  if (!localConfig) {
    return null;
  }

  const handleServersUpdate = (servers: Server[]) => {
    console.log('SettingsModal: Received servers update:', servers);
    setLocalConfig((prev) => (prev ? { ...prev, servers } : null));
  };

  const handleAuthUpdate = (authentications: Authentication[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, authentications } : null));
  };

  const handleProxiesUpdate = (proxies: Proxy[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, proxies } : null));
  };

  const handleGeneralUpdate = (general: GeneralSettings) => {
    setLocalConfig((prev) => (prev ? { ...prev, general } : null));
  };

  const tabs: { id: TabType; label: string; icon: string }[] = [
    { id: 'servers', label: 'Servers', icon: '🖥️' },
    { id: 'auth', label: 'Authentication', icon: '🔐' },
    { id: 'proxies', label: 'Proxies', icon: '🔄' },
    { id: 'general', label: 'General', icon: '⚙️' },
  ];

  return (
    <div className="settings-overlay">
      <div className="settings-modal">
        {/* Header */}
        <div className="settings-header">
          <div className="settings-header-left">
            <h2>Settings</h2>
            {/* Save Status Indicator */}
            {saveStatus !== 'idle' && (
              <span className={`save-status save-status-${saveStatus}`}>
                {saveStatus === 'saving' && (
                  <>
                    <span className="spinner">⟳</span>
                    Saving...
                  </>
                )}
                {saveStatus === 'saved' && '✓ Saved'}
                {saveStatus === 'error' && '⚠ Error saving'}
              </span>
            )}
          </div>
          <button onClick={onClose} className="settings-close-btn">
            <XIcon />
          </button>
        </div>

        {/* Content */}
        <div className="settings-content">
          {/* Sidebar */}
          <div className="settings-sidebar">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`settings-tab-btn ${activeTab === tab.id ? 'active' : ''}`}
              >
                <span>{tab.icon}</span>
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
