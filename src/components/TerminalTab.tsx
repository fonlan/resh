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
  servers: Server[];
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
  servers,
  authentications,
  proxies,
  terminalSettings,
  theme,
}) => {
  const { t } = useTranslation();
  const containerId = `terminal-${tabId}`;

  // Memoize settings to prevent re-creating terminal on reference change
  const memoizedSettings = React.useMemo(() => {
      if (!terminalSettings) return undefined;
      return { ...terminalSettings };
  }, [terminalSettings]);

  const [sessionId, setSessionId] = useState<string | null>(null);
  
  // Define handleData before useTerminal
  const handleData = React.useCallback((data: string) => {
    if (sessionIdRef.current) {
      invoke('send_command', { params: { session_id: sessionIdRef.current, command: data } });
    }
  }, []);

  const { terminal, isReady, write, focus } = useTerminal(containerId, memoizedSettings, theme, handleData);
  const [showManualAuth, setShowManualAuth] = useState(false);
  const [manualCredentials, setManualCredentials] = useState({ username: server.username, password: '', privateKey: '', passphrase: '' });
  const [connectTrigger, setConnectTrigger] = useState(0);
  const connectedRef = useRef(false);
  const sessionIdRef = useRef<string | null>(null);
  const authenticationsRef = useRef(authentications);
  const serversRef = useRef(servers);
  const proxiesRef = useRef(proxies);

  // Use refs for stable access inside the connect effect without triggering it
  const writeRef = useRef(write);
  useEffect(() => {
    writeRef.current = write;
  }, [write]);

  // Update refs when props change
  useEffect(() => {
    authenticationsRef.current = authentications;
    serversRef.current = servers;
    proxiesRef.current = proxies;
  }, [authentications, servers, proxies]);

  // Connection effect - now stabilized
  useEffect(() => {
    if (!serverId || connectedRef.current) return;

    let outputUnlistener: UnlistenFn | null = null;
    let closedUnlistener: UnlistenFn | null = null;

    const connect = async (manualCreds?: typeof manualCredentials) => {
      try {
        connectedRef.current = true;
        writeRef.current(t.terminalTab.connecting.replace('{name}', server.name) + '\r\n');

        let password = manualCreds?.password;
        let private_key = manualCreds?.privateKey;
        let passphrase = manualCreds?.passphrase;
        let username = manualCreds?.username || server.username;

        if (!manualCreds) {
          const auth = authenticationsRef.current.find(a => a.id === server.authId);
          if (!auth && server.authId) {
            writeRef.current('\r\n' + t.terminalTab.error.replace('{error}', 'Authentication not found.') + '\r\n');
            setShowManualAuth(true);
            connectedRef.current = false;
            return;
          }
          if (auth?.type === 'password') {
            password = auth.password;
            username = auth.username || server.username;
          } else if (auth?.type === 'key') {
            private_key = auth.keyContent;
            passphrase = auth.passphrase;
          }
        }

        const proxy = proxiesRef.current.find(p => p.id === server.proxyId);
        let jumphost = null;
        if (server.jumphostId) {
          const jhServer = serversRef.current.find(s => s.id === server.jumphostId);
          if (jhServer) {
            const jhAuth = authenticationsRef.current.find(a => a.id === jhServer.authId);
            let jhUsername = jhServer.username;
            let jhPassword: string | undefined = undefined;
            let jhPrivateKey: string | undefined = undefined;
            let jhPassphrase: string | undefined = undefined;
            if (jhAuth?.type === 'password') {
              jhPassword = jhAuth.password;
              jhUsername = jhAuth.username || jhServer.username;
            } else if (jhAuth?.type === 'key') {
              jhPrivateKey = jhAuth.keyContent;
              jhPassphrase = jhAuth.passphrase;
            }
            jumphost = { host: jhServer.host, port: jhServer.port, username: jhUsername, password: jhPassword, private_key: jhPrivateKey, passphrase: jhPassphrase };
          }
        }

        const response = await invoke<{ session_id: string }>('connect_to_server', { 
          params: { host: server.host, port: server.port, username, password, private_key, passphrase, proxy: proxy || null, jumphost: jumphost || null } 
        });
        
        const sid = response.session_id;
        sessionIdRef.current = sid;
        setSessionId(sid);
        setShowManualAuth(false);
        writeRef.current(t.terminalTab.connected.replace('{id}', sid) + '\r\n');

        outputUnlistener = await listen<string>(`terminal-output:${sid}`, (event) => {
          writeRef.current(event.payload);
        });

        closedUnlistener = await listen(`connection-closed:${sid}`, () => {
          writeRef.current('\r\n' + t.terminalTab.connectionClosed + '\r\n');
        });

      } catch (err) {
        writeRef.current('\r\n' + t.terminalTab.error.replace('{error}', String(err)) + '\r\n');
        connectedRef.current = false;
      }
    };

    if (connectTrigger > 0) {
      connect(manualCredentials.password || manualCredentials.privateKey ? manualCredentials : undefined);
    } else if (!showManualAuth) {
      connect();
    }

    return () => {
      if (outputUnlistener) outputUnlistener();
      if (closedUnlistener) closedUnlistener();
      const currentSid = sessionIdRef.current;
      if (currentSid) {
        invoke('close_session', { session_id: currentSid }).catch(err => console.error(`Failed to close session ${currentSid}:`, err));
      }
    };
  }, [serverId, server.name, server.host, server.port, server.username, server.authId, server.proxyId, server.jumphostId, t, showManualAuth, connectTrigger, manualCredentials]);

  // Terminal focus effect (input is now handled inside useTerminal)
  useEffect(() => {
    if (!terminal || !isReady || !sessionId) return;
    if (isActive) focus();
  }, [terminal, isReady, sessionId, isActive, focus]);

  // Focus on active change
  useEffect(() => {
    if (isActive && isReady) focus();
  }, [isActive, isReady, focus]);

  // Resize handler
  useEffect(() => {
    if (!terminal || !isReady || !sessionId) return;
    let resizeTimeout: ReturnType<typeof setTimeout>;
    const handleResize = () => {
      clearTimeout(resizeTimeout);
      resizeTimeout = setTimeout(() => {
        invoke('resize_terminal', { params: { session_id: sessionId, cols: terminal.cols || 80, rows: terminal.rows || 24 } })
          .catch(err => console.error('Terminal resize failed:', err));
      }, 300);
    };
    window.addEventListener('resize', handleResize);
    return () => { clearTimeout(resizeTimeout); window.removeEventListener('resize', handleResize); };
  }, [terminal, isReady, sessionId]);

  return (
    <div className="relative w-full h-full">
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
      
      {showManualAuth && isActive && (
        <div className="absolute inset-0 bg-black/80 flex items-center justify-center z-10">
          <div className="bg-gray-900 p-6 rounded-lg border border-gray-700 w-full max-w-md shadow-2xl">
            <h3 className="text-lg font-semibold text-white mb-4">Manual Authentication</h3>
            <p className="text-sm text-gray-400 mb-4">Credentials for {server.host} not found in config. Please enter manually:</p>
            
            <div className="space-y-4">
              <div>
                <label htmlFor="manual-username" className="block text-xs text-gray-500 mb-1">Username</label>
                <input 
                  id="manual-username"
                  type="text" 
                  value={manualCredentials.username}
                  onChange={e => setManualCredentials(prev => ({ ...prev, username: e.target.value }))}
                  className="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-white"
                />
              </div>
              
              <div>
                <label htmlFor="manual-password" className="block text-xs text-gray-500 mb-1">Password</label>
                <input 
                  id="manual-password"
                  type="password" 
                  value={manualCredentials.password}
                  onChange={e => setManualCredentials(prev => ({ ...prev, password: e.target.value }))}
                  className="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-white"
                  placeholder="Leave empty if using key"
                />
              </div>

              <div className="text-center text-xs text-gray-600 my-2">— OR —</div>

              <div>
                <label htmlFor="manual-key" className="block text-xs text-gray-500 mb-1">Private Key (PEM)</label>
                <textarea 
                  id="manual-key"
                  value={manualCredentials.privateKey}
                  onChange={e => setManualCredentials(prev => ({ ...prev, privateKey: e.target.value }))}
                  className="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-white font-mono text-[10px]"
                  rows={4}
                />
              </div>
              
              <div className="flex gap-3 mt-6">
                <button 
                  type="button"
                  onClick={() => setShowManualAuth(false)}
                  className="flex-1 bg-gray-800 hover:bg-gray-700 text-white py-2 rounded transition-colors"
                >
                  Cancel
                </button>
                <button 
                  type="button"
                  onClick={() => {
                    connectedRef.current = false;
                    setConnectTrigger(prev => prev + 1);
                  }}
                  className="flex-1 bg-blue-600 hover:bg-blue-500 text-white py-2 rounded transition-colors"
                >
                  Connect
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
