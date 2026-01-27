import React, { useState, useRef, useEffect } from 'react';
import { Plus, Edit2, Trash2 } from 'lucide-react';
import { ProxyConfig as ProxyType, Server as ServerType } from '../../types/config';
import { FormModal } from '../FormModal';
import { ProxyForm, ProxyFormHandle } from './ProxyForm';
import { generateId } from '../../utils/idGenerator';
import { useTranslation } from '../../i18n';

interface ProxyTabProps {
  proxies: ProxyType[];
  onProxiesUpdate: (proxies: ProxyType[]) => void;
  servers: ServerType[];
  onServersUpdate: (servers: ServerType[]) => void;
}

export const ProxyTab: React.FC<ProxyTabProps> = ({ 
  proxies, 
  onProxiesUpdate,
  servers,
  onServersUpdate
}) => {
  const { t } = useTranslation();
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingProxy, setEditingProxy] = useState<ProxyType | null>(null);
  const [isSynced, setIsSynced] = useState(true);
  const formRef = useRef<ProxyFormHandle>(null);

  // Sync state with formref
  useEffect(() => {
    if (formRef.current) {
      setIsSynced(formRef.current.synced);
    }
  }, [isFormOpen]);

  const existingNames = proxies
    .filter((p) => p.id !== editingProxy?.id)
    .map((p) => p.name);

  const handleAddProxy = () => {
    setEditingProxy(null);
    setIsSynced(true);
    setIsFormOpen(true);
  };

  const handleEditProxy = (proxy: ProxyType) => {
    setEditingProxy(proxy);
    setIsSynced(proxy.synced !== undefined ? proxy.synced : true);
    setIsFormOpen(true);
  };

  const handleDeleteProxy = (proxyId: string) => {
    const usingServers = servers.filter((s) => s.proxyId === proxyId);
    
    if (usingServers.length > 0) {
      if (window.confirm(t.proxyTab.deleteInUseConfirmation)) {
        // Clear proxy from servers
        const updatedServers = servers.map((s) => 
          s.proxyId === proxyId ? { ...s, proxyId: null } : s
        );
        onServersUpdate(updatedServers);
        // Delete proxy
        onProxiesUpdate(proxies.filter((p) => p.id !== proxyId));
      }
    } else {
      onProxiesUpdate(proxies.filter((p) => p.id !== proxyId));
    }
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
        <h3 className="section-title">{t.proxyTab.title}</h3>
        <button
          type="button"
          onClick={handleAddProxy}
          className="btn btn-primary"
        >
          <Plus size={16} />
          {t.proxyTab.addProxy}
        </button>
      </div>

      {proxies.length === 0 ? (
        <div className="empty-state-mini">
          <p>{t.proxyTab.emptyState}</p>
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
                  title={t.proxyTab.editTooltip}
                >
                  <Edit2 size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => handleDeleteProxy(proxy.id)}
                  className="btn-icon btn-secondary hover-danger"
                  title={t.proxyTab.deleteTooltip}
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
        title={editingProxy ? t.proxyTab.editProxy : t.proxyTab.addProxy}
        onClose={() => {
          setIsFormOpen(false);
          setEditingProxy(null);
        }}
        onSubmit={handleFormSubmit}
        submitText={t.common.save}
        extraFooterContent={
          <div className="flex items-center gap-2 mr-auto">
            <input
              type="checkbox"
              id="synced-footer-proxy"
              checked={isSynced}
              onChange={(e) => {
                setIsSynced(e.target.checked);
                if (formRef.current) {
                  formRef.current.setSynced(e.target.checked);
                }
              }}
              className="checkbox"
            />
            <label htmlFor="synced-footer-proxy" className="text-sm font-medium text-gray-300 cursor-pointer">
              {t.common.syncThisItem || 'Sync this item'}
            </label>
          </div>
        }
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
