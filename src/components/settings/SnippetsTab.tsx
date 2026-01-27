import React, { useState, useRef, useEffect } from 'react';
import { Plus, Edit2, Trash2, Code, Folder } from 'lucide-react';
import { Snippet } from '../../types/config';
import { FormModal } from '../FormModal';
import { SnippetForm, SnippetFormHandle } from './SnippetForm';
import { generateId } from '../../utils/idGenerator';
import { useTranslation } from '../../i18n';

interface SnippetsTabProps {
  snippets: Snippet[];
  onSnippetsUpdate: (snippets: Snippet[]) => void;
}

export const SnippetsTab: React.FC<SnippetsTabProps> = ({ 
  snippets, 
  onSnippetsUpdate
}) => {
  const { t } = useTranslation();
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingSnippet, setEditingSnippet] = useState<Snippet | null>(null);
  const [isSynced, setIsSynced] = useState(true);
  const formRef = useRef<SnippetFormHandle>(null);

  const existingGroups = Array.from(new Set(
    snippets.map(s => s.group || t.snippetForm.defaultGroup)
  ));

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
    if (window.confirm(t.snippetsTab.deleteConfirmation)) {
      onSnippetsUpdate(snippets.filter((s) => s.id !== snippetId));
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
    <div className="tab-container">
      <div className="flex justify-between items-center mb-4">
        <h3 className="section-title">{t.snippetsTab.title}</h3>
        <button
          type="button"
          onClick={handleAddSnippet}
          className="btn btn-primary"
        >
          <Plus size={16} />
          {t.snippetsTab.addSnippet}
        </button>
      </div>

      {snippets.length === 0 ? (
        <div className="empty-state-mini">
          <p>{t.snippetsTab.emptyState}</p>
        </div>
      ) : (
        <div className="item-list">
          {snippets.map((snippet) => (
            <div
              key={snippet.id}
              className="item-card"
            >
              <div className="item-info">
                <p className="item-name flex items-center gap-2">
                    <Code size={14} className="text-gray-400" />
                    {snippet.name}
                </p>
                <div className="flex items-center gap-2 text-xs text-gray-400 mt-1">
                   <span className="flex items-center gap-1 bg-gray-800 px-1.5 py-0.5 rounded border border-gray-700">
                     <Folder size={10} />
                     {snippet.group || t.snippetForm.defaultGroup}
                   </span>
                   {snippet.description && (
                     <span className="truncate max-w-[200px]">{snippet.description}</span>
                   )}
                </div>
              </div>
              <div className="item-actions">
                <button
                  type="button"
                  onClick={() => handleEditSnippet(snippet)}
                  className="btn-icon btn-secondary"
                  title={t.snippetsTab.editTooltip}
                >
                  <Edit2 size={14} />
                </button>
                <button
                  type="button"
                  onClick={() => handleDeleteSnippet(snippet.id)}
                  className="btn-icon btn-secondary hover-danger"
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
              className="checkbox"
            />
            <label htmlFor="synced-footer-snippet" className="text-sm font-medium text-gray-300 cursor-pointer">
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
    </div>
  );
};
