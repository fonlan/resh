import React, { useState, useRef, useEffect, useCallback } from 'react';
import { Plus, Edit2, Trash2, Power } from 'lucide-react';
import { Server, Authentication, ProxyConfig as ProxyType, Snippet } from '../../types/config';
import { FormModal } from '../FormModal';
import { ConfirmationModal } from '../ConfirmationModal';
import { ServerForm, ServerFormHandle } from './ServerForm';
import { generateId } from '../../utils/idGenerator';
import { useTranslation } from '../../i18n';

interface ServerTabProps {
  servers: Server[];
  authentications: Authentication[];
  proxies: ProxyType[];
  snippets?: Snippet[];
  onServersUpdate: (servers: Server[]) => void;
  onConnectServer?: (serverId: string) => void;
  editServerId?: string | null;
}

export const ServerTab: React.FC<ServerTabProps> = ({
  servers,
  authentications,
  proxies,
  snippets = [],
  onServersUpdate,
  onConnectServer,
  editServerId,
}) => {
  const { t } = useTranslation();
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingServer, setEditingServer] = useState<Server | null>(null);
  const [isSynced, setIsSynced] = useState(true);
  const [serverToDelete, setServerToDelete] = useState<{ id: string; isInUse: boolean } | null>(null);
  const formRef = useRef<ServerFormHandle>(null);

  const handleAddServer = () => {
    setEditingServer(null);
    setIsSynced(true);
    setIsFormOpen(true);
  };

  const handleEditServer = useCallback((server: Server) => {
    setEditingServer(server);
    setIsSynced(server.synced !== undefined ? server.synced : true);
    setIsFormOpen(true);
  }, []);

  const handleDeleteServer = (serverId: string) => {
    const usingServers = servers.filter((s) => s.jumphostId === serverId);
    setServerToDelete({ id: serverId, isInUse: usingServers.length > 0 });
  };

  const confirmDeleteServer = () => {
    if (serverToDelete) {
      const updatedServers = servers
        .filter((s) => s.id !== serverToDelete.id)
        .map((s) => (s.jumphostId === serverToDelete.id ? { ...s, jumphostId: null } : s));
      onServersUpdate(updatedServers);
      setServerToDelete(null);
    }
  };

  const handleSaveServer = (server: Server) => {
    if (editingServer) {
      const updatedServers = servers.map((s) => (s.id === editingServer.id ? { ...server, id: s.id } : s));
      onServersUpdate(updatedServers);
    } else {
      onServersUpdate([...servers, { ...server, id: generateId() }]);
    }
    setIsFormOpen(false);
    setEditingServer(null);
  };

  const handleFormSubmit = () => {
    if (formRef.current) {
      formRef.current.submit();
    }
  };

  const lastProcessedEditServerId = useRef<string | null>(null);

  useEffect(() => {
    if (!editServerId) {
      lastProcessedEditServerId.current = null;
      return;
    }

    if (editServerId !== lastProcessedEditServerId.current) {
      const server = servers.find((s) => s.id === editServerId);
      if (server) {
        handleEditServer(server);
        lastProcessedEditServerId.current = editServerId;
      }
    }
  }, [editServerId, servers, handleEditServer]);

  useEffect(() => {
    if (isFormOpen && formRef.current) {
      setIsSynced(formRef.current.synced);
    }
  }, [isFormOpen]);

  const existingNames = servers
    .filter((s) => s.id !== editingServer?.id)
    .map((s) => s.name);

  const globalSnippetGroups = Array.from(new Set(
    snippets.map(s => s.group || t.snippetForm.defaultGroup)
  ));

  const sortedServers = [...servers].sort((a, b) => a.name.localeCompare(b.name));

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h3 className="text-[15px] font-semibold text-[var(--text-primary)] m-0">{t.serverTab.title}</h3>
        <button
          type="button"
          onClick={handleAddServer}
          className="px-4 py-2 rounded bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-hover)] border-none cursor-pointer text-[13px] font-medium flex items-center gap-2 transition-all"
        >
          <Plus size={16} />
          {t.serverTab.addServer}
        </button>
      </div>

      {sortedServers.length === 0 ? (
        <div className="bg-[var(--bg-tertiary)] border border-[var(--glass-border)] rounded-lg p-8 text-center">
          <p className="text-[var(--text-muted)] text-[13px] m-0">{t.serverTab.emptyState}</p>
        </div>
      ) : (
        <div className="space-y-2">
          {sortedServers.map((server) => {
            const auth = authentications.find((a) => a.id === server.authId);
            const proxy = proxies.find((p) => p.id === server.proxyId);
            const jumphost = servers.find((s) => s.id === server.jumphostId);

            return (
              <div
                key={server.id}
                className="bg-[var(--bg-tertiary)] border border-[var(--glass-border)] rounded-lg p-4 flex items-center justify-between hover:border-[var(--accent-primary)]/30 transition-all"
              >
                <div className="flex-1 min-w-0">
                  <p className="text-[14px] font-medium text-[var(--text-primary)] m-0 mb-1">{server.name}</p>
                  <p className="text-[12px] text-[var(--text-muted)] m-0 mb-2">
                    {server.username ? `${server.username}@` : ''}{server.host}:{server.port}
                  </p>
                  {(auth || proxy || jumphost) && (
                    <div className="flex flex-wrap gap-1.5">
                      {auth && <span className="text-[11px] px-2 py-0.5 rounded bg-[var(--bg-primary)] text-[var(--text-secondary)] border border-[var(--glass-border)]">{t.auth}: {auth.name}</span>}
                      {proxy && <span className="text-[11px] px-2 py-0.5 rounded bg-[var(--bg-primary)] text-[var(--text-secondary)] border border-[var(--glass-border)]">{t.common.proxy}: {proxy.name}</span>}
                      {jumphost && <span className="text-[11px] px-2 py-0.5 rounded bg-[var(--bg-primary)] text-[var(--text-secondary)] border border-[var(--glass-border)]">{t.serverTab.jumphost}: {jumphost.name}</span>}
                    </div>
                  )}
                </div>
                <div className="flex items-center gap-1.5 ml-4">
                  {onConnectServer && (
                    <button
                      type="button"
                      onClick={() => onConnectServer(server.id)}
                      className="p-2 rounded bg-transparent border-none text-[var(--text-muted)] cursor-pointer hover:bg-[var(--bg-primary)] hover:text-[var(--accent-primary)] transition-all"
                      title={t.serverTab.connectTooltip}
                    >
                      <Power size={14} />
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => handleEditServer(server)}
                    className="p-2 rounded bg-transparent border-none text-[var(--text-muted)] cursor-pointer hover:bg-[var(--bg-primary)] hover:text-[var(--text-primary)] transition-all"
                    title={t.serverTab.editTooltip}
                  >
                    <Edit2 size={14} />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleDeleteServer(server.id)}
                    className="p-2 rounded bg-transparent border-none text-[var(--text-muted)] cursor-pointer hover:bg-[rgba(239,68,68,0.1)] hover:text-[var(--color-danger)] transition-all"
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
        noPadding={true}
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
              className="w-4 h-4 rounded border-[var(--glass-border)] bg-[var(--bg-tertiary)] text-[var(--accent-primary)] focus:ring-2 focus:ring-[var(--accent-primary)] focus:ring-offset-0 cursor-pointer"
            />
            <label htmlFor="synced-footer" className="text-sm font-medium text-[var(--text-secondary)] cursor-pointer">
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
          globalSnippetGroups={globalSnippetGroups}
          onSave={handleSaveServer}
        />
      </FormModal>

      <ConfirmationModal
        isOpen={!!serverToDelete}
        title={t.serverTab.deleteTooltip}
        message={serverToDelete?.isInUse ? t.serverTab.deleteInUseConfirmation : t.serverTab.deleteConfirmation}
        onConfirm={confirmDeleteServer}
        onCancel={() => setServerToDelete(null)}
        type="danger"
      />
    </div>
  );
};
