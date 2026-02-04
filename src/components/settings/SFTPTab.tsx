import React, { useState } from 'react';
import { FolderOpen, Plus, Trash2, Check, X } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { Config, EditorRule } from '../../types/config';
import { v4 as uuidv4 } from 'uuid';
import { useTranslation } from '../../i18n';

interface SFTPTabProps {
  config: Config;
  onChange: (config: Config) => void;
}

export const SFTPTab: React.FC<SFTPTabProps> = ({ config, onChange }) => {
  const { t } = useTranslation();
  const [editingRule, setEditingRule] = useState<Partial<EditorRule>>({});
  const [isAdding, setIsAdding] = useState(false);

  const handleDownloadPathChange = (path: string) => {
    onChange({
      ...config,
      general: {
        ...config.general,
        sftp: {
          ...config.general.sftp,
          defaultDownloadPath: path,
        },
      },
    });
  };

  const handlePickDownloadPath = async () => {
    try {
      const selected = await invoke<string | null>('pick_folder');
      if (selected) {
        handleDownloadPathChange(selected);
      }
    } catch (error) {
      console.error('Failed to pick folder', error);
    }
  };

  const handleAddRule = () => {
    if (!editingRule.pattern || !editingRule.editor) return;

    const newRule: EditorRule = {
      id: uuidv4(),
      pattern: editingRule.pattern,
      editor: editingRule.editor,
    };

    onChange({
      ...config,
      general: {
        ...config.general,
        sftp: {
          ...config.general.sftp,
          editors: [...config.general.sftp.editors, newRule],
        },
      },
    });
    setEditingRule({});
    setIsAdding(false);
  };

  const handleDeleteRule = (id: string) => {
    onChange({
      ...config,
      general: {
        ...config.general,
        sftp: {
          ...config.general.sftp,
          editors: config.general.sftp.editors.filter((r) => r.id !== id),
        },
      },
    });
  };

  const handlePickEditor = async () => {
    try {
      const selected = await invoke<string | null>('pick_file');
      if (selected) {
        setEditingRule(prev => ({ ...prev, editor: selected }));
      }
    } catch (error) {
      console.error('Failed to pick file', error);
    }
  };

  return (
    <div className="flex flex-col gap-8">
      <div className="form-group">
        <label htmlFor="sftp-download-path" className="form-label">
          {t.sftp.settings.defaultDownloadPath}
        </label>
        <div className="flex gap-2">
          <input
            id="sftp-download-path"
            type="text"
            value={config.general.sftp.defaultDownloadPath}
            onChange={(e) => handleDownloadPathChange(e.target.value)}
            placeholder="e.g. C:\Users\User\Downloads"
            className="form-input flex-1"
          />
          <button
            type="button"
            onClick={handlePickDownloadPath}
            className="btn btn-secondary btn-icon"
            title={t.sftp.settings.browse}
          >
            <FolderOpen size={16} />
          </button>
        </div>
        <p className="mt-1.5 text-xs text-zinc-500 leading-6">
          {t.sftp.settings.defaultDownloadPathDesc}
        </p>
      </div>

      <div className="form-group">
        <div className="flex justify-between items-center mb-3">
            <h3 className="section-title">
                {t.sftp.settings.editorAssociations}
            </h3>
            <button
                type="button"
                onClick={() => setIsAdding(true)}
                className="btn btn-secondary btn-sm px-2.5 py-1 h-auto"
            >
                <Plus size={14} className="mr-1" />
                {t.sftp.settings.addRule}
            </button>
        </div>

        <div className="border border-[var(--glass-border)] rounded-[var(--radius-md)] overflow-hidden bg-[var(--bg-primary)]">
          <table className="w-full border-collapse text-[13px] text-[var(--text-primary)]">
            <thead>
              <tr>
                <th className="bg-[var(--bg-tertiary)] px-4 py-2.5 text-left font-semibold text-[var(--text-secondary)] border-b border-[var(--glass-border)]">
                  {t.sftp.settings.filePattern}
                </th>
                <th className="bg-[var(--bg-tertiary)] px-4 py-2.5 text-left font-semibold text-[var(--text-secondary)] border-b border-[var(--glass-border)]">
                  {t.sftp.settings.editorPath}
                </th>
                <th className="w-[15%] text-right bg-[var(--bg-tertiary)] px-4 py-2.5 text-left font-semibold text-[var(--text-secondary)] border-b border-[var(--glass-border)]">
                  {t.common.actions}
                </th>
              </tr>
            </thead>
            <tbody>
                {isAdding && (
                    <tr className="bg-[var(--bg-tertiary)]">
                        <td className="px-4 py-3">
                            <input
                                id="new-rule-pattern"
                                type="text"
                                value={editingRule.pattern || ''}
                                onChange={(e) => setEditingRule(prev => ({ ...prev, pattern: e.target.value }))}
                                placeholder={t.sftp.settings.patternPlaceholder}
                                className="form-input px-2.5 py-1.5"
                            />
                        </td>
                        <td className="px-4 py-3">
                             <div className="flex gap-2">
                                <input
                                    id="new-rule-editor"
                                    type="text"
                                    value={editingRule.editor || ''}
                                    onChange={(e) => setEditingRule(prev => ({ ...prev, editor: e.target.value }))}
                                    placeholder={t.sftp.settings.editorPlaceholder}
                                    className="form-input px-2.5 py-1.5"
                                />
                                <button
                                    type="button"
                                    onClick={handlePickEditor}
                                    className="btn btn-secondary btn-icon w-8 h-8"
                                    title={t.sftp.settings.browseFile}
                                >
                                    <FolderOpen size={14} />
                                </button>
                            </div>
                        </td>
                        <td className="w-[15%] text-right px-4 py-3">
                            <div className="flex justify-end gap-1">
                                <button
                                    type="button"
                                    onClick={handleAddRule}
                                    className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[rgba(0,210,106,0.1)] hover:text-[var(--color-success)]"
                                    title={t.common.save}
                                >
                                    <Check size={16} />
                                </button>
                                <button
                                    type="button"
                                    onClick={() => {
                                        setIsAdding(false);
                                        setEditingRule({});
                                    }}
                                    className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
                                    title={t.common.cancel}
                                >
                                    <X size={16} />
                                </button>
                            </div>
                        </td>
                    </tr>
                )}
              {config.general.sftp.editors.map((rule) => (
                <tr key={rule.id} className="hover:bg-white/[0.02]">
                  <td className="font-mono text-[var(--accent-cyan)] w-[30%] px-4 py-2 border-b border-[var(--glass-border)] last:border-b-0">
                    {rule.pattern}
                  </td>
                  <td className="w-[55%] max-w-0 overflow-hidden text-overflow-ellipsis whitespace-nowrap text-[var(--text-secondary)] px-4 py-2 border-b border-[var(--glass-border)] last:border-b-0" title={rule.editor}>
                    {rule.editor}
                  </td>
                  <td className="w-[15%] text-right px-4 py-2 border-b border-[var(--glass-border)] last:border-b-0">
                    <button
                      type="button"
                      onClick={() => handleDeleteRule(rule.id)}
                      className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[rgba(255,71,87,0.1)] hover:text-[var(--color-danger)]"
                      title={t.common.delete}
                    >
                      <Trash2 size={14} />
                    </button>
                  </td>
                </tr>
              ))}
              {config.general.sftp.editors.length === 0 && !isAdding && (
                <tr>
                    <td colSpan={3} className="px-8 py-8 text-center text-zinc-500 italic">
                        {t.sftp.settings.noRules}
                    </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
};
