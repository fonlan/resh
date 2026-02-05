import React, { useRef } from 'react';
import type { ManualAuthCredentials } from '../types';
import { useTranslation } from '../i18n';
import { Upload } from 'lucide-react';

interface ManualAuthModalProps {
  serverName: string;
  credentials: ManualAuthCredentials;
  onCredentialsChange: (creds: ManualAuthCredentials) => void;
  onConnect: () => void;
  onCancel: () => void;
  isRetry?: boolean;
}

export const ManualAuthModal: React.FC<ManualAuthModalProps> = ({
  serverName,
  credentials,
  onCredentialsChange,
  onConnect,
  onCancel,
  isRetry = false,
}) => {
  const { t } = useTranslation();
  const fileInputRef = useRef<HTMLInputElement>(null);
  const modalRef = useRef<HTMLDivElement>(null);
  const mouseDownInsideRef = useRef(false);

  const handleFileUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = (event) => {
      const content = event.target?.result as string;
      if (content) {
        onCredentialsChange({ ...credentials, privateKey: content });
      }
    };
    reader.readAsText(file);
    e.target.value = '';
  };

  const handleOverlayMouseDown = (e: React.MouseEvent) => {
    if (modalRef.current && modalRef.current.contains(e.target as Node)) {
      mouseDownInsideRef.current = true;
    } else {
      mouseDownInsideRef.current = false;
    }
  };

  const handleOverlayMouseUp = (e: React.MouseEvent) => {
    if (!mouseDownInsideRef.current && modalRef.current && !modalRef.current.contains(e.target as Node)) {
      onCancel();
    }
    mouseDownInsideRef.current = false;
  };

  return (
    <div 
      className="fixed inset-0 flex items-center justify-center z-[1000] animate-in fade-in duration-300"
      style={{
        background: 'rgba(2, 6, 23, 0.4)',
        backdropFilter: 'blur(12px) saturate(180%)'
      }}
      onMouseDown={handleOverlayMouseDown}
      onMouseUp={handleOverlayMouseUp}
    >
      <div 
        ref={modalRef}
        className="relative bg-[var(--bg-secondary)] rounded-lg w-full max-w-[480px] flex flex-col overflow-hidden animate-in slide-in-from-bottom-2 duration-400"
        style={{
          boxShadow: '0 25px 50px -12px rgba(0, 0, 0, 0.5), 0 0 0 1px var(--glass-border), inset 0 1px 1px rgba(255, 255, 255, 0.05)'
        }}
      >
        <div className="flex items-center justify-between px-6 py-4 border-b border-[var(--glass-border)] bg-[var(--bg-secondary)]">
          <h2 className="text-[18px] font-bold text-[var(--text-primary)] m-0">
            {isRetry ? t.manualAuth.retryTitle : t.manualAuth.title}
          </h2>
        </div>

        <div className="p-6 overflow-y-auto bg-[var(--bg-secondary)]">
          <p className="text-sm mb-5 text-zinc-400">
            {isRetry ? t.manualAuth.retryDescription.replace('{server}', serverName) : t.manualAuth.enterCredentials.replace('{server}', serverName)}
          </p>

          <div className="mb-4">
            <label htmlFor="manual-username" className="block text-sm font-medium text-zinc-400 mb-1.5">
              {t.manualAuth.usernameLabel}
            </label>
            <input
              id="manual-username"
              type="text"
              value={credentials.username}
              onChange={(e) =>
                onCredentialsChange({ ...credentials, username: e.target.value })
              }
              className="w-full px-3 py-2 text-sm rounded-md border border-zinc-700/50 outline-none transition-all bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]"
            />
          </div>

          <div className="mb-4">
            <label htmlFor="manual-password" className="block text-sm font-medium text-zinc-400 mb-1.5">
              {t.manualAuth.passwordLabel}
            </label>
            <input
              id="manual-password"
              type="password"
              value={credentials.password}
              onChange={(e) =>
                onCredentialsChange({ ...credentials, password: e.target.value })
              }
              className="w-full px-3 py-2 text-sm rounded-md border border-zinc-700/50 outline-none transition-all bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)]"
              placeholder={t.manualAuth.passwordPlaceholder}
            />
          </div>

          <div className="text-center text-xs text-zinc-500 my-4">
            {t.manualAuth.orDivider}
          </div>

          <div className="mb-4">
            <div className="flex justify-between items-center mb-1.5">
              <label htmlFor="manual-key" className="block text-sm font-medium text-zinc-400 mb-0">
                {t.manualAuth.privateKeyLabel}
              </label>
              <button
                type="button"
                onClick={() => fileInputRef.current?.click()}
                className="inline-flex items-center justify-center gap-2 px-2.5 py-1 text-xs font-medium bg-zinc-700/20 text-zinc-300 border border-zinc-700/50 rounded-md hover:bg-zinc-700/40 hover:text-white transition-all"
                title={t.authForm.uploadKey}
              >
                <Upload size={13} />
                <span>{t.authForm.uploadKey}</span>
              </button>
              <input
                type="file"
                ref={fileInputRef}
                onChange={handleFileUpload}
                className="hidden"
                accept=".pem,.key,.txt"
              />
            </div>
            <textarea
              id="manual-key"
              value={credentials.privateKey}
              onChange={(e) =>
                onCredentialsChange({ ...credentials, privateKey: e.target.value })
              }
              className="w-full px-3 py-2 text-xs rounded-md border border-zinc-700/50 outline-none transition-all bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] min-h-[100px] resize-y font-mono"
              placeholder={t.manualAuth.privateKeyPlaceholder}
            />
          </div>

          <div className="mt-4">
            <label className="flex items-center gap-2 cursor-pointer group">
              <input
                type="checkbox"
                checked={credentials.rememberMe || false}
                onChange={(e) =>
                  onCredentialsChange({ ...credentials, rememberMe: e.target.checked })
                }
                className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)]"
              />
              <span className="text-sm text-zinc-400 group-hover:text-zinc-300 transition-colors">
                {t.manualAuth.rememberCredentials}
              </span>
            </label>
          </div>
        </div>

        <div className="px-6 py-4 border-t border-[var(--glass-border)] bg-[var(--bg-secondary)]">
          <div className="flex gap-4 w-full justify-center">
            <button
              type="button"
              onClick={onCancel}
              className="flex-1 max-w-[140px] flex justify-center px-4 py-2 rounded text-[13px] font-medium text-[var(--text-secondary)] bg-transparent border border-zinc-700/50 hover:bg-[var(--bg-tertiary)] hover:text-white transition-all"
            >
              {t.common.cancel}
            </button>
            <button
              type="button"
              onClick={onConnect}
              className="flex-1 max-w-[140px] flex justify-center px-4 py-2 rounded text-[13px] font-medium text-white bg-blue-500 hover:brightness-110 shadow-[0_4px_12px_rgba(59,130,246,0.3)] hover:-translate-y-0.5 transition-all"
            >
              {t.common.connect}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
