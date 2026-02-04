import React, { useState, useRef, useEffect } from 'react';
import { Plus, Edit2, Trash2, Code, Folder } from 'lucide-react';
import { Snippet } from '../../types/config';
import { FormModal } from '../FormModal';
import { ConfirmationModal } from '../ConfirmationModal';
import { SnippetForm, SnippetFormHandle } from './SnippetForm';
import { generateId } from '../../utils/idGenerator';
import { useTranslation } from '../../i18n';

interface SnippetsTabProps {
  snippets: Snippet[];
  onSnippetsUpdate: (snippets: Snippet[]) => void;
  availableGroups?: string[];
}

export const SnippetsTab: React.FC<SnippetsTabProps> = ({ 
  snippets, 
  onSnippetsUpdate,
  availableGroups = []
}) => {
  const { t } = useTranslation();
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingSnippet, setEditingSnippet] = useState<Snippet | null>(null);
  const [isSynced, setIsSynced] = useState(true);
  const [snippetToDelete, setSnippetToDelete] = useState<string | null>(null);
  const formRef = useRef<SnippetFormHandle>(null);

  const existingGroups = Array.from(new Set([
    ...snippets.map(s => s.group || t.snippetForm.defaultGroup),
    ...availableGroups
  ]));

  useEffect(() => {
    if (formRef.current) {
      setIsSynced(formRef.current.synced);
    }
  }, [isFormOpen]);

  const handleAddSnippet = () => {
    setEditingSnippet(null);
    setIsSynced(true);
    setIsFormOpen(true);
  };

  const handleEditSnippet = (snippet: Snippet) => {
    setEditingSnippet(snippet);
    setIsSynced(snippet.synced !== undefined ? snippet.synced : true);
    setIsFormOpen(true);
  };

  const handleDeleteSnippet = (snippetId: string) => {
    setSnippetToDelete(snippetId);
  };

  const confirmDeleteSnippet = () => {
    if (snippetToDelete) {
      onSnippetsUpdate(snippets.filter((s) => s.id !== snippetToDelete));
      setSnippetToDelete(null);
    }
  };

  const handleSaveSnippet = (snippet: Snippet) => {
    if (editingSnippet) {
      onSnippetsUpdate(
        snippets.map((s) => (s.id === editingSnippet.id ? { ...snippet, id: s.id } : s))
      );
    } else {
      onSnippetsUpdate([...snippets, { ...snippet, id: generateId() }]);
    }
    setIsFormOpen(false);
    setEditingSnippet(null);
  };

  const handleFormSubmit = () => {
    if (formRef.current) {
      formRef.current.submit();
    }
  };

  return (
    <div className="w-full max-w-full">
      <div className="flex justify-between items-center mb-4">
        <h3 className="text-base font-semibold tracking-tight">{t.snippetsTab.title}</h3>
        <button
          type="button"
          onClick={handleAddSnippet}
          className="inline-flex items-center justify-center gap-2 px-4 py-2 text-sm font-medium rounded bg-blue-500 text-white shadow-[0_0_20px_rgba(59,130,246,0.2)] border-none cursor-pointer transition-all whitespace-nowrap hover:brightness-110 hover:-translate-y-px active:translate-y-0 font-sans"
        >
          <Plus size={16} />
          {t.snippetsTab.addSnippet}
        </button>
      </div>

      {snippets.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-12 text-center bg-[var(--bg-primary)] border-[1.5px] border-dashed border-zinc-700/50 rounded-md">
          <p className="text-sm text-[var(--text-muted)] m-0">{t.snippetsTab.emptyState}</p>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {snippets.map((snippet) => (
            <div
              key={snippet.id}
              className="flex items-center justify-between p-3 bg-[var(--bg-primary)] border-[1.5px] border-zinc-700/50 rounded-md transition-all gap-3 hover:border-blue-500 hover:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:-translate-y-px"
            >
              <div className="flex flex-col gap-1 flex-1 min-w-0">
                <p className="text-sm font-semibold text-[var(--text-primary)] m-0 whitespace-nowrap overflow-hidden text-overflow-ellipsis flex items-center gap-2">
                    <Code size={14} className="text-[var(--text-muted)]" />
                    {snippet.name}
                </p>
                <div className="flex items-center gap-2 text-xs text-[var(--text-muted)] mt-1">
                   <span className="flex items-center gap-1 bg-[var(--bg-primary)] px-1.5 py-0.5 rounded border border-zinc-700/50">
                     <Folder size={10} />
                     {snippet.group || t.snippetForm.defaultGroup}
                   </span>
                   {snippet.description && (
                     <span className="truncate max-w-[200px]">{snippet.description}</span>
                   )}
                </div>
              </div>
              <div className="flex items-center gap-1.5 flex-shrink-0">
                <button
                  type="button"
                  onClick={() => handleEditSnippet(snippet)}
                  className="inline-flex items-center justify-center w-8 h-8 p-0 bg-[var(--bg-primary)] text-[var(--text-secondary)] border border-zinc-700/50 rounded cursor-pointer transition-all hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] hover:border-blue-500"
                  title={t.snippetsTab.editTooltip}
                >
                  <Edit2 size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => handleDeleteSnippet(snippet.id)}
                  className="inline-flex items-center justify-center w-8 h-8 p-0 bg-[var(--bg-primary)] text-[var(--text-secondary)] border border-zinc-700/50 rounded cursor-pointer transition-all hover:bg-red-500/10 hover:border-red-500 hover:text-red-500"
                  title={t.snippetsTab.deleteTooltip}
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
        title={editingSnippet ? t.snippetsTab.editSnippet : t.snippetsTab.addSnippet}
        onClose={() => {
          setIsFormOpen(false);
          setEditingSnippet(null);
        }}
        onSubmit={handleFormSubmit}
        submitText={t.common.save}
        extraFooterContent={
          <div className="flex items-center gap-2 mr-auto">
            <input
              type="checkbox"
              id="synced-footer-snippet"
              checked={isSynced}
              onChange={(e) => {
                setIsSynced(e.target.checked);
                if (formRef.current) {
                  formRef.current.setSynced(e.target.checked);
                }
              }}
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <label htmlFor="synced-footer-snippet" className="text-sm font-medium text-zinc-300 cursor-pointer">
              {t.common.syncThisItem}
            </label>
          </div>
        }
      >
        <SnippetForm
          ref={formRef}
          snippet={editingSnippet || undefined}
          existingGroups={existingGroups}
          onSave={handleSaveSnippet}
        />
      </FormModal>

      <ConfirmationModal
        isOpen={!!snippetToDelete}
        title={t.snippetsTab.deleteTooltip}
        message={t.snippetsTab.deleteConfirmation}
        onConfirm={confirmDeleteSnippet}
        onCancel={() => setSnippetToDelete(null)}
        type="danger"
      />
    </div>
  );
};
