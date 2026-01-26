import { useState, useImperativeHandle, forwardRef } from 'react';
import { Server, Authentication, ProxyConfig, PortForward } from '../../types/config';
import { validateRequired, validateUniqueName, validatePort } from '../../utils/validation';
import { useTranslation } from '../../i18n';

interface ServerFormProps {
  server?: Server;
  existingNames: string[];
  availableAuths: Authentication[];
  availableProxies: ProxyConfig[];
  availableServers: Server[]; // For jumphost selection
  onSave: (server: Server) => void;
}

export interface ServerFormHandle {
  submit: () => void;
}

export const ServerForm = forwardRef<ServerFormHandle, ServerFormProps>(({
  server,
  existingNames,
  availableAuths,
  availableProxies,
  availableServers,
  onSave,
}, ref) => {
  const { t } = useTranslation();
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

    // Only validate username if no authentication is selected OR if authentication is key type
    const selectedAuth = formData.authId ? availableAuths.find((a) => a.id === formData.authId) : null;
    const shouldValidateUsername = !formData.authId || selectedAuth?.type === 'key';

    if (shouldValidateUsername) {
      const usernameError = validateRequired(formData.username, t.serverForm.usernameLabel);
      if (usernameError) newErrors.username = usernameError;
    }

    // Authentication is required
    const authError = validateRequired(formData.authId || '', t.serverForm.authLabel);
    if (authError) newErrors.authId = authError;

    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSave = () => {
    if (validateForm()) {
      // If password authentication is selected, use the username from authentication
      const serverToSave = { ...formData };
      if (formData.authId) {
        const selectedAuth = availableAuths.find((a) => a.id === formData.authId);
        if (selectedAuth?.type === 'password' && selectedAuth.username) {
          serverToSave.username = selectedAuth.username;
        }
      }
      onSave(serverToSave);
    }
  };

  // Expose submit method to parent via ref
  useImperativeHandle(ref, () => ({
    submit: handleSave,
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

  return (
    <div className="space-y-4 max-h-[600px] overflow-y-auto pr-2">
      {/* Name */}
      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {t.serverForm.nameLabel}
        </label>
        <input
          type="text"
          value={formData.name}
          onChange={(e) => handleChange('name', e.target.value)}
          placeholder={t.serverForm.namePlaceholder}
          className={`w-full px-3 py-2 rounded-md bg-gray-800 border ${
            errors.name ? 'border-red-500' : 'border-gray-600'
          } text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500`}
        />
        {errors.name && <p className="text-red-400 text-xs mt-1">{errors.name}</p>}
      </div>

      {/* Host */}
      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {t.common.host}
        </label>
        <input
          type="text"
          value={formData.host}
          onChange={(e) => handleChange('host', e.target.value)}
          placeholder={t.serverForm.hostPlaceholder}
          className={`w-full px-3 py-2 rounded-md bg-gray-800 border ${
            errors.host ? 'border-red-500' : 'border-gray-600'
          } text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500`}
        />
        {errors.host && <p className="text-red-400 text-xs mt-1">{errors.host}</p>}
      </div>

      {/* Port and Username (Side by Side) */}
      <div className="grid grid-cols-2 gap-4">
        <div>
          <label className="block text-sm font-medium text-gray-300 mb-1">
            {t.common.port}
          </label>
          <input
            type="number"
            value={formData.port}
            onChange={(e) => handleChange('port', parseInt(e.target.value, 10))}
            min={1}
            max={65535}
            className={`w-full px-3 py-2 rounded-md bg-gray-800 border ${
              errors.port ? 'border-red-500' : 'border-gray-600'
            } text-white focus:outline-none focus:ring-2 focus:ring-blue-500`}
          />
          {errors.port && <p className="text-red-400 text-xs mt-1">{errors.port}</p>}
        </div>

        {/* Username - show different UI based on authentication selection */}
        <div>
          <label className="block text-sm font-medium text-gray-300 mb-1">
            {t.serverForm.usernameLabel}
          </label>
          {formData.authId && availableAuths.find((a) => a.id === formData.authId)?.type === 'password' ? (
            <>
              <div className="w-full px-3 py-2 rounded-md bg-gray-900 border border-gray-600 text-gray-400">
                {availableAuths.find((a) => a.id === formData.authId)?.username || 'N/A'}
              </div>
              <p className="text-xs text-gray-500 mt-1">
                {t.serverForm.fromAuth}
              </p>
            </>
          ) : (
            <>
              <input
                type="text"
                value={formData.username}
                onChange={(e) => handleChange('username', e.target.value)}
                placeholder={t.serverForm.usernamePlaceholder}
                className={`w-full px-3 py-2 rounded-md bg-gray-800 border ${
                  errors.username ? 'border-red-500' : 'border-gray-600'
                } text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500`}
              />
              {errors.username && <p className="text-red-400 text-xs mt-1">{errors.username}</p>}
            </>
          )}
        </div>
      </div>

      {/* Authentication Selection */}
      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {t.serverForm.authLabel}
        </label>
        <select
          value={formData.authId || ''}
          onChange={(e) => handleChange('authId', e.target.value || null)}
          className={`w-full px-3 py-2 rounded-md bg-gray-800 border ${
            errors.authId ? 'border-red-500' : 'border-gray-600'
          } text-white focus:outline-none focus:ring-2 focus:ring-blue-500`}
        >
          <option value="">{t.serverForm.authPlaceholder}</option>
          {availableAuths.map((auth) => (
            <option key={auth.id} value={auth.id}>
              {auth.name} ({auth.type === 'password' ? t.authTab.passwordType : t.authTab.keyType})
            </option>
          ))}
        </select>
        {errors.authId && (
          <p className="text-red-400 text-xs mt-1">{errors.authId}</p>
        )}
      </div>

      {/* Proxy or Jumphost Selection */}
      <div className="border-t border-gray-700 pt-4">
        <h3 className="text-sm font-medium text-gray-300 mb-3">
          {t.serverForm.routingTitle}
        </h3>

        <div className="space-y-3">
          {/* Proxy */}
          <div>
            <label className="block text-sm font-medium text-gray-400 mb-1">
              {t.serverForm.proxyLabel}
            </label>
            <select
              value={formData.proxyId || ''}
              onChange={(e) => handleProxyOrJumphostChange('proxy', e.target.value)}
              className="w-full px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white focus:outline-none focus:ring-2 focus:ring-blue-500"
            >
              <option value="">{t.common.none}</option>
              {availableProxies.map((proxy) => (
                <option key={proxy.id} value={proxy.id}>
                  {proxy.name} ({proxy.type.toUpperCase()})
                </option>
              ))}
            </select>
          </div>

          {/* Jumphost */}
          <div>
            <label className="block text-sm font-medium text-gray-400 mb-1">
              {t.serverForm.jumphostLabel}
            </label>
            <select
              value={formData.jumphostId || ''}
              onChange={(e) => handleProxyOrJumphostChange('jumphost', e.target.value)}
              className="w-full px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white focus:outline-none focus:ring-2 focus:ring-blue-500"
            >
              <option value="">{t.common.none}</option>
              {availableServers
                .filter((s) => s.id !== server?.id) // Don't allow self as jumphost
                .map((srv) => (
                  <option key={srv.id} value={srv.id}>
                    {srv.name}
                  </option>
                ))}
            </select>
          </div>
        </div>
      </div>

      {/* Port Forwarding */}
      <div className="border-t border-gray-700 pt-4">
        <h3 className="text-sm font-medium text-gray-300 mb-3">
          {t.serverForm.portForwardingTitle}
        </h3>

        {/* Existing Port Forwards */}
        {formData.portForwards.length > 0 && (
          <div className="space-y-2 mb-3">
            {formData.portForwards.map((pf, index) => (
              <div
                key={index}
                className="flex items-center justify-between bg-gray-800 px-3 py-2 rounded-md"
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
            className="flex-1 px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
          <span className="text-gray-500 self-center">→</span>
          <input
            type="number"
            placeholder={t.serverForm.remotePortPlaceholder}
            value={newPortForward.remote}
            onChange={(e) =>
              setNewPortForward((prev) => ({ ...prev, remote: e.target.value }))
            }
            className="flex-1 px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
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

      {/* Keepalive Settings */}
      <div className="border-t border-gray-700 pt-4">
        <h3 className="text-sm font-medium text-gray-300 mb-3">
          {t.serverForm.keepaliveTitle}
        </h3>
        <div>
          <label className="block text-sm font-medium text-gray-400 mb-1">
            {t.serverForm.keepaliveInterval}
          </label>
          <input
            type="number"
            value={formData.keepAlive}
            onChange={(e) => handleChange('keepAlive', parseInt(e.target.value, 10))}
            min={0}
            className="w-full px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </div>
      </div>

      {/* Auto-Execute Commands */}
      <div className="border-t border-gray-700 pt-4">
        <h3 className="text-sm font-medium text-gray-300 mb-3">
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
                  className="flex-1 px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
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
      <div className="border-t border-gray-700 pt-4">
        <h3 className="text-sm font-medium text-gray-300 mb-3">
          {t.serverForm.envVarsTitle}
        </h3>

        {/* Existing Env Vars */}
        {Object.entries(formData.envVars).length > 0 && (
          <div className="space-y-2 mb-3">
            {Object.entries(formData.envVars).map(([key, value]) => (
              <div key={key} className="flex gap-2">
                <input
                  type="text"
                  value={key}
                  disabled
                  className="w-24 px-3 py-2 rounded-md bg-gray-900 border border-gray-600 text-gray-400 cursor-not-allowed"
                />
                <span className="text-gray-500 self-center">=</span>
                <input
                  type="text"
                  value={value}
                  onChange={(e) => handleEnvVarChange(key, e.target.value)}
                  className="flex-1 px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white focus:outline-none focus:ring-2 focus:ring-blue-500"
                />
                <button
                  type="button"
                  onClick={() => removeEnvVar(key)}
                  className="text-red-400 hover:text-red-300 px-3 py-2"
                >
                  {t.common.remove}
                </button>
              </div>
            ))}
          </div>
        )}

        {/* Add Env Var */}
        <div className="flex gap-2">
          <input
            type="text"
            placeholder={t.serverForm.varNamePlaceholder}
            id="envVarName"
            className="w-32 px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
          <span className="text-gray-500 self-center">=</span>
          <input
            type="text"
            placeholder={t.serverForm.varValuePlaceholder}
            id="envVarValue"
            className="flex-1 px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
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
  );
});
