import React, { useState, useRef, useEffect } from 'react';
import { Plus, Edit2, Trash2 } from 'lucide-react';
import { ProxyConfig as ProxyType, Server as ServerType } from '../../types';
import { FormModal } from '../FormModal';
import { ConfirmationModal } from '../ConfirmationModal';
import { ProxyForm, ProxyFormHandle } from './ProxyForm';
import { generateId } from '../../utils/idGenerator';
import { useTranslation } from '../../i18n';
import { EmojiText } from '../EmojiText';

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
  const [proxyToDelete, setProxyToDelete] = useState<string | null>(null);
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
      setProxyToDelete(proxyId);
    } else {
      onProxiesUpdate(proxies.filter((p) => p.id !== proxyId));
    }
  };

  const confirmDeleteProxy = () => {
    if (proxyToDelete) {
      const updatedServers = servers.map((s) => 
        s.proxyId === proxyToDelete ? { ...s, proxyId: null } : s
      );
      onServersUpdate(updatedServers);
      onProxiesUpdate(proxies.filter((p) => p.id !== proxyToDelete));
      setProxyToDelete(null);
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

  const sortedProxies = [...proxies].sort((a, b) => a.name.localeCompare(b.name));

  return (
    <div className="w-full max-w-full">
      <div className="flex justify-between items-center mb-4">
        <h3 className="text-base font-semibold text-[var(--text-primary)]">{t.proxyTab.title}</h3>
        <button
          type="button"
          onClick={handleAddProxy}
          className="inline-flex items-center justify-center gap-2 px-4 py-2 text-sm font-medium bg-[var(--accent-primary)] text-white shadow-[var(--glow-primary)] hover:brightness-110 hover:-translate-y-px active:translate-y-0 rounded-[var(--radius-sm)] cursor-pointer transition-all duration-[150ms] whitespace-nowrap font-[var(--font-ui)] border-none"
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
          {sortedProxies.map((proxy) => (
            <div
              key={proxy.id}
              className="item-card"
            >
              <div className="item-info">
                <p className="item-name">
                  <EmojiText text={proxy.name} />
                </p>
                <p className="item-detail">
                  {proxy.type.toUpperCase()} • {proxy.host}:{proxy.port}
                </p>
              </div>
              <div className="item-actions">
                <button
                  type="button"
                  onClick={() => handleEditProxy(proxy)}
                  className="btn-icon icon-btn-edit"
                  title={t.proxyTab.editTooltip}
                >
                  <Edit2 size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => handleDeleteProxy(proxy.id)}
                  className="btn-icon icon-btn-delete"
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
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border border-[1.5px] border-[var(--glass-border)] rounded-[4px] bg-[var(--bg-primary)] cursor-pointer relative transition-all duration-[150ms] flex-shrink-0 inline-flex items-center justify-center vertical-align-middle checked:bg-[var(--accent-primary)] checked:border-[var(--accent-primary)] checked:shadow-[var(--glow-primary)] hover:border-[var(--accent-primary)] hover:bg-[var(--bg-tertiary)] focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)]"
            />
            <label htmlFor="synced-footer-proxy" className="text-sm font-medium text-[var(--text-secondary)] cursor-pointer">
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

      <ConfirmationModal
        isOpen={!!proxyToDelete}
        title={t.proxyTab.deleteTooltip}
        message={t.proxyTab.deleteInUseConfirmation}
        onConfirm={confirmDeleteProxy}
        onCancel={() => setProxyToDelete(null)}
        type="danger"
      />
    </div>
  );
};
