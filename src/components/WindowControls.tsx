import React from 'react';
import { Minus, Square, X } from 'lucide-react';
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
        type="button"
        onClick={handleMinimize}
        aria-label="Minimize window"
        className="window-control-btn minimize-btn"
        title="Minimize"
      >
        <Minus size={14} />
      </button>
      <button
        type="button"
        onClick={handleMaximize}
        aria-label="Maximize window"
        className="window-control-btn maximize-btn"
        title="Maximize"
      >
        <Square size={12} />
      </button>
      <button
        type="button"
        onClick={handleClose}
        aria-label="Close window"
        className="window-control-btn close-btn"
        title="Close"
      >
        <X size={16} />
      </button>
    </div>
  );
};
