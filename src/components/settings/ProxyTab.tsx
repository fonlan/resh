import React, { useState, useRef } from 'react';
import { Proxy } from '../../types/config';
import { FormModal } from '../FormModal';
import { ProxyForm, ProxyFormHandle } from './ProxyForm';
import { generateId } from '../../utils/idGenerator';

interface ProxyTabProps {
  proxies: Proxy[];
  onProxiesUpdate: (proxies: Proxy[]) => void;
}

// Edit icon component
const EditIcon: React.FC = () => (
  <svg
    className="w-5 h-5"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"></path>
    <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"></path>
  </svg>
);

// Delete icon component
const DeleteIcon: React.FC = () => (
  <svg
    className="w-5 h-5"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <polyline points="3 6 5 6 21 6"></polyline>
    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
    <line x1="10" y1="11" x2="10" y2="17"></line>
    <line x1="14" y1="11" x2="14" y2="17"></line>
  </svg>
);

export const ProxyTab: React.FC<ProxyTabProps> = ({ proxies, onProxiesUpdate }) => {
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingProxy, setEditingProxy] = useState<Proxy | null>(null);
  const formRef = useRef<ProxyFormHandle>(null);

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

  const handleFormSubmit = () => {
    // Trigger form validation and submission via ref
    if (formRef.current) {
      formRef.current.submit();
    }
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
                  className="icon-btn icon-btn-edit"
                  title="Edit proxy"
                >
                  <EditIcon />
                </button>
                <button
                  onClick={() => handleDeleteProxy(proxy.id)}
                  className="icon-btn icon-btn-delete"
                  title="Delete proxy"
                >
                  <DeleteIcon />
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
        onSubmit={handleFormSubmit}
        submitText="Save"
      >
        <ProxyForm
          ref={formRef}
          proxy={editingProxy || undefined}
          existingNames={existingNames}
          onSave={handleSaveProxy}
        />
      </FormModal>
    </div>
  );
};
