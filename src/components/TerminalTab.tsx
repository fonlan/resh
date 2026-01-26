import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useTerminal } from '../hooks/useTerminal';
import { Server, Authentication, Proxy, TerminalSettings } from '../types/config';

type UnlistenFn = () => void;

interface TerminalTabProps {
  tabId: string;
  serverId: string;
  isActive: boolean;
  onClose: () => void;
  server: Server;
  authentications: Authentication[];
  proxies: Proxy[];
  terminalSettings?: TerminalSettings;
  theme?: 'light' | 'dark' | 'system';
}

export const TerminalTab: React.FC<TerminalTabProps> = ({
  tabId,
  serverId,
  isActive,
  server,
  authentications,
  proxies: _proxies,
  terminalSettings,
  theme,
}) => {
  const containerId = `terminal-${tabId}`;
  const { terminal, write } = useTerminal(containerId, terminalSettings, theme);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const connectedRef = useRef(false);
  const authenticationsRef = useRef(authentications);

  // Update ref when authentications changes (but don't trigger reconnection)
  useEffect(() => {
    authenticationsRef.current = authentications;
  }, [authentications]);

  useEffect(() => {
    if (!serverId || connectedRef.current) return;
    
    let outputUnlistener: UnlistenFn | null = null;
    let closedUnlistener: UnlistenFn | null = null;

    const connect = async () => {
      try {
        connectedRef.current = true;
        write(`Connecting to server ${server.name}...\r\n`);

        // Get authentication credentials
        const auth = authenticationsRef.current.find(a => a.id === server.authId);

        console.log('Connecting Debug:', {
          serverAuthId: server.authId,
          authentications: authenticationsRef.current,
          foundAuth: auth
        });

        let password = '';
        if (auth?.type === 'password') {
          password = auth.password || '';
        }

        console.log('Resolved password:', password ? '******' : '<empty>');

        // For SSH key authentication, use the passphrase if available
        // Note: In a real implementation, the key would be passed differently

        const params = {
          host: server.host,
          port: server.port,
          username: server.username,
          password
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
  }, [server]);

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

    let resizeTimeout: ReturnType<typeof setTimeout>;

    const handleResize = () => {
      clearTimeout(resizeTimeout);
      resizeTimeout = setTimeout(() => {
        const cols = terminal.cols || 80;
        const rows = terminal.rows || 24;

        invoke('resize_terminal', {
          params: {
            session_id: sessionId,
            cols,
            rows,
          }
        }).catch(err => console.error('Terminal resize failed:', err));
      }, 300);
    };

    window.addEventListener('resize', handleResize);
    return () => {
      clearTimeout(resizeTimeout);
      window.removeEventListener('resize', handleResize);
    };
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
