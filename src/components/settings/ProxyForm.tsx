import { useState, useImperativeHandle, forwardRef } from 'react';
import { ProxyConfig } from '../../types';
import { validateRequired, validateUniqueName, validatePort } from '../../utils/validation';
import { useTranslation } from '../../i18n';

interface ProxyFormProps {
  proxy?: ProxyConfig;
  existingNames: string[];
  onSave: (proxy: ProxyConfig) => void;
}

export interface ProxyFormHandle {
  submit: () => void;
  synced: boolean;
  setSynced: (synced: boolean) => void;
}

export const ProxyForm = forwardRef<ProxyFormHandle, ProxyFormProps>(
  ({ proxy, existingNames, onSave }, ref) => {
  const { t } = useTranslation();
  const [formData, setFormData] = useState<ProxyConfig>(() => {
    if (proxy) {
      return {
        ...proxy,
        synced: proxy.synced !== undefined ? proxy.synced : true,
        updatedAt: proxy.updatedAt || new Date().toISOString(),
      };
    }
    return {
      id: '',
      name: '',
      type: 'http',
      host: '',
      port: 8080,
      username: '',
      password: '',
      ignoreSslErrors: false,
      synced: true,
      updatedAt: new Date().toISOString(),
    };
  });

  const [errors, setErrors] = useState<Record<string, string>>({});

  const validateForm = (): boolean => {
    const newErrors: Record<string, string> = {};

    const nameError = validateRequired(formData.name, t.common.name);
    if (nameError) newErrors.name = nameError;

    const uniqueError = validateUniqueName(formData.name, existingNames, proxy?.name);
    if (uniqueError) newErrors.name = uniqueError;

    const hostError = validateRequired(formData.host, t.common.host);
    if (hostError) newErrors.host = hostError;

    const portError = validatePort(formData.port, t.common.port);
    if (portError) newErrors.port = portError;

    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSave = () => {
    if (validateForm()) {
      onSave({
        ...formData,
        updatedAt: new Date().toISOString()
      });
    }
  };

  // Expose submit method to parent via ref
  useImperativeHandle(ref, () => ({
    submit: handleSave,
    synced: formData.synced,
    setSynced: (synced: boolean) => handleChange('synced', synced),
  }));

  const handleChange = (field: keyof ProxyConfig, value: any) => {
    setFormData((prev) => ({
      ...prev,
      [field]: value,
    }));
    // Clear error for this field
    if (errors[field]) {
      setErrors((prev) => {
        const newErrors = { ...prev };
        delete newErrors[field];
        return newErrors;
      });
    }
  };

  return (
    <div className="flex flex-col gap-4">
      {/* Name */}
      <div>
        <label className="block text-sm font-medium text-zinc-400 mb-1.5">
          {t.proxyForm.nameLabel}
        </label>
        <input
          type="text"
          value={formData.name}
          onChange={(e) => handleChange('name', e.target.value)}
          placeholder={t.proxyForm.namePlaceholder}
          className={`w-full px-3 py-2 text-sm rounded-md bg-[var(--bg-primary)] border outline-none transition-all ${
            errors.name ? 'border-red-500' : 'border-zinc-700/50'
          } text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]`}
        />
        {errors.name && <p className="text-red-400 text-xs mt-1">{errors.name}</p>}
      </div>

      {/* Type */}
      <div>
        <label className="block text-sm font-medium text-zinc-400 mb-2">
          {t.proxyForm.typeLabel}
        </label>
        <div className="flex gap-4">
          {(['http', 'socks5'] as const).map((type) => (
            <label key={type} className="flex items-center gap-2 cursor-pointer group">
              <input
                type="radio"
                name="proxyType"
                value={type}
                checked={formData.type === type}
                onChange={() => handleChange('type', type)}
                className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded-full bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 hover:bg-[var(--bg-tertiary)] focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] after:content-[''] after:checked:w-2 after:checked:h-2 after:checked:bg-blue-500 after:checked:rounded-full"
              />
              <span className="text-sm text-zinc-400 group-hover:text-zinc-300 transition-colors uppercase">{type}</span>
            </label>
          ))}
        </div>
      </div>

      {/* Host */}
      <div>
        <label className="block text-sm font-medium text-zinc-400 mb-1.5">
          {t.proxyForm.hostLabel}
        </label>
        <input
          type="text"
          value={formData.host}
          onChange={(e) => handleChange('host', e.target.value)}
          placeholder={t.proxyForm.hostPlaceholder}
          className={`w-full px-3 py-2 text-sm rounded-md bg-[var(--bg-primary)] border outline-none transition-all ${
            errors.host ? 'border-red-500' : 'border-zinc-700/50'
          } text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]`}
        />
        {errors.host && <p className="text-red-400 text-xs mt-1">{errors.host}</p>}
      </div>

      {/* Port */}
      <div>
        <label className="block text-sm font-medium text-zinc-400 mb-1.5">
          {t.proxyForm.portLabel}
        </label>
        <input
          type="number"
          value={formData.port}
          onChange={(e) => handleChange('port', parseInt(e.target.value, 10))}
          min={1}
          max={65535}
          className={`w-full px-3 py-2 text-sm rounded-md bg-[var(--bg-primary)] border outline-none transition-all ${
            errors.port ? 'border-red-500' : 'border-zinc-700/50'
          } text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]`}
        />
        {errors.port && <p className="text-red-400 text-xs mt-1">{errors.port}</p>}
      </div>

      {/* Optional Credentials */}
      <div className="border-t border-zinc-700/50 pt-4 mt-4">
        <h3 className="text-sm font-medium text-zinc-300 mb-3">
          {t.proxyForm.authTitle}
        </h3>

        {/* Ignore SSL checkbox - only show for HTTP proxy */}
        {formData.type === 'http' && (
          <div className="mb-4 p-3 bg-zinc-700/20 rounded-md border border-zinc-700/50">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={formData.ignoreSslErrors}
                onChange={(e) => handleChange('ignoreSslErrors', e.target.checked)}
                className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)]"
              />
              <div>
                <span className="text-sm font-medium text-zinc-300 block">{t.proxyForm.ignoreSslLabel}</span>
                <span className="text-xs text-zinc-500">{t.proxyForm.ignoreSslDesc}</span>
              </div>
            </label>
          </div>
        )}

        <div className="flex flex-col gap-3">
          {/* Username */}
          <div>
            <label className="block text-sm font-medium text-zinc-400 mb-1.5">
              {t.username}
            </label>
            <input
              type="text"
              value={formData.username || ''}
              onChange={(e) => handleChange('username', e.target.value)}
              placeholder={t.proxyForm.usernamePlaceholder}
              className="w-full px-3 py-2 text-sm rounded-md bg-[var(--bg-primary)] border border-zinc-700/50 outline-none transition-all text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]"
            />
          </div>

          {/* Password */}
          <div>
            <label className="block text-sm font-medium text-zinc-400 mb-1.5">
              {t.password}
            </label>
            <input
              type="password"
              value={formData.password || ''}
              onChange={(e) => handleChange('password', e.target.value)}
              placeholder={t.proxyForm.passwordPlaceholder}
              className="w-full px-3 py-2 text-sm rounded-md bg-[var(--bg-primary)] border border-zinc-700/50 outline-none transition-all text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]"
            />
          </div>
        </div>
      </div>
    </div>
  );
});
