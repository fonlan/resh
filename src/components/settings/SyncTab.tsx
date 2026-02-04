import React, { useState } from 'react';
import { RefreshCw, Check, AlertCircle, Loader2 } from 'lucide-react';
import { GeneralSettings } from '../../types/config';
import { useTranslation } from '../../i18n';
import { useConfig } from '../../hooks/useConfig';
import { CustomSelect } from '../CustomSelect';

export interface SyncTabProps {
  general: GeneralSettings;
  onGeneralUpdate: (general: GeneralSettings) => void;
}

export const SyncTab: React.FC<SyncTabProps> = ({ general, onGeneralUpdate }) => {
  const { t } = useTranslation();
  const { triggerSync, config } = useConfig();
  const [syncStatus, setSyncStatus] = useState<'idle' | 'syncing' | 'success' | 'error'>('idle');
  const [syncError, setSyncError] = useState<string | null>(null);

  const handleWebDAVUpdate = (field: keyof typeof general.webdav, value: string | boolean | null) => {
    onGeneralUpdate({
      ...general,
      webdav: { ...general.webdav, [field]: value } as any
    });
  };

  const handleSync = async () => {
    if (syncStatus === 'syncing') return;

    try {
      setSyncStatus('syncing');
      setSyncError(null);
      await triggerSync();
      setSyncStatus('success');
      setTimeout(() => setSyncStatus('idle'), 3000);
    } catch (err) {
      setSyncStatus('error');
      setSyncError(err instanceof Error ? err.message : String(err));
      setTimeout(() => setSyncStatus('idle'), 5000);
    }
  };

  return (
    <div className="w-full max-w-full space-y-6">
      {/* WebDAV Settings Section */}
      <div>
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-4">
            <h3 className="text-base font-semibold tracking-tight mb-0">{t.webdav}</h3>
            {general.webdav.enabled && (
              <button
                type="button"
                onClick={handleSync}
                disabled={syncStatus === 'syncing' || !general.webdav.url}
                className={`inline-flex items-center justify-center gap-2 px-3 py-2 text-sm font-medium rounded border-none cursor-pointer transition-all whitespace-nowrap font-sans ${
                  syncStatus === 'success' ? 'bg-green-500 text-white shadow-[0_0_20px_rgba(34,197,94,0.2)]' :
                  syncStatus === 'error' ? 'bg-red-500 text-white' :
                  'bg-[var(--bg-primary)] text-[var(--text-primary)] border border-zinc-700/50'
                } hover:brightness-110 hover:-translate-y-px active:translate-y-0 disabled:opacity-50 disabled:cursor-not-allowed`}
                title={t.syncNow}
              >
                {syncStatus === 'syncing' ? (
                  <Loader2 size={14} className="animate-spin" />
                ) : syncStatus === 'success' ? (
                  <Check size={14} />
                ) : syncStatus === 'error' ? (
                  <AlertCircle size={14} />
                ) : (
                  <RefreshCw size={14} />
                )}
                <span>
                  {syncStatus === 'syncing' ? t.syncing :
                   syncStatus === 'success' ? t.syncSuccess :
                   syncStatus === 'error' ? t.syncFailed : t.syncNow}
                </span>
              </button>
            )}
          </div>
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.webdav.enabled}
              onChange={(e) => handleWebDAVUpdate('enabled', e.target.checked)}
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <span className="text-sm text-zinc-400">{t.common.enableSync}</span>
          </label>
        </div>

        {syncStatus === 'error' && syncError && (
          <div className="flex items-center gap-2 p-3 my-3 bg-red-500/10 border border-red-500/30 rounded-md text-red-400 text-sm">
            <AlertCircle size={14} className="mt-0.5 shrink-0" />
            <span>{syncError}</span>
          </div>
        )}

        <div className={`space-y-4 ${!general.webdav.enabled ? 'opacity-50 pointer-events-none' : ''}`}>
          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="webdav-url" className="block text-sm font-medium text-zinc-400 mb-1.5 tracking-tight">{t.webdavUrl}</label>
            <input
              id="webdav-url"
              type="text"
              value={general.webdav.url}
              onChange={(e) => handleWebDAVUpdate('url', e.target.value)}
              className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
              placeholder="https://example.com/webdav"
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="webdav-proxy" className="block text-sm font-medium text-zinc-400 mb-1.5 tracking-tight">{t.webdavProxy}</label>
            <CustomSelect
              id="webdav-proxy"
              value={general.webdav.proxyId || ''}
              onChange={(val) => handleWebDAVUpdate('proxyId', val || null)}
              options={[
                { value: '', label: t.common.none },
                ...(config?.proxies || []).map(proxy => ({
                    value: proxy.id,
                    label: proxy.name
                }))
              ]}
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="webdav-username" className="block text-sm font-medium text-zinc-400 mb-1.5 tracking-tight">{t.username}</label>
            <input
              id="webdav-username"
              type="text"
              value={general.webdav.username}
              onChange={(e) => handleWebDAVUpdate('username', e.target.value)}
              className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="webdav-password" className="block text-sm font-medium text-zinc-400 mb-1.5 tracking-tight">{t.password}</label>
            <input
              id="webdav-password"
              type="password"
              value={general.webdav.password}
              onChange={(e) => handleWebDAVUpdate('password', e.target.value)}
              className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
            />
          </div>
        </div>
      </div>
    </div>
  );
};
