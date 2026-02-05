import React, { useState, useRef, useEffect } from 'react';
import { Plus, Edit2, Trash2 } from 'lucide-react';
import { Authentication, Server as ServerType } from '../../types';
import { FormModal } from '../FormModal';
import { ConfirmationModal } from '../ConfirmationModal';
import { AuthForm, AuthFormHandle } from './AuthForm';
import { generateId } from '../../utils/idGenerator';
import { useTranslation } from '../../i18n';
import { EmojiText } from '../EmojiText';

interface AuthTabProps {
  authentications: Authentication[];
  onAuthUpdate: (auths: Authentication[]) => void;
  servers: ServerType[];
  onServersUpdate: (servers: ServerType[]) => void;
}

export const AuthTab: React.FC<AuthTabProps> = ({ 
  authentications, 
  onAuthUpdate,
  servers,
  onServersUpdate
}) => {
  const { t } = useTranslation();
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingAuth, setEditingAuth] = useState<Authentication | null>(null);
  const [isSynced, setIsSynced] = useState(true);
  const [authToDelete, setAuthToDelete] = useState<string | null>(null);
  const formRef = useRef<AuthFormHandle>(null);

  // Sync state with formref
  useEffect(() => {
    if (formRef.current) {
      setIsSynced(formRef.current.synced);
    }
  }, [isFormOpen]);

  const existingNames = authentications
    .filter((a) => a.id !== editingAuth?.id)
    .map((a) => a.name);

  const handleAddAuth = () => {
    setEditingAuth(null);
    setIsSynced(true);
    setIsFormOpen(true);
  };

  const handleEditAuth = (auth: Authentication) => {
    setEditingAuth(auth);
    setIsSynced(auth.synced !== undefined ? auth.synced : true);
    setIsFormOpen(true);
  };

  const handleDeleteAuth = (authId: string) => {
    const usingServers = servers.filter((s) => s.authId === authId);
    
    if (usingServers.length > 0) {
      setAuthToDelete(authId);
    } else {
      onAuthUpdate(authentications.filter((a) => a.id !== authId));
    }
  };

  const confirmDeleteAuth = () => {
    if (authToDelete) {
      const updatedServers = servers.map((s) => 
        s.authId === authToDelete ? { ...s, authId: null } : s
      );
      onServersUpdate(updatedServers);
      onAuthUpdate(authentications.filter((a) => a.id !== authToDelete));
      setAuthToDelete(null);
    }
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
    <div className="w-full max-w-full">
      <div className="flex justify-between items-center mb-4">
        <h3 className="text-base font-semibold text-[var(--text-primary)]">{t.authTab.title}</h3>
        <button
          type="button"
          onClick={handleAddAuth}
          className="inline-flex items-center justify-center gap-2 px-4 py-2 text-sm font-medium bg-[var(--accent-primary)] text-white shadow-[var(--glow-primary)] hover:brightness-110 hover:-translate-y-px active:translate-y-0 rounded-[var(--radius-sm)] cursor-pointer transition-all duration-[150ms] whitespace-nowrap font-[var(--font-ui)] border-none"
        >
          <Plus size={16} />
          {t.authTab.addAuth}
        </button>
      </div>

      {authentications.length === 0 ? (
        <div className="empty-state-mini">
          <p>{t.authTab.emptyState}</p>
        </div>
      ) : (
        <div className="item-list">
          {authentications.map((auth) => (
            <div
              key={auth.id}
              className="item-card"
            >
              <div className="item-info">
                <p className="item-name">
                  <EmojiText text={auth.name} />
                </p>
                <p className="item-detail">
                  {auth.type === 'password' ? t.authTab.passwordType : t.authTab.keyType}
                </p>
              </div>
              <div className="item-actions">
                <button
                  type="button"
                  onClick={() => handleEditAuth(auth)}
                  className="btn-icon icon-btn-edit"
                  title={t.authTab.editTooltip}
                >
                  <Edit2 size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => handleDeleteAuth(auth.id)}
                  className="btn-icon icon-btn-delete"
                  title={t.authTab.deleteTooltip}
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
        title={editingAuth ? t.authTab.editAuth : t.authTab.addAuth}
        onClose={() => {
          setIsFormOpen(false);
          setEditingAuth(null);
        }}
        onSubmit={handleFormSubmit}
        submitText={t.common.save}
        extraFooterContent={
          <div className="flex items-center gap-2 mr-auto">
            <input
              type="checkbox"
              id="synced-footer-auth"
              checked={isSynced}
              onChange={(e) => {
                setIsSynced(e.target.checked);
                if (formRef.current) {
                  formRef.current.setSynced(e.target.checked);
                }
              }}
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border border-[1.5px] border-[var(--glass-border)] rounded-[4px] bg-[var(--bg-primary)] cursor-pointer relative transition-all duration-[150ms] flex-shrink-0 inline-flex items-center justify-center vertical-align-middle checked:bg-[var(--accent-primary)] checked:border-[var(--accent-primary)] checked:shadow-[var(--glow-primary)] hover:border-[var(--accent-primary)] hover:bg-[var(--bg-tertiary)] focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)]"
            />
            <label htmlFor="synced-footer-auth" className="text-sm font-medium text-[var(--text-secondary)] cursor-pointer">
              {t.common.syncThisItem || 'Sync this item'}
            </label>
          </div>
        }
      >
        <AuthForm
          ref={formRef}
          auth={editingAuth || undefined}
          existingNames={existingNames}
          onSave={handleSaveAuth}
        />
      </FormModal>

      <ConfirmationModal
        isOpen={!!authToDelete}
        title={t.authTab.deleteTooltip}
        message={t.authTab.deleteInUseConfirmation}
        onConfirm={confirmDeleteAuth}
        onCancel={() => setAuthToDelete(null)}
        type="danger"
      />
    </div>
  );
};
