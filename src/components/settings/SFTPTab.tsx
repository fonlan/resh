import React, { useState } from 'react';
import { FolderOpen, Plus, Trash2, Check, X } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { Config, EditorRule } from '../../types/config';
import { v4 as uuidv4 } from 'uuid';
import { useTranslation } from '../../i18n';
import './SFTPTab.css';

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
    <div className="sftp-settings-container">
      <div className="form-group">
        <label htmlFor="sftp-download-path" className="form-label">
          {t.sftp.settings.defaultDownloadPath}
        </label>
        <div className="sftp-path-input-group">
          <input
            id="sftp-download-path"
            type="text"
            value={config.general.sftp.defaultDownloadPath}
            onChange={(e) => handleDownloadPathChange(e.target.value)}
            placeholder="e.g. C:\Users\User\Downloads"
            className="form-input"
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
        <p className="sftp-desc-text">
          {t.sftp.settings.defaultDownloadPathDesc}
        </p>
      </div>

      <div className="form-group">
        <div className="sftp-rule-header">
            <h3 className="section-title">
                {t.sftp.settings.editorAssociations}
            </h3>
            <button
                type="button"
                onClick={() => setIsAdding(true)}
                className="btn btn-secondary btn-sm"
                style={{ padding: '4px 10px', height: 'auto' }}
            >
                <Plus size={14} className="mr-1" />
                {t.sftp.settings.addRule}
            </button>
        </div>
        
        <div className="sftp-rule-table-container">
          <table className="sftp-rule-table">
            <thead>
              <tr>
                <th>{t.sftp.settings.filePattern}</th>
                <th>{t.sftp.settings.editorPath}</th>
                <th className="action-cell">{t.common.actions}</th>
              </tr>
            </thead>
            <tbody>
                {isAdding && (
                    <tr className="sftp-add-rule-row">
                        <td>
                            <input
                                id="new-rule-pattern"
                                type="text"
                                value={editingRule.pattern || ''}
                                onChange={(e) => setEditingRule(prev => ({ ...prev, pattern: e.target.value }))}
                                placeholder={t.sftp.settings.patternPlaceholder}
                                className="form-input"
                                style={{ padding: '6px 10px' }}
                            />
                        </td>
                        <td>
                             <div className="flex gap-2">
                                <input
                                    id="new-rule-editor"
                                    type="text"
                                    value={editingRule.editor || ''}
                                    onChange={(e) => setEditingRule(prev => ({ ...prev, editor: e.target.value }))}
                                    placeholder={t.sftp.settings.editorPlaceholder}
                                    className="form-input"
                                    style={{ padding: '6px 10px' }}
                                />
                                <button
                                    type="button"
                                    onClick={handlePickEditor}
                                    className="btn btn-secondary btn-icon"
                                    style={{ width: '32px', height: '32px' }}
                                    title={t.sftp.settings.browseFile}
                                >
                                    <FolderOpen size={14} />
                                </button>
                            </div>
                        </td>
                        <td className="action-cell">
                            <div className="flex justify-end gap-1">
                                <button
                                    type="button"
                                    onClick={handleAddRule}
                                    className="icon-btn-action hover-success"
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
                                    className="icon-btn-action hover-muted"
                                    title={t.common.cancel}
                                >
                                    <X size={16} />
                                </button>
                            </div>
                        </td>
                    </tr>
                )}
              {config.general.sftp.editors.map((rule) => (
                <tr key={rule.id}>
                  <td className="pattern-cell">{rule.pattern}</td>
                  <td className="editor-cell" title={rule.editor}>{rule.editor}</td>
                  <td className="action-cell">
                    <button
                      type="button"
                      onClick={() => handleDeleteRule(rule.id)}
                      className="icon-btn-action hover-danger"
                      title={t.common.delete}
                    >
                      <Trash2 size={14} />
                    </button>
                  </td>
                </tr>
              ))}
              {config.general.sftp.editors.length === 0 && !isAdding && (
                <tr>
                    <td colSpan={3} className="sftp-no-rules">
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
