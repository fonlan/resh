import React from 'react';
import { GeneralSettings } from '../../types';
import { useTranslation } from '../../i18n';
import { CustomSelect } from '../CustomSelect';

export interface GeneralTabProps {
  general: GeneralSettings;
  onGeneralUpdate: (general: GeneralSettings) => void;
}

export const GeneralTab: React.FC<GeneralTabProps> = ({ general, onGeneralUpdate }) => {
  const { t } = useTranslation();

  const handleThemeChange = (theme: 'light' | 'dark' | 'orange' | 'green' | 'system') => {
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
    <div className="w-full max-w-full space-y-6">
      {/* Appearance Section */}
      <div>
        <h3 className="text-base font-semibold  mb-4">{t.appearance}</h3>
        <div className="space-y-4">
          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="theme-select" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.theme}</label>
            <CustomSelect
              id="theme-select"
              value={general.theme}
              onChange={(val) => handleThemeChange(val as 'light' | 'dark' | 'orange' | 'green' | 'system')}
              options={[
                { value: 'system', label: t.system },
                { value: 'light', label: t.light },
                { value: 'dark', label: t.dark },
                { value: 'orange', label: t.orange },
                { value: 'green', label: t.green }
              ]}
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="language-select" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.language}</label>
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

          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="recording-mode-select" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.recordingMode}</label>
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

          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="max-recent-servers" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.maxRecentServers}</label>
            <input
              id="max-recent-servers"
              type="number"
              value={general.maxRecentServers}
              onChange={(e) => handleConfirmationChange('maxRecentServers', parseInt(e.target.value) || 0)}
              min="0"
              max="20"
              className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
            />
          </div>
        </div>
      </div>

      {/* Terminal Settings Section */}
      <div>
        <h3 className="text-base font-semibold  mb-4">{t.terminal}</h3>
        <div className="space-y-4">
          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="font-family" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.fontFamily}</label>
            <input
              id="font-family"
              type="text"
              value={general.terminal.fontFamily}
              onChange={(e) => handleTerminalUpdate('fontFamily', e.target.value)}
              className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
              placeholder={t.fontFamilyPlaceholder}
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="font-size" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.fontSize}</label>
            <input
              id="font-size"
              type="number"
              value={general.terminal.fontSize}
              onChange={(e) => handleTerminalUpdate('fontSize', parseInt(e.target.value) || 14)}
              min="8"
              max="32"
              className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
            />
          </div>

          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="cursor-style" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.cursorStyle}</label>
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

          <div className="flex flex-col gap-1.5 mb-4">
            <label htmlFor="scrollback-limit" className="block text-sm font-medium text-zinc-400 mb-1.5 ">{t.scrollback}</label>
            <input
              id="scrollback-limit"
              type="number"
              value={general.terminal.scrollback}
              onChange={(e) => handleTerminalUpdate('scrollback', parseInt(e.target.value) || 1000)}
              min="100"
              max="50000"
              className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
            />
          </div>
        </div>
      </div>

      {/* Confirmations Section */}
      <div>
        <h3 className="text-base font-semibold  mb-4">{t.confirmations}</h3>
        <div className="space-y-3">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.confirmCloseTab}
              onChange={(e) => handleConfirmationChange('confirmCloseTab', e.target.checked)}
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <span className="block text-sm font-medium text-zinc-400 mb-0 ">{t.confirmCloseTab}</span>
          </label>

          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.confirmExitApp}
              onChange={(e) => handleConfirmationChange('confirmExitApp', e.target.checked)}
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <span className="block text-sm font-medium text-zinc-400 mb-0 ">{t.confirmExitApp}</span>
          </label>

          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={general.debugEnabled}
              onChange={(e) => handleConfirmationChange('debugEnabled', e.target.checked)}
              className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border-[1.5px] border-zinc-700/50 rounded bg-[var(--bg-primary)] cursor-pointer relative transition-all flex-shrink-0 inline-flex items-center justify-center vertical-middle checked:bg-blue-500 checked:border-blue-500 checked:shadow-[0_0_20px_rgba(59,130,246,0.2)] hover:border-blue-500 focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <span className="block text-sm font-medium text-zinc-400 mb-0 ">{t.debugEnabled}</span>
          </label>
        </div>
      </div>
    </div>
  );
};
