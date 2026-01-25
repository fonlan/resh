import React from 'react';
import { appWindow } from '@tauri-apps/api/window';

export const WindowControls: React.FC = () => {
  const handleMinimize = () => {
    appWindow.minimize();
  };

  const handleMaximize = () => {
    appWindow.toggleMaximize();
  };

  const handleClose = () => {
    appWindow.close();
  };

  return (
    <div className="window-controls">
      <button
        onClick={handleMinimize}
        className="window-control-btn minimize-btn"
        title="Minimize"
      >
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path d="M0 6h12" stroke="currentColor" strokeWidth="1" />
        </svg>
      </button>
      <button
        onClick={handleMaximize}
        className="window-control-btn maximize-btn"
        title="Maximize"
      >
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <rect x="0.5" y="0.5" width="11" height="11" stroke="currentColor" strokeWidth="1" fill="none" />
        </svg>
      </button>
      <button
        onClick={handleClose}
        className="window-control-btn close-btn"
        title="Close"
      >
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path d="M1 1l10 10M11 1L1 11" stroke="currentColor" strokeWidth="1" />
        </svg>
      </button>
    </div>
  );
};
