import React, { useState, useEffect } from 'react';
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
}

type TabType = 'servers' | 'auth' | 'proxies' | 'general';

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

export const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, onClose }) => {
  const { config, loading, error, saveConfig } = useConfig();
  const [activeTab, setActiveTab] = useState<TabType>('servers');
  const [isSaving, setIsSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  // Local state for config
  const [localConfig, setLocalConfig] = useState<Config | null>(null);

  useEffect(() => {
    if (config) {
      setLocalConfig(config);
      setSaveError(null);
    }
  }, [config]);

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

  const handleSave = async () => {
    setIsSaving(true);
    setSaveError(null);
    try {
      await saveConfig(localConfig, localConfig);
      onClose();
    } catch (err) {
      setSaveError(err instanceof Error ? err.message : 'Failed to save configuration');
    } finally {
      setIsSaving(false);
    }
  };

  const handleCancel = () => {
    onClose();
  };

  const handleServersUpdate = (servers: Server[]) => {
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
          <h2>Settings</h2>
          <button onClick={handleCancel} className="settings-close-btn">
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

        {/* Footer */}
        <div className="settings-footer">
          <button
            onClick={handleCancel}
            disabled={isSaving}
            className="settings-btn settings-btn-cancel"
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            disabled={isSaving}
            className="settings-btn settings-btn-save"
          >
            {isSaving && <span className="spinner">⟳</span>}
            {isSaving ? 'Saving...' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  );
};
