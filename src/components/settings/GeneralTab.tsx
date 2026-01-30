import React from 'react';
import { GeneralSettings } from '../../types/config';
import { useTranslation } from '../../i18n';
import { CustomSelect } from '../CustomSelect';

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

  const handleRecordingModeChange = (recordingMode: 'raw' | 'text') => {
    onGeneralUpdate({ ...general, recordingMode });
  };

  const handleTerminalUpdate = (field: keyof typeof general.terminal, value: string | number) => {
    onGeneralUpdate({
      ...general,
      terminal: { ...general.terminal, [field]: value }
    });
  };

  const handleConfirmationChange = (field: 'confirmCloseTab' | 'confirmExitApp' | 'debugEnabled' | 'maxRecentServers', value: boolean | number) => {
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
            <CustomSelect
              id="theme-select"
              value={general.theme}
              onChange={(val) => handleThemeChange(val as 'light' | 'dark' | 'system')}
              options={[
                { value: 'system', label: t.system },
                { value: 'light', label: t.light },
                { value: 'dark', label: t.dark }
              ]}
            />
          </div>

          <div className="form-group">
            <label htmlFor="language-select" className="form-label">{t.language}</label>
            <CustomSelect
              id="language-select"
              value={general.language}
              onChange={(val) => handleLanguageChange(val as 'en' | 'zh-CN')}
              options={[
                { value: 'en', label: 'English' },
                { value: 'zh-CN', label: '简体中文' }
              ]}
            />
          </div>

          <div className="form-group">
            <label htmlFor="recording-mode-select" className="form-label">{t.recordingMode}</label>
            <CustomSelect
              id="recording-mode-select"
              value={general.recordingMode || 'raw'}
              onChange={(val) => handleRecordingModeChange(val as 'raw' | 'text')}
              options={[
                { value: 'raw', label: t.recordingModes.raw },
                { value: 'text', label: t.recordingModes.text }
              ]}
            />
          </div>

          <div className="form-group">
            <label htmlFor="max-recent-servers" className="form-label">{t.maxRecentServers}</label>
            <input
              id="max-recent-servers"
              type="number"
              value={general.maxRecentServers}
              onChange={(e) => handleConfirmationChange('maxRecentServers', parseInt(e.target.value) || 0)}
              min="0"
              max="20"
              className="form-input"
            />
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
            <CustomSelect
              id="cursor-style"
              value={general.terminal.cursorStyle}
              onChange={(val) => handleTerminalUpdate('cursorStyle', val)}
              options={[
                { value: 'block', label: t.cursorStyles.block },
                { value: 'underline', label: t.cursorStyles.underline },
                { value: 'bar', label: t.cursorStyles.bar }
              ]}
            />
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

          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.debugEnabled}
              onChange={(e) => handleConfirmationChange('debugEnabled', e.target.checked)}
              className="checkbox"
            />
            <span className="form-label mb-0">{t.debugEnabled}</span>
          </label>
        </div>
      </div>
    </div>
  );
};
