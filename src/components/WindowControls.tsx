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
      console.error('Failed to minimize window:', err);
    }
  };

  const handleMaximize = async () => {
    try {
      await getCurrentWebviewWindow().toggleMaximize();
    } catch (err) {
      console.error('Failed to maximize window:', err);
    }
  };

  const handleClose = async () => {
    try {
      await getCurrentWebviewWindow().close();
    } catch (err) {
      console.error('Failed to close window:', err);
    }
  };

  return (
    <div className="window-controls">
      <button
        type="button"
        onClick={handleMinimize}
        aria-label={t.windowControls.minimize}
        className="window-control-btn minimize-btn"
        title={t.windowControls.minimize}
      >
        <Minus size={14} />
      </button>
      <button
        type="button"
        onClick={handleMaximize}
        aria-label={t.windowControls.maximize}
        className="window-control-btn maximize-btn"
        title={t.windowControls.maximize}
      >
        <Square size={12} />
      </button>
      <button
        type="button"
        onClick={handleClose}
        aria-label={t.windowControls.close}
        className="window-control-btn close-btn"
        title={t.windowControls.close}
      >
        <X size={16} />
      </button>
    </div>
  );
};
