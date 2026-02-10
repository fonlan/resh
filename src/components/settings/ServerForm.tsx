import { Ref, useState, useImperativeHandle } from 'react';
import { Settings, Network, Terminal, Code, Bot } from 'lucide-react';
import { Server, Authentication, ProxyConfig, PortForward } from '../../types';
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
  ref?: Ref<ServerFormHandle>;
}

export interface ServerFormHandle {
  submit: () => void;
  synced: boolean;
  setSynced: (synced: boolean) => void;
}

type TabId = 'general' | 'routing' | 'advanced' | 'snippets' | 'ai';

export const ServerForm = ({ 
  server,
  existingNames,
  availableAuths,
  availableProxies,
  availableServers,
  globalSnippetGroups = [],
  onSave,
  ref
}: ServerFormProps) => {
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
      snippets: [],
      synced: true,
      updatedAt: new Date().toISOString(),
      additionalPrompt: '',
    };
  });

  const [errors, setErrors] = useState<Record<string, string>>({});

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
      advanced: ['portForwards', 'autoExecCommands'],
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
                w-full text-left px-[14px] py-2.5 rounded bg-transparent border-none
                text-[var(--text-secondary)] cursor-pointer text-[13px] font-medium
                transition-all duration-200 flex items-center gap-2.5 relative
                hover:bg-[rgba(255,255,255,0.03)] hover:text-[var(--text-primary)] hover:translate-x-0.5
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
              {isActive && (
                <span
                  className="absolute -left-3 top-[20%] bottom-[20%] w-[3px] rounded-r"
                  style={{
                    background: 'var(--accent-primary)',
                    boxShadow: '0 0 10px var(--accent-primary)'
                  }}
                />
              )}
              {tab.icon}
              <span className="relative z-[1]">{tab.label}</span>
              {hasError && <span className="ml-auto w-2 h-2 rounded-full bg-red-500 relative z-[1]" />}
            </button>
          );
        })}
      </div>

      {/* Content */}
      <div className="flex-1 p-6 overflow-y-auto bg-[var(--bg-secondary)]">
        <div className="space-y-6">
          {activeTab === 'general' && (
            <>
              {/* Name */}
              <div>
                <label htmlFor="server-name" className="block text-sm font-medium text-zinc-400 mb-1.5">
                  {t.serverForm.nameLabel}
                </label>
                <input
                  id="server-name"
                  type="text"
                  value={formData.name}
                  onChange={(e) => handleChange('name', e.target.value)}
                  placeholder={t.serverForm.namePlaceholder}
                  className={`w-full px-3 py-2 text-sm rounded-md border outline-none transition-all ${
                    errors.name ? 'border-red-500' : 'border-zinc-700/50'
                  } bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]`}
                />
                {errors.name && <p className="text-red-400 text-xs mt-1">{errors.name}</p>}
              </div>

              {/* Host */}
              <div>
                <label htmlFor="server-host" className="block text-sm font-medium text-zinc-400 mb-1.5">
                  {t.common.host}
                </label>
                <input
                  id="server-host"
                  type="text"
                  value={formData.host}
                  onChange={(e) => handleChange('host', e.target.value)}
                  placeholder={t.serverForm.hostPlaceholder}
                  className={`w-full px-3 py-2 text-sm rounded-md border outline-none transition-all ${
                    errors.host ? 'border-red-500' : 'border-zinc-700/50'
                  } bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]`}
                />
                {errors.host && <p className="text-red-400 text-xs mt-1">{errors.host}</p>}
              </div>

              {/* Port and Username (Side by Side) */}
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label htmlFor="server-port" className="block text-sm font-medium text-zinc-400 mb-1.5">
                    {t.common.port}
                  </label>
                  <input
                    id="server-port"
                    type="number"
                    value={formData.port}
                    onChange={(e) => handleChange('port', parseInt(e.target.value, 10))}
                    min={1}
                    max={65535}
                    className={`w-full px-3 py-2 text-sm rounded-md border outline-none transition-all ${
                      errors.port ? 'border-red-500' : 'border-zinc-700/50'
                    } bg-[var(--bg-primary)] text-[var(--text-primary)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]`}
                  />
                  {errors.port && <p className="text-red-400 text-xs mt-1">{errors.port}</p>}
                </div>

                {/* Username */}
                <div>
                  <div className="flex items-center gap-2">
                    <label htmlFor="server-username" className="block text-sm font-medium text-zinc-400 mb-1.5">
                      {t.serverForm.usernameLabel}
                    </label>
                    <span className="text-xs text-zinc-500 mb-1.5">({t.common.optional})</span>
                  </div>
                  <input
                    id="server-username"
                    type="text"
                    value={formData.username}
                    onChange={(e) => handleChange('username', e.target.value)}
                    placeholder={t.serverForm.usernamePlaceholder}
                    className="w-full px-3 py-2 text-sm rounded-md border border-zinc-700/50 outline-none transition-all bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]"
                  />
                </div>
              </div>

              {/* Authentication Selection */}
              <div>
                <div className="flex items-center gap-2">
                  <label htmlFor="server-auth" className="block text-sm font-medium text-zinc-400 mb-1.5">
                    {t.serverForm.authLabel}
                  </label>
                  <span className="text-xs text-zinc-500 mb-1.5">({t.common.optional})</span>
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
            <div className="space-y-4">
              <h3 className="text-sm font-medium text-zinc-300 mb-4 border-b border-zinc-700/50 pb-2">
                {t.serverForm.routingTitle}
              </h3>
              
              <div className="space-y-4">
                {/* Proxy */}
                <div className="flex flex-col gap-1.5">
                  <label htmlFor="server-proxy" className="block text-sm font-medium text-zinc-400">
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
                <div className="flex flex-col gap-1.5">
                  <label htmlFor="server-jumphost" className="block text-sm font-medium text-zinc-400">
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
                <div className="pt-4 border-t border-zinc-700/50">
                  <h3 className="text-sm font-medium text-zinc-300 mb-3">
                    {t.serverForm.keepaliveTitle}
                  </h3>
                  <div className="flex flex-col gap-1.5">
                    <label htmlFor="server-keepalive" className="block text-sm font-medium text-zinc-400">
                      {t.serverForm.keepaliveInterval}
                    </label>
                    <input
                      id="server-keepalive"
                      type="number"
                      value={formData.keepAlive}
                      onChange={(e) => handleChange('keepAlive', parseInt(e.target.value, 10))}
                      min={0}
                      className="w-full px-3 py-2 text-sm rounded-md border border-zinc-700/50 outline-none transition-all bg-[var(--bg-primary)] text-[var(--text-primary)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]"
                    />
                  </div>
                </div>
              </div>
            </div>
          )}

          {activeTab === 'advanced' && (
            <div className="space-y-6">
              {/* Port Forwarding */}
              <div>
                <h3 className="text-sm font-medium text-zinc-300 mb-3 border-b border-zinc-700/50 pb-2">
                  {t.serverForm.portForwardingTitle}
                </h3>

                {/* Existing Port Forwards */}
                {formData.portForwards.length > 0 && (
                  <div className="space-y-2 mb-3">
                    {formData.portForwards.map((pf, index) => (
                      <div
                        key={`${pf.local}-${pf.remote}`}
                        className="flex items-center justify-between bg-[var(--bg-secondary)] border border-zinc-700/50 px-3 py-2 rounded-md"
                      >
                        <span className="text-zinc-300 text-sm">
                          {pf.local} → {pf.remote}
                        </span>
                        <button
                          type="button"
                          onClick={() => removePortForward(index)}
                          className="text-red-400 hover:text-red-300 text-sm transition-colors"
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
                    className="flex-1 min-w-0 px-3 py-2 text-sm rounded-md border border-zinc-700/50 outline-none transition-all bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]"
                  />
                  <span className="text-zinc-500 self-center">→</span>
                  <input
                    type="number"
                    placeholder={t.serverForm.remotePortPlaceholder}
                    value={newPortForward.remote}
                    onChange={(e) =>
                      setNewPortForward((prev) => ({ ...prev, remote: e.target.value }))
                    }
                    className="flex-1 min-w-0 px-3 py-2 text-sm rounded-md border border-zinc-700/50 outline-none transition-all bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]"
                  />
                  <button
                    type="button"
                    onClick={addPortForward}
                    className="px-4 py-2 text-sm bg-zinc-700/30 text-white rounded-md border border-zinc-700/50 hover:bg-zinc-700/50 transition-all flex items-center justify-center whitespace-nowrap"
                  >
                    {t.common.add}
                  </button>
                </div>
              </div>

              {/* Auto-Execute Commands */}
              <div>
                <h3 className="text-sm font-medium text-zinc-300 mb-3 border-b border-zinc-700/50 pb-2">
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
                          placeholder={t.serverForm.autoExecPlaceholder}
                          className="flex-1 min-w-0 px-3 py-2 text-sm rounded-md border border-zinc-700/50 outline-none transition-all bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]"
                        />
                        <button
                          type="button"
                          onClick={() => removeAutoExecCommand(index)}
                          className="text-red-400 hover:text-red-300 px-3 py-2 text-sm flex items-center transition-colors"
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
                  className="px-4 py-2 text-sm bg-zinc-700/30 text-white rounded-md border border-zinc-700/50 hover:bg-zinc-700/50 transition-all flex items-center justify-center whitespace-nowrap"
                >
                  {t.serverForm.addCommand}
                </button>
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
                 <label htmlFor="server-additional-prompt" className="block text-sm font-medium text-zinc-400 mb-1.5">
                    {t.ai.serverAdditionalPrompt}
                 </label>
                 <div className="text-xs text-zinc-500 mb-2">
                    {t.ai.serverAdditionalPromptDesc}
                 </div>
                   <textarea
                     id="server-additional-prompt"
                     value={formData.additionalPrompt || ''}
                     onChange={(e) => handleChange('additionalPrompt', e.target.value)}
                     className="w-full h-48 px-3 py-2 text-sm rounded-md border border-zinc-700/50 outline-none transition-all bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]"
                     placeholder={t.ai.serverAdditionalPromptPlaceholder}
                     style={{ resize: 'vertical' }}
                   />
               </div>
             </div>
          )}
        </div>
      </div>
    </div>
  );
};
