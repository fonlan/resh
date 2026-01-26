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
    <div className="space-y-6" style={{ color: '#e0e0e0' }}>
      {/* Appearance Section */}
      <div>
        <h3 className="text-lg font-semibold mb-4" style={{ color: '#ffffff' }}>
          {t.appearance}
        </h3>
        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium mb-2">{t.theme}</label>
            <select
              value={general.theme}
              onChange={(e) => handleThemeChange(e.target.value as 'light' | 'dark' | 'system')}
              className="w-full px-3 py-2 rounded border"
              style={{
                backgroundColor: '#2a2a2a',
                color: '#e0e0e0',
                borderColor: '#3a3a3a'
              }}
            >
              <option value="system">{t.system}</option>
              <option value="light">{t.light}</option>
              <option value="dark">{t.dark}</option>
            </select>
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">{t.language}</label>
            <select
              value={general.language}
              onChange={(e) => handleLanguageChange(e.target.value as 'en' | 'zh-CN')}
              className="w-full px-3 py-2 rounded border"
              style={{
                backgroundColor: '#2a2a2a',
                color: '#e0e0e0',
                borderColor: '#3a3a3a'
              }}
            >
              <option value="en">English</option>
              <option value="zh-CN">简体中文</option>
            </select>
          </div>
        </div>
      </div>

      {/* Terminal Settings Section */}
      <div>
        <h3 className="text-lg font-semibold mb-4" style={{ color: '#ffffff' }}>
          {t.terminal}
        </h3>
        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium mb-2">{t.fontFamily}</label>
            <input
              type="text"
              value={general.terminal.fontFamily}
              onChange={(e) => handleTerminalUpdate('fontFamily', e.target.value)}
              className="w-full px-3 py-2 rounded border"
              style={{
                backgroundColor: '#2a2a2a',
                color: '#e0e0e0',
                borderColor: '#3a3a3a'
              }}
              placeholder="e.g., 'Courier New', monospace"
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">{t.fontSize}</label>
            <input
              type="number"
              value={general.terminal.fontSize}
              onChange={(e) => handleTerminalUpdate('fontSize', parseInt(e.target.value) || 14)}
              min="8"
              max="32"
              className="w-full px-3 py-2 rounded border"
              style={{
                backgroundColor: '#2a2a2a',
                color: '#e0e0e0',
                borderColor: '#3a3a3a'
              }}
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">{t.cursorStyle}</label>
            <select
              value={general.terminal.cursorStyle}
              onChange={(e) => handleTerminalUpdate('cursorStyle', e.target.value)}
              className="w-full px-3 py-2 rounded border"
              style={{
                backgroundColor: '#2a2a2a',
                color: '#e0e0e0',
                borderColor: '#3a3a3a'
              }}
            >
              <option value="block">Block</option>
              <option value="underline">Underline</option>
              <option value="bar">Bar</option>
            </select>
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">{t.scrollback}</label>
            <input
              type="number"
              value={general.terminal.scrollback}
              onChange={(e) => handleTerminalUpdate('scrollback', parseInt(e.target.value) || 1000)}
              min="100"
              max="50000"
              className="w-full px-3 py-2 rounded border"
              style={{
                backgroundColor: '#2a2a2a',
                color: '#e0e0e0',
                borderColor: '#3a3a3a'
              }}
            />
          </div>
        </div>
      </div>

      {/* WebDAV Settings Section */}
      <div>
        <h3 className="text-lg font-semibold mb-4" style={{ color: '#ffffff' }}>
          {t.webdav}
        </h3>
        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium mb-2">{t.webdavUrl}</label>
            <input
              type="text"
              value={general.webdav.url}
              onChange={(e) => handleWebDAVUpdate('url', e.target.value)}
              className="w-full px-3 py-2 rounded border"
              style={{
                backgroundColor: '#2a2a2a',
                color: '#e0e0e0',
                borderColor: '#3a3a3a'
              }}
              placeholder="https://example.com/webdav"
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">{t.username}</label>
            <input
              type="text"
              value={general.webdav.username}
              onChange={(e) => handleWebDAVUpdate('username', e.target.value)}
              className="w-full px-3 py-2 rounded border"
              style={{
                backgroundColor: '#2a2a2a',
                color: '#e0e0e0',
                borderColor: '#3a3a3a'
              }}
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">{t.password}</label>
            <input
              type="password"
              value={general.webdav.password}
              onChange={(e) => handleWebDAVUpdate('password', e.target.value)}
              className="w-full px-3 py-2 rounded border"
              style={{
                backgroundColor: '#2a2a2a',
                color: '#e0e0e0',
                borderColor: '#3a3a3a'
              }}
            />
          </div>
        </div>
      </div>

      {/* Confirmations Section */}
      <div>
        <h3 className="text-lg font-semibold mb-4" style={{ color: '#ffffff' }}>
          {t.confirmations}
        </h3>
        <div className="space-y-3">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.confirmCloseTab}
              onChange={(e) => handleConfirmationChange('confirmCloseTab', e.target.checked)}
              className="w-4 h-4"
              style={{ accentColor: '#0066cc' }}
            />
            <span className="text-sm">{t.confirmCloseTab}</span>
          </label>

          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.confirmExitApp}
              onChange={(e) => handleConfirmationChange('confirmExitApp', e.target.checked)}
              className="w-4 h-4"
              style={{ accentColor: '#0066cc' }}
            />
            <span className="text-sm">{t.confirmExitApp}</span>
          </label>
        </div>
      </div>
    </div>
  );
};
