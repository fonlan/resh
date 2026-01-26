import React, { useState, useRef } from 'react';
import { Plus, Edit2, Trash2, Power } from 'lucide-react';
import { Server, Authentication, Proxy as ProxyType } from '../../types/config';
import { FormModal } from '../FormModal';
import { ServerForm, ServerFormHandle } from './ServerForm';
import { generateId } from '../../utils/idGenerator';

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
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingServer, setEditingServer] = useState<Server | null>(null);
  const formRef = useRef<ServerFormHandle>(null);

  const existingNames = servers
    .filter((s) => s.id !== editingServer?.id)
    .map((s) => s.name);

  const handleAddServer = () => {
    setEditingServer(null);
    setIsFormOpen(true);
  };

  const handleEditServer = (server: Server) => {
    setEditingServer(server);
    setIsFormOpen(true);
  };

  const handleDeleteServer = (serverId: string) => {
    onServersUpdate(servers.filter((s) => s.id !== serverId));
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
        <h3 className="section-title">SSH Servers</h3>
        <button
          type="button"
          onClick={handleAddServer}
          className="btn btn-primary"
        >
          <Plus size={16} />
          Add Server
        </button>
      </div>

      {servers.length === 0 ? (
        <div className="empty-state-mini">
          <p>No servers configured. Add one to get started.</p>
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
                      {auth && <span className="tag">Auth: {auth.name}</span>}
                      {proxy && <span className="tag">Proxy: {proxy.name}</span>}
                      {jumphost && <span className="tag">Jumphost: {jumphost.name}</span>}
                    </div>
                  )}
                </div>
                <div className="item-actions">
                  {onConnectServer && (
                    <button
                      type="button"
                      onClick={() => onConnectServer(server.id)}
                      className="btn-icon btn-secondary"
                      title="Connect to server"
                    >
                      <Power size={14} />
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => handleEditServer(server)}
                    className="btn-icon btn-secondary"
                    title="Edit server"
                  >
                    <Edit2 size={14} />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleDeleteServer(server.id)}
                    className="btn-icon btn-secondary hover-danger"
                    title="Delete server"
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
        title={editingServer ? 'Edit Server' : 'Add Server'}
        onClose={() => {
          setIsFormOpen(false);
          setEditingServer(null);
        }}
        onSubmit={handleFormSubmit}
        submitText="Save"
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
