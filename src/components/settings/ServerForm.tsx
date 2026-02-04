import { useState, useImperativeHandle, forwardRef } from 'react';
import { Settings, Network, Terminal, Code, Trash2, Edit2, Check, X, Bot } from 'lucide-react';
import { Server, Authentication, ProxyConfig, PortForward } from '../../types/config';
import { validateRequired, validateUniqueName, validatePort } from '../../utils/validation';
import { useTranslation } from '../../i18n';
import { CustomSelect } from '../CustomSelect';
import { SnippetsTab } from './SnippetsTab';

interface ServerFormProps {
  server?: Server;
  existingNames: string[];
  availableAuths: Authentication[];
  availableProxies: ProxyConfig[];
  availableServers: Server[]; // For jumphost selection
  globalSnippetGroups?: string[];
  onSave: (server: Server) => void;
}

export interface ServerFormHandle {
  submit: () => void;
  synced: boolean;
  setSynced: (synced: boolean) => void;
}

type TabId = 'general' | 'routing' | 'advanced' | 'snippets' | 'ai';

export const ServerForm = forwardRef<ServerFormHandle, ServerFormProps>(({
  server,
  existingNames,
  availableAuths,
  availableProxies,
  availableServers,
  globalSnippetGroups = [],
  onSave,
}, ref) => {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<TabId>('general');
  const [formData, setFormData] = useState<Server>(() => {
    if (server) {
      // Merge existing server data with default values to handle potential missing fields (dirty data)
      return {
        id: server.id || '',
        name: server.name || '',
        host: server.host || '',
        port: server.port || 22,
        username: server.username || '',
        authId: server.authId || null,
        proxyId: server.proxyId || null,
        jumphostId: server.jumphostId || null,
        portForwards: server.portForwards || [],
        keepAlive: server.keepAlive || 0,
        autoExecCommands: server.autoExecCommands || [],
        envVars: server.envVars || {},
        snippets: server.snippets || [],
        synced: server.synced !== undefined ? server.synced : true,
        updatedAt: server.updatedAt || new Date().toISOString(),
        additionalPrompt: server.additionalPrompt || '',
      };
    }
    return {
      id: '',
      name: '',
      host: '',
      port: 22,
      username: '',
      authId: null,
      proxyId: null,
      jumphostId: null,
      portForwards: [],
      keepAlive: 0,
      autoExecCommands: [],
      envVars: {},
      snippets: [],
      synced: true,
      updatedAt: new Date().toISOString(),
      additionalPrompt: '',
    };
  });

  const [errors, setErrors] = useState<Record<string, string>>({});

  // Env var editing state
  const [editingEnvVar, setEditingEnvVar] = useState<{ key: string; newKey: string; newValue: string } | null>(null);

  // Port forward input state
  const [newPortForward, setNewPortForward] = useState({ local: '', remote: '' });

  const validateForm = (): boolean => {
    const newErrors: Record<string, string> = {};

    const nameError = validateRequired(formData.name, t.common.name);
    if (nameError) newErrors.name = nameError;

    const uniqueError = validateUniqueName(formData.name, existingNames, server?.name);
    if (uniqueError) newErrors.name = uniqueError;

    const hostError = validateRequired(formData.host, t.common.host);
    if (hostError) newErrors.host = hostError;

    const portError = validatePort(formData.port, t.common.port);
    if (portError) newErrors.port = portError;

    // Username and auth are optional - will be prompted at connect time if missing

    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSave = () => {
    if (validateForm()) {
      const serverToSave = { 
        ...formData,
        updatedAt: new Date().toISOString()
      };
      onSave(serverToSave);
    }
  };

  // Expose submit method to parent via ref
  useImperativeHandle(ref, () => ({
    submit: handleSave,
    synced: formData.synced,
    setSynced: (synced: boolean) => handleChange('synced', synced),
  }));

  const handleChange = (field: keyof Server, value: any) => {
    setFormData((prev) => ({
      ...prev,
      [field]: value,
    }));
    if (errors[field]) {
      setErrors((prev) => {
        const newErrors = { ...prev };
        delete newErrors[field];
        return newErrors;
      });
    }
  };

  const handleProxyOrJumphostChange = (type: 'proxy' | 'jumphost', value: string) => {
    if (type === 'proxy') {
      setFormData((prev) => ({
        ...prev,
        proxyId: value || null,
        jumphostId: null, // Clear jumphost
      }));
    } else {
      setFormData((prev) => ({
        ...prev,
        jumphostId: value || null,
        proxyId: null, // Clear proxy
      }));
    }
  };

  const addPortForward = () => {
    const localPort = parseInt(newPortForward.local, 10);
    const remotePort = parseInt(newPortForward.remote, 10);

    if (isNaN(localPort) || isNaN(remotePort)) {
      return;
    }

    const portForward: PortForward = {
      local: localPort,
      remote: remotePort,
    };

    setFormData((prev) => ({
      ...prev,
      portForwards: [...prev.portForwards, portForward],
    }));

    setNewPortForward({ local: '', remote: '' });
  };

  const removePortForward = (index: number) => {
    setFormData((prev) => ({
      ...prev,
      portForwards: prev.portForwards.filter((_, i) => i !== index),
    }));
  };

  const handleEnvVarChange = (key: string, value: string) => {
    setFormData((prev) => ({
      ...prev,
      envVars: {
        ...prev.envVars,
        [key]: value,
      },
    }));
  };

  const removeEnvVar = (key: string) => {
    setFormData((prev) => {
      const newEnvVars = { ...prev.envVars };
      delete newEnvVars[key];
      return {
        ...prev,
        envVars: newEnvVars,
      };
    });
  };

  const handleAutoExecCommandChange = (index: number, value: string) => {
    setFormData((prev) => {
      const newCommands = [...prev.autoExecCommands];
      newCommands[index] = value;
      return {
        ...prev,
        autoExecCommands: newCommands,
      };
    });
  };

  const removeAutoExecCommand = (index: number) => {
    setFormData((prev) => ({
      ...prev,
      autoExecCommands: prev.autoExecCommands.filter((_, i) => i !== index),
    }));
  };

  const addAutoExecCommand = () => {
    setFormData((prev) => ({
      ...prev,
      autoExecCommands: [...prev.autoExecCommands, ''],
    }));
  };

  const hasTabError = (tab: TabId) => {
    const fieldsByTab: Record<TabId, string[]> = {
      general: ['name', 'host', 'port', 'username', 'authId'],
      routing: ['proxyId', 'jumphostId', 'keepAlive'],
      advanced: ['portForwards', 'autoExecCommands', 'envVars'],
      snippets: [],
      ai: [],
    };
    return fieldsByTab[tab].some(field => errors[field]);
  };

  const tabs = [
    { id: 'general' as TabId, label: t.general, icon: <Settings size={18} /> },
    { id: 'routing' as TabId, label: t.routing, icon: <Network size={18} /> },
    { id: 'advanced' as TabId, label: t.advanced, icon: <Terminal size={18} /> },
    { id: 'snippets' as TabId, label: t.snippetsTab.title, icon: <Code size={18} /> },
    { id: 'ai' as TabId, label: t.ai.tabTitle, icon: <Bot size={18} /> },
  ];

  return (
    <div className="flex h-full min-h-[400px] items-stretch">
      {/* Sidebar */}
      <div className="w-[200px] bg-[var(--bg-primary)] border-r border-[var(--glass-border)] p-3 flex flex-col gap-1">
        {tabs.map((tab) => {
          const hasError = hasTabError(tab.id);
          const isActive = activeTab === tab.id;
          
          return (
            <button
              key={tab.id}
              type="button"
              onClick={() => setActiveTab(tab.id)}
              className={`
                w-full text-left px-3 py-2.5 rounded bg-transparent border-none
                text-[var(--text-secondary)] cursor-pointer text-[13px] font-medium
                transition-all duration-200 flex items-center gap-2.5 relative
                hover:bg-[rgba(255,255,255,0.03)] hover:text-[var(--text-primary)]
                ${isActive ? 'active' : ''}
              `}
              style={
                isActive
                  ? {
                      background: 'var(--bg-tertiary)',
                      color: 'var(--accent-primary)',
                      boxShadow: '0 1px 2px rgba(0, 0, 0, 0.1), inset 0 1px 0 rgba(255, 255, 255, 0.05)'
                    }
                  : {}
              }
            >
              {tab.icon}
              <span>{tab.label}</span>
              {hasError && <span className="ml-auto w-2 h-2 rounded-full bg-red-500" />}
            </button>
          );
        })}
      </div>

      {/* Content */}
      <div className="flex-1 p-6 overflow-y-auto bg-[var(--bg-primary)]">
        <div className="space-y-6">
          {activeTab === 'general' && (
            <>
              {/* Name */}
              <div>
                <label htmlFor="server-name" className="block text-sm font-medium text-gray-300 mb-1">
                  {t.serverForm.nameLabel}
                </label>
                <input
                  id="server-name"
                  type="text"
                  value={formData.name}
                  onChange={(e) => handleChange('name', e.target.value)}
                  placeholder={t.serverForm.namePlaceholder}
                  className={`w-full px-3 py-2 rounded-md border ${
                    errors.name ? 'border-red-500' : 'border-gray-600'
                  } bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-blue-500`}
                />
                {errors.name && <p className="text-red-400 text-xs mt-1">{errors.name}</p>}
              </div>

              {/* Host */}
              <div>
                <label htmlFor="server-host" className="block text-sm font-medium text-gray-300 mb-1">
                  {t.common.host}
                </label>
                <input
                  id="server-host"
                  type="text"
                  value={formData.host}
                  onChange={(e) => handleChange('host', e.target.value)}
                  placeholder={t.serverForm.hostPlaceholder}
                  className={`w-full px-3 py-2 rounded-md border ${
                    errors.host ? 'border-red-500' : 'border-gray-600'
                  } bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-blue-500`}
                />
                {errors.host && <p className="text-red-400 text-xs mt-1">{errors.host}</p>}
              </div>

              {/* Port and Username (Side by Side) */}
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label htmlFor="server-port" className="block text-sm font-medium text-gray-300 mb-1">
                    {t.common.port}
                  </label>
                  <input
                    id="server-port"
                    type="number"
                    value={formData.port}
                    onChange={(e) => handleChange('port', parseInt(e.target.value, 10))}
                    min={1}
                    max={65535}
                    className={`w-full px-3 py-2 rounded-md border ${
                      errors.port ? 'border-red-500' : 'border-gray-600'
                    } bg-[var(--bg-primary)] text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-blue-500`}
                  />
                  {errors.port && <p className="text-red-400 text-xs mt-1">{errors.port}</p>}
                </div>

                {/* Username */}
                <div>
                  <div className="flex items-center gap-2">
                    <label htmlFor="server-username" className="block text-sm font-medium text-gray-300 mb-1">
                      {t.serverForm.usernameLabel}
                    </label>
                    <span className="text-xs text-gray-500">({t.common.optional})</span>
                  </div>
                  <input
                    id="server-username"
                    type="text"
                    value={formData.username}
                    onChange={(e) => handleChange('username', e.target.value)}
                    placeholder={t.serverForm.usernamePlaceholder}
                    className="w-full px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                </div>
              </div>

              {/* Authentication Selection */}
              <div>
                <div className="flex items-center gap-2">
                  <label htmlFor="server-auth" className="block text-sm font-medium text-gray-300 mb-1">
                    {t.serverForm.authLabel}
                  </label>
                  <span className="text-xs text-gray-500">({t.common.optional})</span>
                </div>
                <CustomSelect
                  id="server-auth"
                  value={formData.authId || ''}
                  onChange={(val) => handleChange('authId', val || null)}
                  options={[
                      { value: '', label: t.serverForm.authPlaceholder },
                      ...availableAuths.map(auth => ({
                          value: auth.id,
                          label: `${auth.name} (${auth.type === 'password' ? t.authTab.passwordType : t.authTab.keyType})`
                      }))
                  ]}
                />
              </div>
            </>
          )}

          {activeTab === 'routing' && (
            <>
              <h3 className="text-sm font-medium text-gray-300 mb-4 border-b border-gray-700 pb-2">
                {t.serverForm.routingTitle}
              </h3>
              
              <div className="space-y-4">
                {/* Proxy */}
                <div>
                  <label htmlFor="server-proxy" className="block text-sm font-medium text-gray-400 mb-1">
                    {t.serverForm.proxyLabel}
                  </label>
                  <CustomSelect
                    id="server-proxy"
                    value={formData.proxyId || ''}
                    onChange={(val) => handleProxyOrJumphostChange('proxy', val)}
                    options={[
                        { value: '', label: t.common.none },
                        ...[...availableProxies]
                            .sort((a, b) => a.name.localeCompare(b.name))
                            .map(proxy => ({
                                value: proxy.id,
                                label: `${proxy.name} (${proxy.type.toUpperCase()})`
                            }))
                    ]}
                  />
                </div>

                {/* Jumphost */}
                <div>
                  <label htmlFor="server-jumphost" className="block text-sm font-medium text-gray-400 mb-1">
                    {t.serverForm.jumphostLabel}
                  </label>
                  <CustomSelect
                    id="server-jumphost"
                    value={formData.jumphostId || ''}
                    onChange={(val) => handleProxyOrJumphostChange('jumphost', val)}
                    options={[
                        { value: '', label: t.common.none },
                        ...availableServers
                            .filter((s) => s.id !== server?.id) // Don't allow self as jumphost
                            .sort((a, b) => a.name.localeCompare(b.name))
                            .map((srv) => ({
                                value: srv.id,
                                label: srv.name
                            }))
                    ]}
                  />
                </div>

                {/* Keepalive Settings */}
                <div className="pt-4 border-t border-gray-700">
                  <h3 className="text-sm font-medium text-gray-300 mb-3">
                    {t.serverForm.keepaliveTitle}
                  </h3>
                  <div>
                    <label htmlFor="server-keepalive" className="block text-sm font-medium text-gray-400 mb-1">
                      {t.serverForm.keepaliveInterval}
                    </label>
                    <input
                      id="server-keepalive"
                      type="number"
                      value={formData.keepAlive}
                      onChange={(e) => handleChange('keepAlive', parseInt(e.target.value, 10))}
                      min={0}
                      className="w-full px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-blue-500"
                    />
                  </div>
                </div>
              </div>
            </>
          )}

          {activeTab === 'advanced' && (
            <div className="space-y-6">
              {/* Port Forwarding */}
              <div>
                <h3 className="text-sm font-medium text-gray-300 mb-3 border-b border-gray-700 pb-2">
                  {t.serverForm.portForwardingTitle}
                </h3>

                {/* Existing Port Forwards */}
                {formData.portForwards.length > 0 && (
                  <div className="space-y-2 mb-3">
                    {formData.portForwards.map((pf, index) => (
                      <div
                        key={`${pf.local}-${pf.remote}`}
                        className="flex items-center justify-between bg-[var(--bg-primary)] px-3 py-2 rounded-md"
                      >
                        <span className="text-gray-300 text-sm">
                          {pf.local} → {pf.remote}
                        </span>
                        <button
                          type="button"
                          onClick={() => removePortForward(index)}
                          className="text-red-400 hover:text-red-300 text-sm"
                        >
                          {t.common.remove}
                        </button>
                      </div>
                    ))}
                  </div>
                )}

                {/* Add Port Forward */}
                <div className="flex gap-2">
                  <input
                    type="number"
                    placeholder={t.serverForm.localPortPlaceholder}
                    value={newPortForward.local}
                    onChange={(e) =>
                      setNewPortForward((prev) => ({ ...prev, local: e.target.value }))
                    }
                    className="flex-1 px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                  <span className="text-gray-500 self-center">→</span>
                  <input
                    type="number"
                    placeholder={t.serverForm.remotePortPlaceholder}
                    value={newPortForward.remote}
                    onChange={(e) =>
                      setNewPortForward((prev) => ({ ...prev, remote: e.target.value }))
                    }
                    className="flex-1 px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                  <button
                    type="button"
                    onClick={addPortForward}
                    className="px-4 py-2 bg-gray-700 text-white rounded-md hover:bg-gray-600"
                  >
                    {t.common.add}
                  </button>
                </div>
              </div>

              {/* Auto-Execute Commands */}
              <div>
                <h3 className="text-sm font-medium text-gray-300 mb-3 border-b border-gray-700 pb-2">
                  {t.serverForm.autoExecTitle}
                </h3>

                {/* Existing Commands */}
                {formData.autoExecCommands.length > 0 && (
                  <div className="space-y-2 mb-3">
                    {formData.autoExecCommands.map((cmd, index) => (
                      <div key={index} className="flex gap-2">
                        <input
                          type="text"
                          value={cmd}
                          onChange={(e) => handleAutoExecCommandChange(index, e.target.value)}
                          placeholder="e.g., cd /var/log"
                          className="flex-1 px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-blue-500"
                        />
                        <button
                          type="button"
                          onClick={() => removeAutoExecCommand(index)}
                          className="text-red-400 hover:text-red-300 px-3 py-2"
                        >
                          {t.common.remove}
                        </button>
                      </div>
                    ))}
                  </div>
                )}

                <button
                  type="button"
                  onClick={addAutoExecCommand}
                  className="px-4 py-2 bg-gray-700 text-white rounded-md hover:bg-gray-600"
                >
                  {t.serverForm.addCommand}
                </button>
              </div>

              {/* Environment Variables */}
              <div>
                <h3 className="text-sm font-medium text-gray-300 mb-3 border-b border-gray-700 pb-2">
                  {t.serverForm.envVarsTitle}
                </h3>

                {/* Existing Env Vars */}
                {Object.entries(formData.envVars).length > 0 && (
                  <div className="space-y-2 mb-3">
                    {Object.entries(formData.envVars).map(([key, value]) => {
                      const isEditing = editingEnvVar?.key === key;

                      if (isEditing) {
                        return (
                          <div key={key} className="flex gap-2">
                            <input
                              type="text"
                              value={editingEnvVar.newKey}
                              onChange={(e) => setEditingEnvVar({ ...editingEnvVar, newKey: e.target.value })}
                              className="w-24 px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-blue-500"
                            />
                            <span className="text-gray-500 self-center">=</span>
                            <input
                              type="text"
                              value={editingEnvVar.newValue}
                              onChange={(e) => setEditingEnvVar({ ...editingEnvVar, newValue: e.target.value })}
                              className="flex-1 px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-primary)] focus:outline-none focus:ring-2 focus:ring-blue-500"
                            />
                             <button
                               type="button"
                               onClick={() => {
                                 const { key: oldKey, newKey, newValue } = editingEnvVar;
                                 const newEnvVars = { ...formData.envVars };
                                 delete newEnvVars[oldKey];
                                 newEnvVars[newKey] = newValue;
                                 setFormData((prev) => ({ ...prev, envVars: newEnvVars }));
                                 setEditingEnvVar(null);
                               }}
                               className="p-1.5 rounded bg-transparent border-none text-[var(--accent-success)] cursor-pointer hover:bg-[rgba(34,197,94,0.1)] transition-all"
                             >
                               <Check size={16} />
                             </button>
                             <button
                               type="button"
                               onClick={() => setEditingEnvVar(null)}
                               className="p-1.5 rounded bg-transparent border-none text-[var(--color-danger)] cursor-pointer hover:bg-[rgba(239,68,68,0.1)] transition-all"
                             >
                               <X size={16} />
                             </button>
                          </div>
                        );
                      }

                      return (
                        <div key={key} className="flex gap-2">
                          <input
                            type="text"
                            value={key}
                            disabled
                            className="w-24 px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-muted)] cursor-not-allowed"
                          />
                          <span className="text-gray-500 self-center">=</span>
                          <input
                            type="text"
                            value={value}
                            disabled
                            className="flex-1 px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-muted)] cursor-not-allowed"
                          />
                          <button
                            type="button"
                            onClick={() => setEditingEnvVar({ key, newKey: key, newValue: value })}
                            className="p-1.5 rounded bg-transparent border-none text-[var(--text-muted)] cursor-pointer hover:bg-[var(--bg-primary)] hover:text-[var(--accent-primary)] transition-all"
                            title={t.common.edit}
                          >
                            <Edit2 size={16} />
                          </button>
                          <button
                            type="button"
                            onClick={() => removeEnvVar(key)}
                            className="p-1.5 rounded bg-transparent border-none text-[var(--text-muted)] cursor-pointer hover:bg-[rgba(239,68,68,0.1)] hover:text-[var(--color-danger)] transition-all"
                            title={t.common.remove}
                          >
                            <Trash2 size={16} />
                          </button>
                        </div>
                      );
                    })}
                  </div>
                )}

                {/* Add Env Var */}
                <div className="flex gap-2">
                  <input
                    type="text"
                    placeholder={t.serverForm.varNamePlaceholder}
                    id="envVarName"
                    className="w-32 px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                  <span className="text-gray-500 self-center">=</span>
                  <input
                    type="text"
                    placeholder={t.serverForm.varValuePlaceholder}
                    id="envVarValue"
                    className="flex-1 px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                  <button
                    type="button"
                    onClick={() => {
                      const nameInput = document.getElementById('envVarName') as HTMLInputElement;
                      const valueInput = document.getElementById('envVarValue') as HTMLInputElement;
                      if (nameInput.value && valueInput.value) {
                        handleEnvVarChange(nameInput.value, valueInput.value);
                        nameInput.value = '';
                        valueInput.value = '';
                      }
                    }}
                    className="px-4 py-2 bg-gray-700 text-white rounded-md hover:bg-gray-600"
                  >
                    {t.common.add}
                  </button>
                </div>
              </div>
            </div>
          )}
          
          {activeTab === 'snippets' && (
            <SnippetsTab
              snippets={formData.snippets || []}
              onSnippetsUpdate={(newSnippets) => handleChange('snippets', newSnippets)}
              availableGroups={globalSnippetGroups}
            />
          )}

          {activeTab === 'ai' && (
             <div>
               <div className="mb-4">
                 <label htmlFor="server-additional-prompt" className="block text-sm font-medium text-gray-300 mb-1">
                    {t.ai.serverAdditionalPrompt}
                 </label>
                 <div className="text-xs text-gray-400 mb-2">
                    {t.ai.serverAdditionalPromptDesc}
                 </div>
                  <textarea
                    id="server-additional-prompt"
                    value={formData.additionalPrompt || ''}
                    onChange={(e) => handleChange('additionalPrompt', e.target.value)}
                    className="w-full h-48 px-3 py-2 rounded-md border border-gray-600 bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] placeholder:text-[13px] focus:outline-none focus:ring-2 focus:ring-blue-500"
                    placeholder="e.g. This server runs RHEL 8 with Podman. Prefer podman commands."
                    style={{ resize: 'vertical' }}
                  />
               </div>
             </div>
          )}
        </div>
      </div>
    </div>
  );
});
