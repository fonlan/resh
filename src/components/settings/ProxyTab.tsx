import React, { useState } from 'react';
import { Proxy } from '../../types/config';
import { FormModal } from '../FormModal';
import { ProxyForm } from './ProxyForm';
import { generateId } from '../../utils/idGenerator';

interface ProxyTabProps {
  proxies: Proxy[];
  onProxiesUpdate: (proxies: Proxy[]) => void;
}

export const ProxyTab: React.FC<ProxyTabProps> = ({ proxies, onProxiesUpdate }) => {
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingProxy, setEditingProxy] = useState<Proxy | null>(null);

  const existingNames = proxies
    .filter((p) => p.id !== editingProxy?.id)
    .map((p) => p.name);

  const handleAddProxy = () => {
    setEditingProxy(null);
    setIsFormOpen(true);
  };

  const handleEditProxy = (proxy: Proxy) => {
    setEditingProxy(proxy);
    setIsFormOpen(true);
  };

  const handleDeleteProxy = (proxyId: string) => {
    onProxiesUpdate(proxies.filter((p) => p.id !== proxyId));
  };

  const handleSaveProxy = (proxy: Proxy) => {
    if (editingProxy) {
      // Update existing
      onProxiesUpdate(
        proxies.map((p) => (p.id === editingProxy.id ? { ...proxy, id: p.id } : p))
      );
    } else {
      // Add new
      onProxiesUpdate([...proxies, { ...proxy, id: generateId() }]);
    }
    setIsFormOpen(false);
    setEditingProxy(null);
  };

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h3 className="text-lg font-semibold text-white">Proxy Servers</h3>
        <button
          onClick={handleAddProxy}
          className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700"
        >
          Add Proxy
        </button>
      </div>

      {proxies.length === 0 ? (
        <div className="text-center py-8 text-gray-400">
          No proxies configured. Add one to get started.
        </div>
      ) : (
        <div className="space-y-2">
          {proxies.map((proxy) => (
            <div
              key={proxy.id}
              className="flex items-center justify-between bg-gray-800 p-4 rounded-md"
            >
              <div className="flex-1">
                <p className="font-medium text-white">{proxy.name}</p>
                <p className="text-sm text-gray-400">
                  {proxy.type.toUpperCase()} • {proxy.host}:{proxy.port}
                </p>
              </div>
              <div className="flex gap-2">
                <button
                  onClick={() => handleEditProxy(proxy)}
                  className="px-3 py-1 text-sm bg-gray-700 text-white rounded hover:bg-gray-600"
                >
                  Edit
                </button>
                <button
                  onClick={() => handleDeleteProxy(proxy.id)}
                  className="px-3 py-1 text-sm bg-red-700 text-white rounded hover:bg-red-600"
                >
                  Delete
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      <FormModal
        isOpen={isFormOpen}
        title={editingProxy ? 'Edit Proxy' : 'Add Proxy'}
        onClose={() => {
          setIsFormOpen(false);
          setEditingProxy(null);
        }}
        onSubmit={() => {}}
        submitText="Save"
      >
        <ProxyForm
          proxy={editingProxy || undefined}
          existingNames={existingNames}
          onSave={handleSaveProxy}
        />
      </FormModal>
    </div>
  );
};
