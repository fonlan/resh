import React, { useState, useRef, useEffect } from 'react';
import { Plus, Edit2, Trash2, Power } from 'lucide-react';
import { Server, Authentication, ProxyConfig as ProxyType } from '../../types/config';
import { FormModal } from '../FormModal';
import { ServerForm, ServerFormHandle } from './ServerForm';
import { generateId } from '../../utils/idGenerator';
import { useTranslation } from '../../i18n';

interface ServerTabProps {
  servers: Server[];
  authentications: Authentication[];
  proxies: ProxyType[];
  onServersUpdate: (servers: Server[]) => void;
  onConnectServer?: (serverId: string) => void;
}

export const ServerTab: React.FC<ServerTabProps> = ({
  servers,
  authentications,
  proxies,
  onServersUpdate,
  onConnectServer,
}) => {
  const { t } = useTranslation();
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingServer, setEditingServer] = useState<Server | null>(null);
  const [isSynced, setIsSynced] = useState(true);
  const formRef = useRef<ServerFormHandle>(null);

  // Sync state with formref
  useEffect(() => {
    if (formRef.current) {
      setIsSynced(formRef.current.synced);
    }
  }, [isFormOpen]);

  const existingNames = servers
    .filter((s) => s.id !== editingServer?.id)
    .map((s) => s.name);

  const handleAddServer = () => {
    setEditingServer(null);
    setIsSynced(true);
    setIsFormOpen(true);
  };

  const handleEditServer = (server: Server) => {
    setEditingServer(server);
    setIsSynced(server.synced !== undefined ? server.synced : true);
    setIsFormOpen(true);
  };

  const handleDeleteServer = (serverId: string) => {
    const usingServers = servers.filter((s) => s.jumphostId === serverId);

    if (usingServers.length > 0) {
      if (window.confirm(t.serverTab.deleteInUseConfirmation)) {
        // Clear jumphost from servers
        const updatedServers = servers
          .filter((s) => s.id !== serverId)
          .map((s) => (s.jumphostId === serverId ? { ...s, jumphostId: null } : s));
        onServersUpdate(updatedServers);
      }
    } else {
      onServersUpdate(servers.filter((s) => s.id !== serverId));
    }
  };

  const handleSaveServer = (server: Server) => {
    if (editingServer) {
      // Update existing
      const updatedServers = servers.map((s) => (s.id === editingServer.id ? { ...server, id: s.id } : s));
      onServersUpdate(updatedServers);
    } else {
      // Add new
      onServersUpdate([...servers, { ...server, id: generateId() }]);
    }
    setIsFormOpen(false);
    setEditingServer(null);
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
        <h3 className="section-title">{t.serverTab.title}</h3>
        <button
          type="button"
          onClick={handleAddServer}
          className="btn btn-primary"
        >
          <Plus size={16} />
          {t.serverTab.addServer}
        </button>
      </div>

      {servers.length === 0 ? (
        <div className="empty-state-mini">
          <p>{t.serverTab.emptyState}</p>
        </div>
      ) : (
        <div className="item-list">
          {servers.map((server) => {
            const auth = authentications.find((a) => a.id === server.authId);
            const proxy = proxies.find((p) => p.id === server.proxyId);
            const jumphost = servers.find((s) => s.id === server.jumphostId);

            return (
              <div
                key={server.id}
                className="item-card"
              >
                <div className="item-info">
                  <p className="item-name">{server.name}</p>
                  <p className="item-detail">
                    {server.username}@{server.host}:{server.port}
                  </p>
                  {(auth || proxy || jumphost) && (
                    <div className="item-tags">
                      {auth && <span className="tag">{t.auth}: {auth.name}</span>}
                      {proxy && <span className="tag">{t.proxyTab.editTooltip.split(' ')[1]}: {proxy.name}</span>}
                      {jumphost && <span className="tag">{t.serverTab.jumphost}: {jumphost.name}</span>}
                    </div>
                  )}
                </div>
                <div className="item-actions">
                  {onConnectServer && (
                    <button
                      type="button"
                      onClick={() => onConnectServer(server.id)}
                      className="btn-icon btn-secondary"
                      title={t.serverTab.connectTooltip}
                    >
                      <Power size={14} />
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => handleEditServer(server)}
                    className="btn-icon btn-secondary"
                    title={t.serverTab.editTooltip}
                  >
                    <Edit2 size={14} />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleDeleteServer(server.id)}
                    className="btn-icon btn-secondary hover-danger"
                    title={t.serverTab.deleteTooltip}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <FormModal
        isOpen={isFormOpen}
        title={editingServer ? t.serverTab.editServer : t.serverTab.addServer}
        onClose={() => {
          setIsFormOpen(false);
          setEditingServer(null);
        }}
        onSubmit={handleFormSubmit}
        submitText={t.common.save}
        extraFooterContent={
          <div className="flex items-center gap-2 mr-auto">
            <input
              type="checkbox"
              id="synced-footer"
              checked={isSynced}
              onChange={(e) => {
                setIsSynced(e.target.checked);
                if (formRef.current) {
                  formRef.current.setSynced(e.target.checked);
                }
              }}
              className="checkbox"
            />
            <label htmlFor="synced-footer" className="text-sm font-medium text-gray-300 cursor-pointer">
              {t.common.syncThisItem || 'Sync this item'}
            </label>
          </div>
        }
      >
        <ServerForm
          ref={formRef}
          server={editingServer || undefined}
          existingNames={existingNames}
          availableAuths={authentications}
          availableProxies={proxies}
          availableServers={servers}
          onSave={handleSaveServer}
        />
      </FormModal>
    </div>
  );
};
