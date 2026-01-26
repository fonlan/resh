import React, { useState, useRef } from 'react';
import { Plus, Edit2, Trash2 } from 'lucide-react';
import { Proxy as ProxyType } from '../../types/config';
import { FormModal } from '../FormModal';
import { ProxyForm, ProxyFormHandle } from './ProxyForm';
import { generateId } from '../../utils/idGenerator';

interface ProxyTabProps {
  proxies: ProxyType[];
  onProxiesUpdate: (proxies: ProxyType[]) => void;
}

export const ProxyTab: React.FC<ProxyTabProps> = ({ proxies, onProxiesUpdate }) => {
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingProxy, setEditingProxy] = useState<ProxyType | null>(null);
  const formRef = useRef<ProxyFormHandle>(null);

  const existingNames = proxies
    .filter((p) => p.id !== editingProxy?.id)
    .map((p) => p.name);

  const handleAddProxy = () => {
    setEditingProxy(null);
    setIsFormOpen(true);
  };

  const handleEditProxy = (proxy: ProxyType) => {
    setEditingProxy(proxy);
    setIsFormOpen(true);
  };

  const handleDeleteProxy = (proxyId: string) => {
    onProxiesUpdate(proxies.filter((p) => p.id !== proxyId));
  };

  const handleSaveProxy = (proxy: ProxyType) => {
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
    <div className="tab-container">
      <div className="flex justify-between items-center mb-4">
        <h3 className="section-title">Proxy Servers</h3>
        <button
          type="button"
          onClick={handleAddProxy}
          className="btn btn-primary"
        >
          <Plus size={16} />
          Add Proxy
        </button>
      </div>

      {proxies.length === 0 ? (
        <div className="empty-state-mini">
          <p>No proxies configured. Add one to get started.</p>
        </div>
      ) : (
        <div className="item-list">
          {proxies.map((proxy) => (
            <div
              key={proxy.id}
              className="item-card"
            >
              <div className="item-info">
                <p className="item-name">{proxy.name}</p>
                <p className="item-detail">
                  {proxy.type.toUpperCase()} • {proxy.host}:{proxy.port}
                </p>
              </div>
              <div className="item-actions">
                <button
                  type="button"
                  onClick={() => handleEditProxy(proxy)}
                  className="btn-icon btn-secondary"
                  title="Edit proxy"
                >
                  <Edit2 size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => handleDeleteProxy(proxy.id)}
                  className="btn-icon btn-secondary hover-danger"
                  title="Delete proxy"
                >
                  <Trash2 size={14} />
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
