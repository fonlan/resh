import React, { useState, useRef, useEffect } from 'react';
import { Plus, Edit2, Trash2 } from 'lucide-react';
import { Authentication, Server as ServerType } from '../../types/config';
import { FormModal } from '../FormModal';
import { ConfirmationModal } from '../ConfirmationModal';
import { AuthForm, AuthFormHandle } from './AuthForm';
import { generateId } from '../../utils/idGenerator';
import { useTranslation } from '../../i18n';

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
        <h3 className="text-base font-semibold text-[var(--text-primary)] tracking-[-0.01em]">{t.authTab.title}</h3>
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
        <div className="flex flex-col items-center justify-center py-12 px-6 text-center bg-[var(--bg-primary)] border-2 border-dashed border-[var(--glass-border)] rounded-[var(--radius-md)]">
          <p className="text-sm text-[var(--text-muted)] m-0">{t.authTab.emptyState}</p>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {authentications.map((auth) => (
            <div
              key={auth.id}
              className="flex items-center justify-between p-3 px-4 bg-[var(--bg-primary)] border border-[1.5px] border-[var(--glass-border)] rounded-[var(--radius-md)] transition-all duration-[150ms] gap-3 hover:border-[var(--accent-primary)] hover:shadow-[var(--glow-primary)] hover:-translate-y-px"
            >
              <div className="flex flex-col gap-1 flex-1 min-w-0">
                <p className="text-sm font-semibold text-[var(--text-primary)] m-0 whitespace-nowrap overflow-hidden text-ellipsis">{auth.name}</p>
                <p className="text-xs text-[var(--text-secondary)] m-0 whitespace-nowrap overflow-hidden text-ellipsis">
                  {auth.type === 'password' ? t.authTab.passwordType : t.authTab.keyType}
                </p>
              </div>
              <div className="flex items-center gap-1.5 flex-shrink-0">
                <button
                  type="button"
                  onClick={() => handleEditAuth(auth)}
                  className="inline-flex items-center justify-center w-8 h-8 p-0 bg-[var(--bg-tertiary)] text-[var(--text-secondary)] border border-[1.5px] border-[var(--glass-border)] rounded-[var(--radius-sm)] cursor-pointer transition-all duration-[150ms] hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] hover:border-[var(--accent-primary)]"
                  title={t.authTab.editTooltip}
                >
                  <Edit2 size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => handleDeleteAuth(auth.id)}
                  className="inline-flex items-center justify-center w-8 h-8 p-0 bg-[var(--bg-tertiary)] text-[var(--text-secondary)] border border-[1.5px] border-[var(--glass-border)] rounded-[var(--radius-sm)] cursor-pointer transition-all duration-[150ms] hover:bg-[rgba(239,68,68,0.1)] hover:border-[var(--color-danger)] hover:text-[var(--color-danger)]"
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
