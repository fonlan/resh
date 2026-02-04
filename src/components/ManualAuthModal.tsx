import React, { useRef } from 'react';
import type { ManualAuthCredentials } from '../types/config';
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
          <p className="text-sm mb-5 text-[var(--text-secondary)]">
            {isRetry ? t.manualAuth.retryDescription.replace('{server}', serverName) : t.manualAuth.enterCredentials.replace('{server}', serverName)}
          </p>

          <div className="mb-4">
            <label htmlFor="manual-username" className="block text-sm font-medium text-[var(--text-secondary)] mb-1.5">
              {t.manualAuth.usernameLabel}
            </label>
            <input
              id="manual-username"
              type="text"
              value={credentials.username}
              onChange={(e) =>
                onCredentialsChange({ ...credentials, username: e.target.value })
              }
              className="w-full px-3 py-2 bg-[var(--bg-tertiary)] border border-[var(--glass-border)] rounded text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent-primary)] transition-colors placeholder-[var(--text-muted)]"
            />
          </div>

          <div className="mb-4">
            <label htmlFor="manual-password" className="block text-sm font-medium text-[var(--text-secondary)] mb-1.5">
              {t.manualAuth.passwordLabel}
            </label>
            <input
              id="manual-password"
              type="password"
              value={credentials.password}
              onChange={(e) =>
                onCredentialsChange({ ...credentials, password: e.target.value })
              }
              className="w-full px-3 py-2 bg-[var(--bg-tertiary)] border border-[var(--glass-border)] rounded text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent-primary)] transition-colors placeholder-[var(--text-muted)]"
              placeholder={t.manualAuth.passwordPlaceholder}
            />
          </div>

          <div className="text-center text-xs text-[var(--text-muted)] my-4">
            {t.manualAuth.orDivider}
          </div>

          <div className="mb-4">
            <div className="flex justify-between items-center mb-2">
              <label htmlFor="manual-key" className="block text-sm font-medium text-[var(--text-secondary)] mb-0">
                {t.manualAuth.privateKeyLabel}
              </label>
              <button
                type="button"
                onClick={() => fileInputRef.current?.click()}
                className="py-1 px-2 text-xs flex items-center gap-1.5 rounded text-[var(--text-primary)] bg-[var(--bg-tertiary)] hover:bg-[var(--bg-primary)] border border-[var(--glass-border)] transition-colors"
                title={t.authForm.uploadKey}
              >
                <Upload size={14} />
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
              className="w-full px-3 py-2 bg-[var(--bg-tertiary)] border border-[var(--glass-border)] rounded text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent-primary)] transition-colors placeholder-[var(--text-muted)] min-h-[100px] resize-y"
              placeholder={t.manualAuth.privateKeyPlaceholder}
            />
          </div>

          <div className="mt-4">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={credentials.rememberMe || false}
                onChange={(e) =>
                  onCredentialsChange({ ...credentials, rememberMe: e.target.checked })
                }
                className="w-4 h-4 rounded border-[var(--glass-border)] bg-[var(--bg-tertiary)] text-[var(--accent-primary)] focus:ring-[var(--accent-primary)] cursor-pointer"
              />
              <span className="text-sm text-[var(--text-secondary)]">
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
              className="flex-1 max-w-[140px] flex justify-center px-4 py-2 rounded text-sm font-medium text-[var(--text-primary)] bg-[var(--bg-tertiary)] hover:bg-[var(--bg-primary)] border border-[var(--glass-border)] transition-colors"
            >
              {t.common.cancel}
            </button>
            <button
              type="button"
              onClick={onConnect}
              className="flex-1 max-w-[140px] flex justify-center px-4 py-2 rounded text-sm font-medium text-white bg-[var(--accent-primary)] hover:bg-[var(--accent-hover)] transition-colors"
            >
              {t.common.connect}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
