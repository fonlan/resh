import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { listen } from '@tauri-apps/api/event';
import { useTerminal } from '../hooks/useTerminal';

type UnlistenFn = () => void;

interface TerminalTabProps {
  tabId: string;
  serverId: string;
  isActive: boolean;
  onClose: () => void;
}

export const TerminalTab: React.FC<TerminalTabProps> = ({ tabId, serverId, isActive }) => {
  const containerId = `terminal-${tabId}`;
  const { terminal, write } = useTerminal(containerId);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const connectedRef = useRef(false);

  useEffect(() => {
    if (!serverId || connectedRef.current) return;
    
    let outputUnlistener: UnlistenFn | null = null;
    let closedUnlistener: UnlistenFn | null = null;

    const connect = async () => {
      try {
        connectedRef.current = true;
        write(`Connecting to server ${serverId}...\r\n`);
        
        const params = {
          host: 'localhost',
          port: 22,
          username: 'dummy',
          password: 'dummy'
        };

        const response = await invoke<{ session_id: string }>('connect_to_server', { params });
        const sid = response.session_id;
        setSessionId(sid);
        write(`Connected! Session ID: ${sid}\r\n`);

        outputUnlistener = await listen<string>(`terminal-output:${sid}`, (event) => {
           write(event.payload);
        });

        closedUnlistener = await listen(`connection-closed:${sid}`, () => {
           write('\r\nConnection closed.\r\n');
        });

      } catch (err) {
        write(`\r\nError: ${err}\r\n`);
        connectedRef.current = false;
      }
    };

    connect();

    return () => {
      if (outputUnlistener) outputUnlistener();
      if (closedUnlistener) closedUnlistener();
      connectedRef.current = false;
    };
  }, [serverId]);

  useEffect(() => {
    if (!terminal || !sessionId) return;

    const disposable = terminal.onData((data) => {
      invoke('send_command', {
        params: {
          session_id: sessionId,
          command: data
        }
      });
    });

    return () => disposable.dispose();
  }, [terminal, sessionId]);

  useEffect(() => {
    if (!terminal || !sessionId) return;

    const handleResize = () => {
      const cols = terminal.cols || 80;
      const rows = terminal.rows || 24;

      invoke('resize_terminal', {
        sessionId,
        cols,
        rows,
      }).catch(err => console.error('Resize failed:', err));
    };

    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, [terminal, sessionId]);

  return (
    <div
      id={containerId}
      style={{
        display: isActive ? 'block' : 'none',
        width: '100%',
        height: '100%',
        minHeight: '400px',
        backgroundColor: '#000'
      }}
    />
  );
};
