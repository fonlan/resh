import React, { useState } from 'react';
import { Authentication } from '../../types/config';
import { FormModal } from '../FormModal';
import { AuthForm } from './AuthForm';
import { generateId } from '../../utils/idGenerator';

interface AuthTabProps {
  authentications: Authentication[];
  onAuthUpdate: (auths: Authentication[]) => void;
}

export const AuthTab: React.FC<AuthTabProps> = ({ authentications, onAuthUpdate }) => {
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingAuth, setEditingAuth] = useState<Authentication | null>(null);

  const existingNames = authentications
    .filter((a) => a.id !== editingAuth?.id)
    .map((a) => a.name);

  const handleAddAuth = () => {
    setEditingAuth(null);
    setIsFormOpen(true);
  };

  const handleEditAuth = (auth: Authentication) => {
    setEditingAuth(auth);
    setIsFormOpen(true);
  };

  const handleDeleteAuth = (authId: string) => {
    onAuthUpdate(authentications.filter((a) => a.id !== authId));
  };

  const handleSaveAuth = (auth: Authentication) => {
    if (editingAuth) {
      // Update existing
      onAuthUpdate(
        authentications.map((a) => (a.id === editingAuth.id ? { ...auth, id: a.id } : a))
      );
    } else {
      // Add new
      onAuthUpdate([...authentications, { ...auth, id: generateId() }]);
    }
    setIsFormOpen(false);
    setEditingAuth(null);
  };

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h3 className="text-lg font-semibold text-white">SSH Authentications</h3>
        <button
          onClick={handleAddAuth}
          className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700"
        >
          Add Authentication
        </button>
      </div>

      {authentications.length === 0 ? (
        <div className="text-center py-8 text-gray-400">
          No authentications configured. Add one to get started.
        </div>
      ) : (
        <div className="space-y-2">
          {authentications.map((auth) => (
            <div
              key={auth.id}
              className="flex items-center justify-between bg-gray-800 p-4 rounded-md"
            >
              <div className="flex-1">
                <p className="font-medium text-white">{auth.name}</p>
                <p className="text-sm text-gray-400">
                  {auth.type === 'password' ? 'Password' : 'SSH Key'} • {auth.username}
                </p>
              </div>
              <div className="flex gap-2">
                <button
                  onClick={() => handleEditAuth(auth)}
                  className="px-3 py-1 text-sm bg-gray-700 text-white rounded hover:bg-gray-600"
                >
                  Edit
                </button>
                <button
                  onClick={() => handleDeleteAuth(auth.id)}
                  className="px-3 py-1 text-sm bg-red-700 text-white rounded hover:bg-red-600"
                >
                  Delete
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      <FormModal
        isOpen={isFormOpen}
        title={editingAuth ? 'Edit Authentication' : 'Add Authentication'}
        onClose={() => {
          setIsFormOpen(false);
          setEditingAuth(null);
        }}
        onSubmit={() => {
          // Form submission is handled by AuthForm component via onSave callback
        }}
        submitText="Save"
      >
        <AuthForm
          auth={editingAuth || undefined}
          existingNames={existingNames}
          onSave={handleSaveAuth}
        />
      </FormModal>
    </div>
  );
};
