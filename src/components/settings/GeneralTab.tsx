import React from 'react';
import { GeneralSettings } from '../../types/config';
import { useTranslation } from '../../i18n';

export interface GeneralTabProps {
  general: GeneralSettings;
  onGeneralUpdate: (general: GeneralSettings) => void;
}

export const GeneralTab: React.FC<GeneralTabProps> = ({ general, onGeneralUpdate }) => {
  const { t } = useTranslation();

  const handleThemeChange = (theme: 'light' | 'dark' | 'system') => {
    onGeneralUpdate({ ...general, theme });
  };

  const handleLanguageChange = (language: 'en' | 'zh-CN') => {
    onGeneralUpdate({ ...general, language });
  };

  const handleTerminalUpdate = (field: keyof typeof general.terminal, value: string | number) => {
    onGeneralUpdate({
      ...general,
      terminal: { ...general.terminal, [field]: value }
    });
  };

  const handleWebDAVUpdate = (field: keyof typeof general.webdav, value: string) => {
    onGeneralUpdate({
      ...general,
      webdav: { ...general.webdav, [field]: value }
    });
  };

  const handleConfirmationChange = (field: 'confirmCloseTab' | 'confirmExitApp', value: boolean) => {
    onGeneralUpdate({ ...general, [field]: value });
  };

  return (
    <div className="tab-container space-y-6">
      {/* Appearance Section */}
      <div className="section">
        <h3 className="section-title mb-4">{t.appearance}</h3>
        <div className="space-y-4">
          <div className="form-group">
            <label htmlFor="theme-select" className="form-label">{t.theme}</label>
            <select
              id="theme-select"
              value={general.theme}
              onChange={(e) => handleThemeChange(e.target.value as 'light' | 'dark' | 'system')}
              className="form-input form-select"
            >
              <option value="system">{t.system}</option>
              <option value="light">{t.light}</option>
              <option value="dark">{t.dark}</option>
            </select>
          </div>

          <div className="form-group">
            <label htmlFor="language-select" className="form-label">{t.language}</label>
            <select
              id="language-select"
              value={general.language}
              onChange={(e) => handleLanguageChange(e.target.value as 'en' | 'zh-CN')}
              className="form-input form-select"
            >
              <option value="en">English</option>
              <option value="zh-CN">简体中文</option>
            </select>
          </div>
        </div>
      </div>

      {/* Terminal Settings Section */}
      <div className="section">
        <h3 className="section-title mb-4">{t.terminal}</h3>
        <div className="space-y-4">
          <div className="form-group">
            <label htmlFor="font-family" className="form-label">{t.fontFamily}</label>
            <input
              id="font-family"
              type="text"
              value={general.terminal.fontFamily}
              onChange={(e) => handleTerminalUpdate('fontFamily', e.target.value)}
              className="form-input"
              placeholder="e.g., 'Courier New', monospace"
            />
          </div>

          <div className="form-group">
            <label htmlFor="font-size" className="form-label">{t.fontSize}</label>
            <input
              id="font-size"
              type="number"
              value={general.terminal.fontSize}
              onChange={(e) => handleTerminalUpdate('fontSize', parseInt(e.target.value) || 14)}
              min="8"
              max="32"
              className="form-input"
            />
          </div>

          <div className="form-group">
            <label htmlFor="cursor-style" className="form-label">{t.cursorStyle}</label>
            <select
              id="cursor-style"
              value={general.terminal.cursorStyle}
              onChange={(e) => handleTerminalUpdate('cursorStyle', e.target.value)}
              className="form-input form-select"
            >
              <option value="block">{t.cursorStyles.block}</option>
              <option value="underline">{t.cursorStyles.underline}</option>
              <option value="bar">{t.cursorStyles.bar}</option>
            </select>
          </div>

          <div className="form-group">
            <label htmlFor="scrollback-limit" className="form-label">{t.scrollback}</label>
            <input
              id="scrollback-limit"
              type="number"
              value={general.terminal.scrollback}
              onChange={(e) => handleTerminalUpdate('scrollback', parseInt(e.target.value) || 1000)}
              min="100"
              max="50000"
              className="form-input"
            />
          </div>
        </div>
      </div>

      {/* WebDAV Settings Section */}
      <div className="section">
        <h3 className="section-title mb-4">{t.webdav}</h3>
        <div className="space-y-4">
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

      {/* Confirmations Section */}
      <div className="section">
        <h3 className="section-title mb-4">{t.confirmations}</h3>
        <div className="space-y-3">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.confirmCloseTab}
              onChange={(e) => handleConfirmationChange('confirmCloseTab', e.target.checked)}
              className="checkbox"
            />
            <span className="form-label mb-0">{t.confirmCloseTab}</span>
          </label>

          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.confirmExitApp}
              onChange={(e) => handleConfirmationChange('confirmExitApp', e.target.checked)}
              className="checkbox"
            />
            <span className="form-label mb-0">{t.confirmExitApp}</span>
          </label>
        </div>
      </div>
    </div>
  );
};
