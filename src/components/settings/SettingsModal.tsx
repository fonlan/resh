import React, { useState, useEffect } from 'react';
import { Config } from '../../types/config';
import { useConfig } from '../../hooks/useConfig';
import { ServerTab } from './ServerTab';
import { AuthTab } from './AuthTab';
import { ProxyTab } from './ProxyTab';
import { GeneralTab } from './GeneralTab';

export interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
}

type TabType = 'servers' | 'auth' | 'proxies' | 'general';

// Simple X icon component
const XIcon: React.FC = () => (
  <svg
    width="24"
    height="24"
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
      <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
        <div className="bg-white rounded-lg p-6">
          <p className="text-gray-900">Loading configuration...</p>
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
      // Save to both sync and local parts
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

  const handleServersUpdate = (servers: any[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, servers } : null));
  };

  const handleAuthUpdate = (authentications: any[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, authentications } : null));
  };

  const handleProxiesUpdate = (proxies: any[]) => {
    setLocalConfig((prev) => (prev ? { ...prev, proxies } : null));
  };

  const tabs: { id: TabType; label: string; icon: string }[] = [
    { id: 'servers', label: 'Servers', icon: '🖥️' },
    { id: 'auth', label: 'Authentication', icon: '🔐' },
    { id: 'proxies', label: 'Proxies', icon: '🔄' },
    { id: 'general', label: 'General', icon: '⚙️' },
  ];

  return (
    <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
      <div className="bg-white rounded-lg shadow-xl w-full max-w-2xl max-h-[90vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b border-gray-200">
          <h2 className="text-2xl font-bold text-gray-900">Settings</h2>
          <button
            onClick={handleCancel}
            className="text-gray-500 hover:text-gray-700 transition-colors"
          >
            <XIcon />
          </button>
        </div>

        {/* Content */}
        <div className="flex flex-1 overflow-hidden">
          {/* Tabs Navigation */}
          <div className="w-40 bg-gray-50 border-r border-gray-200 p-4 space-y-2 overflow-y-auto">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`w-full text-left px-4 py-3 rounded-lg transition-colors $
                  activeTab === tab.id
                    ? 'bg-blue-500 text-white font-medium'
                    : 'text-gray-700 hover:bg-gray-100'
                }`}
              >
                <span className="mr-2">{tab.icon}</span>
                {tab.label}
              </button>
            ))}
          </div>

          {/* Tab Content */}
          <div className="flex-1 p-6 overflow-y-auto">
            {error && (
              <div className="bg-red-100 border border-red-400 text-red-700 p-4 rounded-lg mb-4">
                {error}
              </div>
            )}

            {saveError && (
              <div className="bg-red-100 border border-red-400 text-red-700 p-4 rounded-lg mb-4">
                {saveError}
              </div>
            )}

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
            {activeTab === 'general' && <GeneralTab />}
          </div>
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-3 p-6 border-t border-gray-200">
          <button
            onClick={handleCancel}
            disabled={isSaving}
            className="px-6 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 transition-colors font-medium disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            disabled={isSaving}
            className="px-6 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 transition-colors font-medium disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
          >
            {isSaving && <span className="inline-block animate-spin">⟳</span>}
            {isSaving ? 'Saving...' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  );
};
