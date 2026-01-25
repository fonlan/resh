import React from 'react';
import { useTerminal } from '../hooks/useTerminal';

interface TerminalTabProps {
  tabId: string;
  isActive: boolean;
}

export const TerminalTab: React.FC<TerminalTabProps> = ({ tabId, isActive }) => {
  const containerId = `terminal-${tabId}`;
  const { write: _write } = useTerminal(containerId);

  return (
    <div
      id={containerId}
      style={{
        display: isActive ? 'block' : 'none',
        width: '100%',
        height: '100%',
      }}
    />
  );
};
