import React from 'react';
import './StatusBar.css';
import { useConfig } from '../hooks/useConfig';

interface StatusBarProps {
  leftText: string;
  rightText: string;
  theme?: 'light' | 'dark' | 'system';
  connected: boolean;
}

export const StatusBar: React.FC<StatusBarProps> = ({ leftText, rightText, theme, connected }) => {
  const { config } = useConfig();
  // Determine colors based on theme
  const isDark = theme === 'dark' || (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
  
  const style = {
    backgroundColor: isDark ? '#1a1a1a' : '#f0f0f0',
    color: isDark ? '#d4d4d4' : '#333333',
    borderColor: isDark ? '#333' : '#ddd',
    fontFamily: config?.general.terminal.fontFamily || 'Consolas, monospace'
  };

  return (
    <div className="status-bar" style={style}>
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
      <div 
        className="status-bar-left" 
        title={leftText}
        onContextMenu={(e) => {
          e.preventDefault();
          if (leftText) {
            navigator.clipboard.writeText(leftText).catch(err => console.error('Failed to copy status text:', err));
          }
        }}
        style={{ cursor: 'pointer' }}
      >
        {leftText}
      </div>
      <div className="status-bar-right" title={rightText}>
        <span className={`status-dot ${connected ? 'connected' : 'disconnected'}`} />
        {rightText}
      </div>
    </div>
  );
};
