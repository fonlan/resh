import React, { useState } from 'react';
import { RefreshCw, Check, AlertCircle, Loader2 } from 'lucide-react';
import { GeneralSettings } from '../../types/config';
import { useTranslation } from '../../i18n';
import { useConfig } from '../../hooks/useConfig';

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
    <div className="tab-container space-y-6">
      {/* WebDAV Settings Section */}
      <div className="section">
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-4">
            <h3 className="section-title mb-0">{t.webdav}</h3>
            {general.webdav.enabled && (
              <button
                type="button"
                onClick={handleSync}
                disabled={syncStatus === 'syncing' || !general.webdav.url}
                className={`sync-btn ${
                  syncStatus === 'success' ? 'sync-btn-success' : 
                  syncStatus === 'error' ? 'sync-btn-error' : ''
                }`}
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
              className="checkbox"
            />
            <span className="text-sm text-gray-400">{t.common.enableSync}</span>
          </label>
        </div>
        
        {syncStatus === 'error' && syncError && (
          <div className="sync-error-message">
            <AlertCircle size={14} className="mt-0.5 shrink-0" />
            <span>{syncError}</span>
          </div>
        )}
        
        <div className={`space-y-4 ${!general.webdav.enabled ? 'opacity-50 pointer-events-none' : ''}`}>
          <div className="form-group">
            <label htmlFor="webdav-url" className="form-label">{t.webdavUrl}</label>
            <input
              id="webdav-url"
              type="text"
              value={general.webdav.url}
              onChange={(e) => handleWebDAVUpdate('url', e.target.value)}
              className="form-input"
              placeholder="https://example.com/webdav"
            />
          </div>

          <div className="form-group">
            <label htmlFor="webdav-proxy" className="form-label">{t.webdavProxy}</label>
            <select
              id="webdav-proxy"
              value={general.webdav.proxyId || ''}
              onChange={(e) => handleWebDAVUpdate('proxyId', e.target.value || null)}
              className="form-input form-select"
            >
              <option value="">{t.common.none}</option>
              {config?.proxies.map(proxy => (
                <option key={proxy.id} value={proxy.id}>{proxy.name}</option>
              ))}
            </select>
          </div>

          <div className="form-group">
            <label htmlFor="webdav-username" className="form-label">{t.username}</label>
            <input
              id="webdav-username"
              type="text"
              value={general.webdav.username}
              onChange={(e) => handleWebDAVUpdate('username', e.target.value)}
              className="form-input"
            />
          </div>

          <div className="form-group">
            <label htmlFor="webdav-password" className="form-label">{t.password}</label>
            <input
              id="webdav-password"
              type="password"
              value={general.webdav.password}
              onChange={(e) => handleWebDAVUpdate('password', e.target.value)}
              className="form-input"
            />
          </div>
        </div>
      </div>
    </div>
  );
};
