import React from 'react';
import { Minus, Square, X } from 'lucide-react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useTranslation } from '../i18n';

export const WindowControls: React.FC = () => {
  const { t } = useTranslation();
  const handleMinimize = async () => {
    try {
      await getCurrentWebviewWindow().minimize();
    } catch (err) {
      // Failed to minimize window
    }
  };

  const handleMaximize = async () => {
    try {
      await getCurrentWebviewWindow().toggleMaximize();
    } catch (err) {
      // Failed to maximize window
    }
  };

  const handleClose = async () => {
    try {
      await getCurrentWebviewWindow().close();
    } catch (err) {
      // Failed to close window
    }
  };

  return (
    <div className="flex items-center">
      <button
        type="button"
        onClick={handleMinimize}
        aria-label={t.windowControls.minimize}
        className="w-[46px] h-10 flex items-center justify-center bg-transparent border-none text-[var(--text-secondary)] cursor-pointer transition-all hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
        title={t.windowControls.minimize}
      >
        <Minus size={14} />
      </button>
      <button
        type="button"
        onClick={handleMaximize}
        aria-label={t.windowControls.maximize}
        className="w-[46px] h-10 flex items-center justify-center bg-transparent border-none text-[var(--text-secondary)] cursor-pointer transition-all hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
        title={t.windowControls.maximize}
      >
        <Square size={12} />
      </button>
      <button
        type="button"
        onClick={handleClose}
        aria-label={t.windowControls.close}
        className="w-[46px] h-10 flex items-center justify-center bg-transparent border-none text-[var(--text-secondary)] cursor-pointer transition-all hover:bg-[var(--color-danger)] hover:text-white"
        title={t.windowControls.close}
      >
        <X size={16} />
      </button>
    </div>
  );
};
