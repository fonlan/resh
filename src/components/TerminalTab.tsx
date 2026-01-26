import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useTerminal } from '../hooks/useTerminal';
import { Server, Authentication, ProxyConfig, TerminalSettings } from '../types/config';
import { useTranslation } from '../i18n';

type UnlistenFn = () => void;

interface TerminalTabProps {
  tabId: string;
  serverId: string;
  isActive: boolean;
  onClose: () => void;
  server: Server;
  authentications: Authentication[];
  proxies: ProxyConfig[];
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
  const { t } = useTranslation();
  const containerId = `terminal-${tabId}`;
  const { terminal, write } = useTerminal(containerId, terminalSettings, theme);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const connectedRef = useRef(false);
  const sessionIdRef = useRef<string | null>(null);
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
        write(t.terminalTab.connecting.replace('{name}', server.name) + '\r\n');

        // Get authentication credentials
        const auth = authenticationsRef.current.find(a => a.id === server.authId);

        console.log('Connecting Debug:', {
          serverAuthId: server.authId,
          authentications: authenticationsRef.current,
          foundAuth: auth
        });

        let password = undefined;
        let private_key = undefined;
        let passphrase = undefined;

        if (auth?.type === 'password') {
          password = auth.password;
        } else if (auth?.type === 'key') {
          private_key = auth.keyContent;
          passphrase = auth.passphrase;
        }

        console.log('Resolved auth:', {
          type: auth?.type,
          hasPassword: !!password,
          hasKey: !!private_key,
          hasPassphrase: !!passphrase
        });

        const params = {
          host: server.host,
          port: server.port,
          username: server.username,
          password,
          private_key,
          passphrase
        };

        const response = await invoke<{ session_id: string }>('connect_to_server', { params });
        const sid = response.session_id;
        sessionIdRef.current = sid;
        setSessionId(sid);
        write(t.terminalTab.connected.replace('{id}', sid) + '\r\n');

        outputUnlistener = await listen<string>(`terminal-output:${sid}`, (event) => {
          write(event.payload);
        });

        closedUnlistener = await listen(`connection-closed:${sid}`, () => {
          write('\r\n' + t.terminalTab.connectionClosed + '\r\n');
        });

      } catch (err) {
        write('\r\n' + t.terminalTab.error.replace('{error}', String(err)) + '\r\n');
        connectedRef.current = false;
      }
    };

    connect();

    return () => {
      if (outputUnlistener) outputUnlistener();
      if (closedUnlistener) closedUnlistener();

      const currentSid = sessionIdRef.current;
      if (currentSid) {
        invoke('close_session', { session_id: currentSid }).catch(err =>
          console.error(`Failed to close session ${currentSid}:`, err)
        );
      }
    };
  }, [serverId, server.name, server.host, server.port, server.username, server.authId, write, t]);

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
