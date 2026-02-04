import React from 'react';
import { useConfig } from '../hooks/useConfig';
import { EmojiText } from './EmojiText';

interface StatusBarProps {
  leftText: string;
  rightText: string;
  theme?: 'light' | 'dark' | 'orange' | 'green' | 'system';
  connected: boolean;
}

export const StatusBar: React.FC<StatusBarProps> = ({ leftText, rightText, theme, connected }) => {
  const { config } = useConfig();
  // Determine colors based on theme
  const isDark = theme === 'dark' || theme === 'orange' || theme === 'green' || (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
  
  const style = {
    backgroundColor: theme === 'orange' ? '#292524' : (theme === 'green' ? '#121a16' : (isDark ? '#1a1a1a' : '#f0f0f0')),
    color: theme === 'orange' ? '#fafaf9' : (theme === 'green' ? '#f0fdf4' : (isDark ? '#d4d4d4' : '#333333')),
    borderColor: theme === 'orange' ? '#44403c' : (theme === 'green' ? '#1c2621' : (isDark ? '#333' : '#ddd')),
  } as React.CSSProperties;

  return (
    <div className="flex flex-row h-6 leading-6 w-full text-xs font-mono overflow-hidden border-b border-[rgba(128,128,128,0.2)] select-none px-2 box-border" style={style}>
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
      <div 
        className="flex-1 whitespace-nowrap overflow-hidden text-ellipsis text-left pr-2.5 cursor-pointer" 
        title={leftText}
        onContextMenu={(e) => {
          e.preventDefault();
          if (leftText) {
            navigator.clipboard.writeText(leftText).catch(() => {
              // Failed to copy
            });
          }
        }}
      >
        <EmojiText text={leftText} />
      </div>
      <div className="flex-none max-w-[200px] text-right whitespace-nowrap overflow-hidden text-ellipsis flex items-center justify-end" title={rightText}>
        <span className={`w-2 h-2 rounded-full mr-2 inline-block shrink-0 transition-all duration-300 ${connected ? 'bg-[#4cd964] shadow-[0_0_6px_rgba(76,217,100,0.6)]' : 'bg-[#ff3b30] shadow-[0_0_6px_rgba(255,59,48,0.6)]'}`} />
        <EmojiText text={rightText} />
      </div>
    </div>
  );
};
