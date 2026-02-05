import React, { useState, useCallback } from 'react';
import { FolderOpen, Plus, Trash2, Check, X, GripVertical, Pencil } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { Config, EditorRule, SftpCustomCommand } from '../../types';
import { v4 as uuidv4 } from 'uuid';
import { useTranslation } from '../../i18n';
import { Cloud, CloudOff } from 'lucide-react';

interface SFTPTabProps {
  config: Config;
  onChange: (config: Config) => void;
}

export const SFTPTab: React.FC<SFTPTabProps> = ({ config, onChange }) => {
  const { t } = useTranslation();
  const [editingRule, setEditingRule] = useState<Partial<EditorRule>>({});
  const [isAdding, setIsAdding] = useState(false);
  const [editingRuleId, setEditingRuleId] = useState<string | null>(null);

  const [editingCommand, setEditingCommand] = useState<Partial<SftpCustomCommand>>({});
  const [isAddingCommand, setIsAddingCommand] = useState(false);
  const [editingCommandId, setEditingCommandId] = useState<string | null>(null);

  const editors = config.general.sftp.editors;
  const customCommands = config.sftpCustomCommands || [];

  const handleReorderEditors = useCallback((newEditors: EditorRule[]) => {
    onChange({
      ...config,
      general: {
        ...config.general,
        sftp: {
          ...config.general.sftp,
          editors: newEditors,
        },
      },
    });
  }, [config, onChange]);

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
          editors: [newRule, ...config.general.sftp.editors],
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

  const handleEditRule = (rule: EditorRule) => {
      setEditingRuleId(rule.id);
      setEditingRule({ ...rule });
      setIsAdding(false);
  };

  const handleCancelEditRule = () => {
      setEditingRuleId(null);
      setEditingRule({});
  };

  const handleSaveEditRule = () => {
      if (!editingRuleId) return;
      if (!editingRule.pattern || !editingRule.editor) return;

      const newEditors = editors.map(r => {
          if (r.id === editingRuleId) {
              return { ...r, ...editingRule } as EditorRule;
          }
          return r;
      });

      onChange({
          ...config,
          general: {
              ...config.general,
              sftp: {
                  ...config.general.sftp,
                  editors: newEditors,
              },
          },
      });

      setEditingRuleId(null);
      setEditingRule({});
  };

  const handleAddCommand = () => {
    if (!editingCommand.name || !editingCommand.pattern || !editingCommand.command) return;

    const newCommand: SftpCustomCommand = {
      id: uuidv4(),
      name: editingCommand.name,
      pattern: editingCommand.pattern,
      command: editingCommand.command,
      synced: editingCommand.synced ?? true,
      updatedAt: new Date().toISOString(),
    };

    const newCommands = [...customCommands, newCommand].sort((a, b) => a.name.localeCompare(b.name));

    onChange({
      ...config,
      sftpCustomCommands: newCommands,
    });
    setEditingCommand({});
    setIsAddingCommand(false);
  };

  const handleEditCommand = (cmd: SftpCustomCommand) => {
      setEditingCommandId(cmd.id);
      setEditingCommand({ ...cmd });
      setIsAddingCommand(false);
  };

  const handleCancelEditCommand = () => {
      setEditingCommandId(null);
      setEditingCommand({});
  };

  const handleSaveEditCommand = () => {
      if (!editingCommandId) return;
      handleUpdateCommand(editingCommandId, editingCommand);
      setEditingCommandId(null);
      setEditingCommand({});
  };

  const handleDeleteCommand = (id: string) => {
    onChange({
      ...config,
      sftpCustomCommands: customCommands.filter((c) => c.id !== id),
    });
  };

  const handleUpdateCommand = (id: string, updates: Partial<SftpCustomCommand>) => {
    const newCommands = customCommands.map(c => {
        if (c.id === id) {
            return { ...c, ...updates, updatedAt: new Date().toISOString() };
        }
        return c;
    }).sort((a, b) => a.name.localeCompare(b.name));

    onChange({
        ...config,
        sftpCustomCommands: newCommands
    });
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
        <label htmlFor="sftp-max-concurrent" className="form-label">
          {t.sftp.settings.maxConcurrentTransfers}
        </label>
        <input
          id="sftp-max-concurrent"
          type="number"
          min="1"
          max="10"
          value={config.general.sftp.maxConcurrentTransfers}
          onChange={(e) => {
            const value = Math.max(1, Math.min(10, parseInt(e.target.value) || 1));
            onChange({
              ...config,
              general: {
                ...config.general,
                sftp: {
                  ...config.general.sftp,
                  maxConcurrentTransfers: value,
                },
              },
            });
            invoke('sftp_set_max_concurrent', { max: value }).catch(console.error);
          }}
          className="form-input w-32"
        />
        <p className="mt-1.5 text-xs text-zinc-500 leading-6">
          {t.sftp.settings.maxConcurrentTransfersDesc}
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
                <th className="w-8 bg-[var(--bg-tertiary)] px-2 py-2.5 border-b border-[var(--glass-border)]"></th>
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
                  <td className="px-2 py-3 border-b border-[var(--glass-border)]"></td>
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
                  <td className="w-[15%] text-right px-4 py-3 border-b border-[var(--glass-border)]">
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
              {editors.map((rule, index) => (
                <React.Fragment key={rule.id}>
                {editingRuleId === rule.id ? (
                  <tr className="bg-[var(--bg-tertiary)]">
                    <td className="px-2 py-3 border-b border-[var(--glass-border)]"></td>
                    <td className="px-4 py-3">
                      <input
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
                    <td className="w-[15%] text-right px-4 py-3 border-b border-[var(--glass-border)]">
                      <div className="flex justify-end gap-1">
                        <button
                          type="button"
                          onClick={handleSaveEditRule}
                          className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[rgba(0,210,106,0.1)] hover:text-[var(--color-success)]"
                          title={t.common.save}
                        >
                          <Check size={16} />
                        </button>
                        <button
                          type="button"
                          onClick={handleCancelEditRule}
                          className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
                          title={t.common.cancel}
                        >
                          <X size={16} />
                        </button>
                      </div>
                    </td>
                  </tr>
                ) : (
                <tr
                  className="hover:bg-white/[0.02]"
                  draggable={editors.length > 1}
                  onDragStart={(e) => {
                    e.dataTransfer.setData('text/plain', index.toString());
                    e.dataTransfer.effectAllowed = 'move';
                  }}
                  onDragOver={(e) => {
                    e.preventDefault();
                  }}
                  onDrop={(e) => {
                    e.preventDefault();
                    const rawData = e.dataTransfer.getData('text/plain');
                    const sourceIndex = parseInt(rawData, 10);
                    if (!isNaN(sourceIndex) && sourceIndex !== index) {
                      const newEditors = [...editors];
                      const [draggedItem] = newEditors.splice(sourceIndex, 1);
                      newEditors.splice(index, 0, draggedItem);
                      handleReorderEditors(newEditors);
                    }
                  }}
                >
                  <td className="px-2 py-2 border-b border-[var(--glass-border)] last:border-b-0 text-zinc-400">
                    {editors.length > 1 ? (
                      <div className="flex items-center justify-center w-5 h-5 rounded hover:bg-[var(--bg-tertiary)] cursor-grab select-none">
                        <GripVertical size={14} />
                      </div>
                    ) : (
                      <div className="flex items-center justify-center w-5 h-5" />
                    )}
                  </td>
                  <td className="font-mono text-[var(--accent-cyan)] w-[30%] px-4 py-2 border-b border-[var(--glass-border)] last:border-b-0">
                    {rule.pattern}
                  </td>
                  <td className="w-[55%] max-w-0 overflow-hidden text-overflow-ellipsis whitespace-nowrap text-[var(--text-secondary)] px-4 py-2 border-b border-[var(--glass-border)] last:border-b-0" title={rule.editor}>
                    {rule.editor}
                  </td>
                  <td className="w-[15%] text-right px-4 py-2 border-b border-[var(--glass-border)] last:border-b-0">
                    <div className="flex justify-end gap-1">
                        <button
                            type="button"
                            onClick={() => handleEditRule(rule)}
                            className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
                            title={t.common.edit}
                        >
                            <Pencil size={14} />
                        </button>
                        <button
                        type="button"
                        onClick={() => handleDeleteRule(rule.id)}
                        className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[rgba(255,71,87,0.1)] hover:text-[var(--color-danger)]"
                        title={t.common.delete}
                        >
                        <Trash2 size={14} />
                        </button>
                    </div>
                  </td>
                </tr>
                )}
                </React.Fragment>
              ))}
              {editors.length === 0 && !isAdding && (
                <tr>
                  <td colSpan={4} className="px-8 py-8 text-center text-zinc-500 italic">
                    {t.sftp.settings.noRules}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        {editors.length > 1 && (
          <p className="mt-1.5 text-xs text-zinc-500 leading-6">
            {t.sftp.settings.priorityHint}
          </p>
        )}
      </div>

      <div className="form-group">
        <div className="flex justify-between items-center mb-3">
            <h3 className="section-title">
                {t.sftp.settings.customCommands}
            </h3>
            <button
                type="button"
                onClick={() => setIsAddingCommand(true)}
                className="btn btn-secondary btn-sm px-2.5 py-1 h-auto"
            >
                <Plus size={14} className="mr-1" />
                {t.sftp.settings.addCommand}
            </button>
        </div>

        <div className="border border-[var(--glass-border)] rounded-[var(--radius-md)] overflow-hidden bg-[var(--bg-primary)]">
          <table className="w-full border-collapse text-[13px] text-[var(--text-primary)]">
            <thead>
              <tr>
                <th className="w-16 bg-[var(--bg-tertiary)] px-2 py-2.5 text-center font-semibold text-[var(--text-secondary)] border-b border-[var(--glass-border)]">
                    {t.sftp.settings.commandSync}
                </th>
                <th className="bg-[var(--bg-tertiary)] px-4 py-2.5 text-left font-semibold text-[var(--text-secondary)] border-b border-[var(--glass-border)]">
                    {t.sftp.settings.commandName}
                </th>
                <th className="bg-[var(--bg-tertiary)] px-4 py-2.5 text-left font-semibold text-[var(--text-secondary)] border-b border-[var(--glass-border)]">
                    {t.sftp.settings.commandPattern}
                </th>
                <th className="bg-[var(--bg-tertiary)] px-4 py-2.5 text-left font-semibold text-[var(--text-secondary)] border-b border-[var(--glass-border)]">
                    {t.sftp.settings.commandExec}
                </th>
                <th className="w-[15%] text-right bg-[var(--bg-tertiary)] px-4 py-2.5 text-left font-semibold text-[var(--text-secondary)] border-b border-[var(--glass-border)]">
                  {t.common.actions}
                </th>
              </tr>
            </thead>
            <tbody>
              {isAddingCommand && (
                <tr className="bg-[var(--bg-tertiary)]">
                  <td className="px-2 py-3 border-b border-[var(--glass-border)] text-center">
                    <input
                        type="checkbox"
                        checked={editingCommand.synced ?? true}
                        onChange={(e) => setEditingCommand(prev => ({ ...prev, synced: e.target.checked }))}
                    />
                  </td>
                  <td className="px-4 py-3">
                    <input
                      type="text"
                      value={editingCommand.name || ''}
                      onChange={(e) => setEditingCommand(prev => ({ ...prev, name: e.target.value }))}
                      placeholder={t.sftp.settings.commandPlaceholderName}
                      className="form-input px-2.5 py-1.5 w-full"
                    />
                  </td>
                  <td className="px-4 py-3">
                    <input
                      type="text"
                      value={editingCommand.pattern || ''}
                      onChange={(e) => setEditingCommand(prev => ({ ...prev, pattern: e.target.value }))}
                      placeholder={t.sftp.settings.commandPlaceholderPattern}
                      className="form-input px-2.5 py-1.5 w-full"
                    />
                  </td>
                  <td className="px-4 py-3">
                    <input
                      type="text"
                      value={editingCommand.command || ''}
                      onChange={(e) => setEditingCommand(prev => ({ ...prev, command: e.target.value }))}
                      placeholder={t.sftp.settings.commandPlaceholderExec}
                      className="form-input px-2.5 py-1.5 w-full"
                    />
                  </td>
                  <td className="w-[15%] text-right px-4 py-3 border-b border-[var(--glass-border)]">
                    <div className="flex justify-end gap-1">
                      <button
                        type="button"
                        onClick={handleAddCommand}
                        className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[rgba(0,210,106,0.1)] hover:text-[var(--color-success)]"
                      >
                        <Check size={16} />
                      </button>
                      <button
                        type="button"
                        onClick={() => {
                          setIsAddingCommand(false);
                          setEditingCommand({});
                        }}
                        className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
                      >
                        <X size={16} />
                      </button>
                    </div>
                  </td>
                </tr>
              )}
              {customCommands.map((cmd) => (
                <React.Fragment key={cmd.id}>
                {editingCommandId === cmd.id ? (
                    <tr className="bg-[var(--bg-tertiary)]">
                      <td className="px-2 py-3 border-b border-[var(--glass-border)] text-center">
                        <input
                            type="checkbox"
                            checked={editingCommand.synced ?? true}
                            onChange={(e) => setEditingCommand(prev => ({ ...prev, synced: e.target.checked }))}
                        />
                      </td>
                      <td className="px-4 py-3">
                        <input
                          type="text"
                          value={editingCommand.name || ''}
                          onChange={(e) => setEditingCommand(prev => ({ ...prev, name: e.target.value }))}
                          placeholder={t.sftp.settings.commandPlaceholderName}
                          className="form-input px-2.5 py-1.5 w-full"
                        />
                      </td>
                      <td className="px-4 py-3">
                        <input
                          type="text"
                          value={editingCommand.pattern || ''}
                          onChange={(e) => setEditingCommand(prev => ({ ...prev, pattern: e.target.value }))}
                          placeholder={t.sftp.settings.commandPlaceholderPattern}
                          className="form-input px-2.5 py-1.5 w-full"
                        />
                      </td>
                      <td className="px-4 py-3">
                        <input
                          type="text"
                          value={editingCommand.command || ''}
                          onChange={(e) => setEditingCommand(prev => ({ ...prev, command: e.target.value }))}
                          placeholder={t.sftp.settings.commandPlaceholderExec}
                          className="form-input px-2.5 py-1.5 w-full"
                        />
                      </td>
                      <td className="w-[15%] text-right px-4 py-3 border-b border-[var(--glass-border)]">
                        <div className="flex justify-end gap-1">
                          <button
                            type="button"
                            onClick={handleSaveEditCommand}
                            className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[rgba(0,210,106,0.1)] hover:text-[var(--color-success)]"
                          >
                            <Check size={16} />
                          </button>
                          <button
                            type="button"
                            onClick={handleCancelEditCommand}
                            className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
                          >
                            <X size={16} />
                          </button>
                        </div>
                      </td>
                    </tr>
                ) : (
                <tr
                  className="hover:bg-white/[0.02]"
                >
                  <td className="px-2 py-2 border-b border-[var(--glass-border)] last:border-b-0 text-center">
                    <button
                        type="button"
                        onClick={() => handleUpdateCommand(cmd.id, { synced: !cmd.synced })}
                        className={`border-0 bg-transparent cursor-pointer ${cmd.synced ? 'text-[var(--accent-primary)]' : 'text-zinc-600'}`}
                        title={cmd.synced ? t.sftp.settings.synced : t.sftp.settings.localOnly}
                    >
                        {cmd.synced ? <Cloud size={14} /> : <CloudOff size={14} />}
                    </button>
                  </td>
                  <td className="px-4 py-2 border-b border-[var(--glass-border)] last:border-b-0 font-medium">
                    {cmd.name}
                  </td>
                  <td className="font-mono text-[var(--accent-cyan)] px-4 py-2 border-b border-[var(--glass-border)] last:border-b-0">
                    {cmd.pattern}
                  </td>
                  <td className="font-mono text-[var(--text-secondary)] px-4 py-2 border-b border-[var(--glass-border)] last:border-b-0 overflow-hidden text-overflow-ellipsis whitespace-nowrap max-w-[200px]" title={cmd.command}>
                    {cmd.command}
                  </td>
                  <td className="w-[15%] text-right px-4 py-2 border-b border-[var(--glass-border)] last:border-b-0">
                    <div className="flex justify-end gap-1">
                        <button
                            type="button"
                            onClick={() => handleEditCommand(cmd)}
                            className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
                            title={t.common.edit}
                        >
                            <Pencil size={14} />
                        </button>
                        <button
                        type="button"
                        onClick={() => handleDeleteCommand(cmd.id)}
                        className="inline-flex items-center justify-center w-7 h-7 rounded bg-transparent border-0 cursor-pointer transition-all text-zinc-500 hover:bg-[rgba(255,71,87,0.1)] hover:text-[var(--color-danger)]"
                        title={t.common.delete}
                        >
                        <Trash2 size={14} />
                        </button>
                    </div>
                  </td>
                </tr>
                )}
                </React.Fragment>
              ))}
              {customCommands.length === 0 && !isAddingCommand && (
                <tr>
                  <td colSpan={5} className="px-8 py-8 text-center text-zinc-500 italic">
                    {t.sftp.settings.noCommands}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        <p className="mt-1.5 text-xs text-zinc-500 leading-6">
            {t.sftp.settings.commandHelp}
        </p>
      </div>
    </div>
  );
};
