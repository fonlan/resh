import React from 'react';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

export const WindowControls: React.FC = () => {
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
        onClick={handleMinimize}
        aria-label="Minimize window"
        className="window-control-btn minimize-btn focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
        title="Minimize"
      >
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path d="M0 6h12" stroke="currentColor" strokeWidth="1" />
        </svg>
      </button>
      <button
        onClick={handleMaximize}
        aria-label="Maximize window"
        className="window-control-btn maximize-btn focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
        title="Maximize"
      >
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <rect x="0.5" y="0.5" width="11" height="11" stroke="currentColor" strokeWidth="1" fill="none" />
        </svg>
      </button>
      <button
        onClick={handleClose}
        aria-label="Close window"
        className="window-control-btn close-btn focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
        title="Close"
      >
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path d="M1 1l10 10M11 1L1 11" stroke="currentColor" strokeWidth="1" />
        </svg>
      </button>
    </div>
  );
};
