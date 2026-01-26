import React, { useState, useRef } from 'react';
import { Plus, Edit2, Trash2 } from 'lucide-react';
import { Authentication } from '../../types/config';
import { FormModal } from '../FormModal';
import { AuthForm, AuthFormHandle } from './AuthForm';
import { generateId } from '../../utils/idGenerator';

interface AuthTabProps {
  authentications: Authentication[];
  onAuthUpdate: (auths: Authentication[]) => void;
}

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
    <div className="tab-container">
      <div className="flex justify-between items-center mb-4">
        <h3 className="section-title">SSH Authentications</h3>
        <button
          type="button"
          onClick={handleAddAuth}
          className="btn btn-primary"
        >
          <Plus size={16} />
          Add Authentication
        </button>
      </div>

      {authentications.length === 0 ? (
        <div className="empty-state-mini">
          <p>No authentications configured. Add one to get started.</p>
        </div>
      ) : (
        <div className="item-list">
          {authentications.map((auth) => (
            <div
              key={auth.id}
              className="item-card"
            >
              <div className="item-info">
                <p className="item-name">{auth.name}</p>
                <p className="item-detail">
                  {auth.type === 'password' ? 'Password' : 'SSH Key'} • {auth.username}
                </p>
              </div>
              <div className="item-actions">
                <button
                  type="button"
                  onClick={() => handleEditAuth(auth)}
                  className="btn-icon btn-secondary"
                  title="Edit authentication"
                >
                  <Edit2 size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => handleDeleteAuth(auth.id)}
                  className="btn-icon btn-secondary hover-danger"
                  title="Delete authentication"
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
