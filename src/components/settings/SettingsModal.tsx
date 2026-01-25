import React, { useState } from 'react';
import { Config } from '../../types/config';
import { ServerTab } from './ServerTab';
import { AuthTab } from './AuthTab';
import { ProxyTab } from './ProxyTab';
import { GeneralTab } from './GeneralTab';

export interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  config?: Config | null;
  onSave?: (config: Config) => void;
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

export const SettingsModal: React.FC<SettingsModalProps> = ({
  isOpen,
  onClose,
  config,
  onSave,
}) => {
  const [activeTab, setActiveTab] = useState<TabType>('servers');
  const [localConfig, setLocalConfig] = useState<Config | null>(config || null);

  React.useEffect(() => {
    setLocalConfig(config || null);
  }, [config]);

  const tabs: { id: TabType; label: string; icon: string }[] = [
    { id: 'servers', label: 'Servers', icon: '🖥️' },
    { id: 'auth', label: 'Authentication', icon: '🔐' },
    { id: 'proxies', label: 'Proxies', icon: '🔄' },
    { id: 'general', label: 'General', icon: '⚙️' },
  ];

  const handleSave = () => {
    if (onSave && localConfig) {
      onSave(localConfig);
    }
    onClose();
  };

  const handleCancel = () => {
    onClose();
  };

  const handleServersUpdate = (servers: any[]) => {
    if (localConfig) {
      setLocalConfig({ ...localConfig, servers });
    }
  };

  const handleAuthUpdate = (authentications: any[]) => {
    if (localConfig) {
      setLocalConfig({ ...localConfig, authentications });
    }
  };

  const handleProxiesUpdate = (proxies: any[]) => {
    if (localConfig) {
      setLocalConfig({ ...localConfig, proxies });
    }
  };

  if (!isOpen) return null;

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
            {activeTab === 'servers' && localConfig && (
              <ServerTab
                servers={localConfig.servers}
                authentications={localConfig.authentications}
                proxies={localConfig.proxies}
                onServersUpdate={handleServersUpdate}
              />
            )}
            {activeTab === 'auth' && localConfig && (
              <AuthTab
                authentications={localConfig.authentications}
                onAuthUpdate={handleAuthUpdate}
              />
            )}
            {activeTab === 'proxies' && localConfig && (
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
            className="px-6 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50 transition-colors font-medium"
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            className="px-6 py-2 bg-blue-500 text-white rounded-lg hover:bg-blue-600 transition-colors font-medium"
          >
            Save
          </button>
        </div>
      </div>
    </div>
  );
};
