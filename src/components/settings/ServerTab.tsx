import React, { useState } from 'react';
import { Server, Authentication, Proxy } from '../../types/config';
import { FormModal } from '../FormModal';
import { ServerForm } from './ServerForm';
import { generateId } from '../../utils/idGenerator';

interface ServerTabProps {
  servers: Server[];
  authentications: Authentication[];
  proxies: Proxy[];
  onServersUpdate: (servers: Server[]) => void;
}

export const ServerTab: React.FC<ServerTabProps> = ({
  servers,
  authentications,
  proxies,
  onServersUpdate,
}) => {
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingServer, setEditingServer] = useState<Server | null>(null);

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
      onServersUpdate(
        servers.map((s) => (s.id === editingServer.id ? { ...server, id: s.id } : s))
      );
    } else {
      // Add new
      onServersUpdate([...servers, { ...server, id: generateId() }]);
    }
    setIsFormOpen(false);
    setEditingServer(null);
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
                  <button
                    onClick={() => handleEditServer(server)}
                    className="px-3 py-1 text-sm bg-gray-700 text-white rounded hover:bg-gray-600"
                  >
                    Edit
                  </button>
                  <button
                    onClick={() => handleDeleteServer(server.id)}
                    className="px-3 py-1 text-sm bg-red-700 text-white rounded hover:bg-red-600"
                  >
                    Delete
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
        onSubmit={() => {}}
        submitText="Save"
      >
        <ServerForm
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
