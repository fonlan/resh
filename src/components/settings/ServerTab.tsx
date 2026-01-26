import React, { useState, useRef } from 'react';
import { Server, Authentication, Proxy } from '../../types/config';
import { FormModal } from '../FormModal';
import { ServerForm, ServerFormHandle } from './ServerForm';
import { generateId } from '../../utils/idGenerator';

interface ServerTabProps {
  servers: Server[];
  authentications: Authentication[];
  proxies: Proxy[];
  onServersUpdate: (servers: Server[]) => void;
  onConnectServer?: (serverId: string) => void;
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

// Connect icon component (power icon)
const ConnectIcon: React.FC = () => (
  <svg
    className="w-5 h-5"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M18.36 6.64a9 9 0 1 1-12.73 0"></path>
    <line x1="12" y1="2" x2="12" y2="12"></line>
  </svg>
);

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
    console.log('ServerTab: Saving server:', server);
    if (editingServer) {
      // Update existing
      const updatedServers = servers.map((s) => (s.id === editingServer.id ? { ...server, id: s.id } : s));
      console.log('ServerTab: Updated server list:', updatedServers);
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
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h3 className="text-lg font-semibold text-white">SSH Servers</h3>
        <button
          onClick={handleAddServer}
          className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700"
        >
          Add Server
        </button>
      </div>

      {servers.length === 0 ? (
        <div className="text-center py-8 text-gray-400">
          No servers configured. Add one to get started.
        </div>
      ) : (
        <div className="space-y-2">
          {servers.map((server) => {
            const auth = authentications.find((a) => a.id === server.authId);
            const proxy = proxies.find((p) => p.id === server.proxyId);
            const jumphost = servers.find((s) => s.id === server.jumphostId);

            return (
              <div
                key={server.id}
                className="flex items-center justify-between bg-gray-800 p-4 rounded-md"
              >
                <div className="flex-1">
                  <p className="font-medium text-white">{server.name}</p>
                  <p className="text-sm text-gray-400">
                    {server.username}@{server.host}:{server.port}
                  </p>
                  {(auth || proxy || jumphost) && (
                    <div className="text-xs text-gray-500 mt-1 flex gap-2">
                      {auth && <span>Auth: {auth.name}</span>}
                      {proxy && <span>Proxy: {proxy.name}</span>}
                      {jumphost && <span>Jumphost: {jumphost.name}</span>}
                    </div>
                  )}
                </div>
                <div className="flex gap-2">
                  {onConnectServer && (
                    <button
                      onClick={() => onConnectServer(server.id)}
                      className="icon-btn icon-btn-connect"
                      title="Connect to server"
                    >
                      <ConnectIcon />
                    </button>
                  )}
                  <button
                    onClick={() => handleEditServer(server)}
                    className="icon-btn icon-btn-edit"
                    title="Edit server"
                  >
                    <EditIcon />
                  </button>
                  <button
                    onClick={() => handleDeleteServer(server.id)}
                    className="icon-btn icon-btn-delete"
                    title="Delete server"
                  >
                    <DeleteIcon />
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
