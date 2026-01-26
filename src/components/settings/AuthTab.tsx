import React, { useState, useRef } from 'react';
import { Authentication } from '../../types/config';
import { FormModal } from '../FormModal';
import { AuthForm, AuthFormHandle } from './AuthForm';
import { generateId } from '../../utils/idGenerator';

interface AuthTabProps {
  authentications: Authentication[];
  onAuthUpdate: (auths: Authentication[]) => void;
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

export const AuthTab: React.FC<AuthTabProps> = ({ authentications, onAuthUpdate }) => {
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingAuth, setEditingAuth] = useState<Authentication | null>(null);
  const formRef = useRef<AuthFormHandle>(null);

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

  const handleFormSubmit = () => {
    // Trigger form validation and submission via ref
    if (formRef.current) {
      formRef.current.submit();
    }
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
                  className="icon-btn icon-btn-edit"
                  title="Edit authentication"
                >
                  <EditIcon />
                </button>
                <button
                  onClick={() => handleDeleteAuth(auth.id)}
                  className="icon-btn icon-btn-delete"
                  title="Delete authentication"
                >
                  <DeleteIcon />
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
        onSubmit={handleFormSubmit}
        submitText="Save"
      >
        <AuthForm
          ref={formRef}
          auth={editingAuth || undefined}
          existingNames={existingNames}
          onSave={handleSaveAuth}
        />
      </FormModal>
    </div>
  );
};
