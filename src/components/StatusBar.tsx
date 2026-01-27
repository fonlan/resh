import React from 'react';
import './StatusBar.css';

interface StatusBarProps {
  leftText: string;
  rightText: string;
  theme?: 'light' | 'dark' | 'system';
}

export const StatusBar: React.FC<StatusBarProps> = ({ leftText, rightText, theme }) => {
  // Determine colors based on theme
  const isDark = theme === 'dark' || (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
  
  const style = {
    backgroundColor: isDark ? '#1a1a1a' : '#f0f0f0',
    color: isDark ? '#d4d4d4' : '#333333',
    borderColor: isDark ? '#333' : '#ddd'
  };

  return (
    <div className="status-bar" style={style}>
      <div className="status-bar-left" title={leftText}>
        {leftText}
      </div>
      <div className="status-bar-right" title={rightText}>
        {rightText}
      </div>
    </div>
  );
};
