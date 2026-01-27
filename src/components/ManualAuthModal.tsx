import React from 'react';
import { ManualAuthCredentials } from '../types/config'; // I might need to define this or just inline it

interface ManualAuthModalProps {
  serverName: string;
  credentials: ManualAuthCredentials;
  onCredentialsChange: (creds: ManualAuthCredentials) => void;
  onConnect: () => void;
  onCancel: () => void;
}

export const ManualAuthModal: React.FC<ManualAuthModalProps> = ({
  serverName,
  credentials,
  onCredentialsChange,
  onConnect,
  onCancel,
}) => {
  return (
    <div className="absolute inset-0 bg-black/80 flex items-center justify-center z-10">
      <div className="bg-gray-900 p-6 rounded-lg border border-gray-700 w-full max-w-md shadow-2xl">
        <h3 className="text-lg font-semibold text-white mb-4">Manual Authentication</h3>
        <p className="text-sm text-gray-400 mb-4">
          Credentials for {serverName} not found in config. Please enter manually:
        </p>

        <div className="space-y-4">
          <div>
            <label htmlFor="manual-username" className="block text-xs text-gray-500 mb-1">
              Username
            </label>
            <input
              id="manual-username"
              type="text"
              value={credentials.username}
              onChange={(e) =>
                onCredentialsChange({ ...credentials, username: e.target.value })
              }
              className="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-white"
            />
          </div>

          <div>
            <label htmlFor="manual-password" className="block text-xs text-gray-500 mb-1">
              Password
            </label>
            <input
              id="manual-password"
              type="password"
              value={credentials.password}
              onChange={(e) =>
                onCredentialsChange({ ...credentials, password: e.target.value })
              }
              className="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-white"
              placeholder="Leave empty if using key"
            />
          </div>

          <div className="text-center text-xs text-gray-600 my-2">— OR —</div>

          <div>
            <label htmlFor="manual-key" className="block text-xs text-gray-500 mb-1">
              Private Key (PEM)
            </label>
            <textarea
              id="manual-key"
              value={credentials.privateKey}
              onChange={(e) =>
                onCredentialsChange({ ...credentials, privateKey: e.target.value })
              }
              className="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-white font-mono text-[10px]"
              rows={4}
            />
          </div>

          <div className="flex gap-3 mt-6">
            <button
              type="button"
              onClick={onCancel}
              className="flex-1 bg-gray-800 hover:bg-gray-700 text-white py-2 rounded transition-colors"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={onConnect}
              className="flex-1 bg-blue-600 hover:bg-blue-500 text-white py-2 rounded transition-colors"
            >
              Connect
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
