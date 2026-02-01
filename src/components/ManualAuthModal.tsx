import React, { useRef } from 'react';
import type { ManualAuthCredentials } from '../types/config';
import { useTranslation } from '../i18n';
import { Upload } from 'lucide-react';
import './FormModal.css';

interface ManualAuthModalProps {
  serverName: string;
  credentials: ManualAuthCredentials;
  onCredentialsChange: (creds: ManualAuthCredentials) => void;
  onConnect: () => void;
  onCancel: () => void;
}

export const ManualAuthModal: React.FC<ManualAuthModalProps> = ({
  serverName,
  credentials,
  onCredentialsChange,
  onConnect,
  onCancel,
}) => {
  const { t } = useTranslation();
  const fileInputRef = useRef<HTMLInputElement>(null);

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

  return (
    <div className="form-modal-overlay">
      <div className="form-modal-container" style={{ maxWidth: '480px' }}>
        <div className="form-modal-header">
          <h2>{t.manualAuth.title}</h2>
        </div>

        <div className="form-modal-content with-padding">
          <p className="text-sm" style={{ color: 'var(--text-secondary)', marginBottom: '20px' }}>
            {t.manualAuth.enterCredentials.replace('{server}', serverName)}
          </p>

          <div className="form-group">
            <label htmlFor="manual-username" className="form-label">
              {t.manualAuth.usernameLabel}
            </label>
            <input
              id="manual-username"
              type="text"
              value={credentials.username}
              onChange={(e) =>
                onCredentialsChange({ ...credentials, username: e.target.value })
              }
              className="form-input"
            />
          </div>

          <div className="form-group">
            <label htmlFor="manual-password" className="form-label">
              {t.manualAuth.passwordLabel}
            </label>
            <input
              id="manual-password"
              type="password"
              value={credentials.password}
              onChange={(e) =>
                onCredentialsChange({ ...credentials, password: e.target.value })
              }
              className="form-input"
              placeholder={t.manualAuth.passwordPlaceholder}
            />
          </div>

          <div className="text-center text-xs" style={{ color: 'var(--text-muted)', margin: '16px 0' }}>
            {t.manualAuth.orDivider}
          </div>

          <div className="form-group">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '8px' }}>
              <label htmlFor="manual-key" className="form-label" style={{ marginBottom: 0 }}>
                {t.manualAuth.privateKeyLabel}
              </label>
              <button
                type="button"
                onClick={() => fileInputRef.current?.click()}
                className="btn btn-secondary py-1 px-2 text-xs flex items-center gap-1.5"
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
              className="form-input"
              placeholder={t.manualAuth.privateKeyPlaceholder}
              style={{ minHeight: '100px', resize: 'vertical' }}
            />
          </div>
        </div>

        <div className="form-modal-footer">
          <div style={{ display: 'flex', gap: '16px', width: '100%', justifyContent: 'center' }}>
            <button
              type="button"
              onClick={onCancel}
              className="form-modal-cancel-btn"
              style={{ flex: 1, maxWidth: '140px', display: 'flex', justifyContent: 'center' }}
            >
              {t.common.cancel}
            </button>
            <button
              type="button"
              onClick={onConnect}
              className="form-modal-submit-btn"
              style={{ flex: 1, maxWidth: '140px', display: 'flex', justifyContent: 'center' }}
            >
              {t.common.connect}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
