import React, { useEffect, useRef, useState, useCallback, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useTerminal } from '../hooks/useTerminal';
import { useConfig } from '../hooks/useConfig'; // Import useConfig
import { Server, Authentication, ProxyConfig, TerminalSettings, ManualAuthCredentials } from '../types/config';
import { useTranslation } from '../i18n';
import { StatusBar } from './StatusBar';
import { ManualAuthModal } from './ManualAuthModal';
import { processInputBuffer } from '../utils/terminalUtils';

type UnlistenFn = () => void;

interface TerminalTabProps {
  tabId: string;
  serverId: string;
  isActive: boolean;
  onClose: (tabId: string) => void;
  server: Server;
  servers: Server[];
  authentications: Authentication[];
  proxies: ProxyConfig[];
  terminalSettings?: TerminalSettings;
  theme?: 'light' | 'dark' | 'system';
  onSessionChange?: (sessionId: string | null) => void;
}

export const TerminalTab = React.memo<TerminalTabProps>(({
  tabId,
  serverId,
  isActive,
  server,
  servers,
  authentications,
  proxies,
  terminalSettings,
  theme,
  onSessionChange,
}) => {
  const { t } = useTranslation();
  const { config } = useConfig(); // Use config
  const containerId = `terminal-${tabId}`;

  // Memoize settings to prevent re-creating terminal on reference change
  const memoizedSettings = useMemo(() => {
    if (!terminalSettings) return undefined;
    return { ...terminalSettings };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [JSON.stringify(terminalSettings)]);

  const [sessionId, setSessionId] = useState<string | null>(null);
  const [isConnected, setIsConnected] = useState(false);
  
  // Status Bar State
  const [statusText, setStatusText] = useState<string>('');
  const inputBufferRef = useRef<string>('');
  const isInputModeRef = useRef<boolean>(false);
  const sessionIdRef = useRef<string | null>(null);

  // Define handleData before useTerminal
  const handleData = useCallback((data: string) => {
    const { newBuffer, commandExecuted } = processInputBuffer(data, inputBufferRef.current);
    
    inputBufferRef.current = newBuffer;
    isInputModeRef.current = true;
    
    if (commandExecuted !== null) {
      setStatusText(commandExecuted);
    }

    if (sessionIdRef.current) {
      invoke('send_command', { params: { session_id: sessionIdRef.current, command: data } });
    }
  }, []);

  const handleResize = useCallback((cols: number, rows: number) => {
    if (sessionIdRef.current) {
      invoke('resize_terminal', { params: { session_id: sessionIdRef.current, cols, rows } })
        .catch(err => console.error('Terminal resize failed:', err));
    }
  }, []);

  const { terminal, isReady, write, focus, getBufferText } = useTerminal(containerId, memoizedSettings, theme, handleData, handleResize);

  // Determine container background based on theme
  const containerBg = useMemo(() => {
    if (theme === 'light') return '#ffffff';
    if (theme === 'dark') return '#000000';
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? '#000000' : '#ffffff';
  }, [theme]);

  const [showManualAuth, setShowManualAuth] = useState(false);
  const [isCancelled, setIsCancelled] = useState(false);
  const [manualCredentials, setManualCredentials] = useState<ManualAuthCredentials>({ 
    username: server.username, 
    password: '', 
    privateKey: '', 
    passphrase: '' 
  });
  const [connectTrigger, setConnectTrigger] = useState(0);
  const connectedRef = useRef(false);
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

  // Reset cancellation when server config changes
  useEffect(() => {
    setIsCancelled(false);
  }, [serverId, server.host, server.port, server.username, server.authId]);

  // Connection effect
  useEffect(() => {
    if (!serverId || connectedRef.current) return;

    let outputUnlistener: UnlistenFn | null = null;
    let closedUnlistener: UnlistenFn | null = null;

    const updateStatus = (text: string) => {
      if (!isInputModeRef.current) {
        setStatusText(text);
      }
    };

    const connect = async (manualCreds?: ManualAuthCredentials) => {
      try {
        connectedRef.current = true;
        // Reset input mode on new connection attempt
        isInputModeRef.current = false;
        inputBufferRef.current = '';

        const connectingMsg = t.terminalTab.connecting.replace('{name}', server.name);
        updateStatus(connectingMsg);

        let password = manualCreds?.password;
        let private_key = manualCreds?.privateKey;
        let passphrase = manualCreds?.passphrase;
        let username = manualCreds?.username || server.username;

        // Check if manual auth is needed (missing username or auth)
        if (!manualCreds && (!server.username || !server.authId)) {
          setShowManualAuth(true);
          connectedRef.current = false;
          return;
        }

        if (!manualCreds) {
          const auth = authenticationsRef.current.find(a => a.id === server.authId);
          if (auth?.type === 'password') {
            password = auth.password || undefined;
          } else if (auth?.type === 'key') {
            private_key = auth.keyContent || undefined;
            passphrase = auth.passphrase || undefined;
          }
        }

        // Ensure username is provided
        if (!username) {
          setShowManualAuth(true);
          connectedRef.current = false;
          return;
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
              jhPassword = jhAuth.password || undefined;
            } else if (jhAuth?.type === 'key') {
              jhPrivateKey = jhAuth.keyContent || undefined;
              jhPassphrase = jhAuth.passphrase || undefined;
            }
            jumphost = {
              host: jhServer.host,
              port: jhServer.port,
              username: jhUsername,
              password: jhPassword || undefined,
              private_key: jhPrivateKey || undefined,
              passphrase: jhPassphrase || undefined
            };
          }
        }

        const response = await invoke<{ session_id: string }>('connect_to_server',
          {
            params: {
              host: server.host,
              port: server.port,
              username,
              password: password || undefined,
              private_key: private_key || undefined,
              passphrase: passphrase || undefined,
              proxy: proxy || null,
              jumphost: jumphost || null
            }
          });
        
        const sid = response.session_id;
        sessionIdRef.current = sid;
        setSessionId(sid);
        onSessionChange?.(sid);
        setShowManualAuth(false);
        setIsConnected(true);
        const connectedMsg = t.terminalTab.connected.replace('{id}', sid);
        updateStatus(connectedMsg);

        outputUnlistener = await listen<string>(`terminal-output:${sid}`, (event) => {
          writeRef.current(event.payload);
        });

        closedUnlistener = await listen(`connection-closed:${sid}`, () => {
          updateStatus(t.terminalTab.connectionClosed);
          setIsConnected(false);
        });

      } catch (err) {
        const errorMsg = t.terminalTab.error.replace('{error}', String(err));
        writeRef.current('\r\n' + errorMsg + '\r\n');
        updateStatus(`Error: ${String(err)}`);
        connectedRef.current = false;
        setIsConnected(false);
      }
    };

    if (connectTrigger > 0) {
      connect(manualCredentials.password || manualCredentials.privateKey ? manualCredentials : undefined);
    } else if (!showManualAuth && !isCancelled) {
      connect();
    }

    return () => {
      if (outputUnlistener) outputUnlistener();
      if (closedUnlistener) closedUnlistener();
      const currentSid = sessionIdRef.current;
      if (currentSid) {
        invoke('close_session', { session_id: currentSid }).catch(err => console.error(`Failed to close session ${currentSid}:`, err));
        onSessionChange?.(null);
      }
    };
  }, [serverId, server.name, server.host, server.port, server.username, server.authId, server.proxyId, server.jumphostId, t, showManualAuth, isCancelled, connectTrigger, manualCredentials, onSessionChange]);

  // Terminal focus effect
  useEffect(() => {
    if (!terminal || !isReady || !sessionId) return;
    if (isActive) focus();
  }, [terminal, isReady, sessionId, isActive, focus]);

  // Focus on active change
  useEffect(() => {
    if (isActive && isReady) focus();
  }, [isActive, isReady, focus]);

  // Listen for snippet paste event
  useEffect(() => {
    if (!isActive) return;

    const handlePasteSnippet = (e: CustomEvent<string>) => {
      const content = e.detail;
      if (content) {
        if (sessionIdRef.current) {
          invoke('send_command', { params: { session_id: sessionIdRef.current, command: content } });
        }
        focus();
      }
    };

    window.addEventListener('paste-snippet', handlePasteSnippet as EventListener);
    return () => {
      window.removeEventListener('paste-snippet', handlePasteSnippet as EventListener);
    };
  }, [isActive, focus]);

  // Listen for export logs event
  useEffect(() => {
    const handleExportLogs = async () => {
      if (!isReady) return;
      const content = getBufferText();
      const defaultPath = `resh-log-${server.name.replace(/[^a-z0-9]/gi, '_').toLowerCase()}-${new Date().toISOString().replace(/[:.]/g, '-')}.txt`;
      
      try {
        await invoke('export_terminal_log', { content, defaultPath });
      } catch (err) {
        console.error('Failed to export logs:', err);
      }
    };

    window.addEventListener(`export-terminal-logs:${tabId}`, handleExportLogs as EventListener);
    return () => {
      window.removeEventListener(`export-terminal-logs:${tabId}`, handleExportLogs as EventListener);
    };
  }, [tabId, isReady, getBufferText, server.name]);

  // Listen for recording events
  useEffect(() => {
    const handleStartRecording = async (e: CustomEvent<{ path: string }>) => {
      const { path } = e.detail;
      const mode = config?.general.recordingMode || 'raw';
      if (sessionIdRef.current) {
        try {
          await invoke('start_recording', { sessionId: sessionIdRef.current, filePath: path, mode });
        } catch (err) {
          console.error('Failed to start recording:', err);
        }
      }
    };

    const handleStopRecording = async () => {
      if (sessionIdRef.current) {
        try {
          await invoke('stop_recording', { sessionId: sessionIdRef.current });
        } catch (err) {
          console.error('Failed to stop recording:', err);
        }
      }
    };

    window.addEventListener(`start-recording:${tabId}`, handleStartRecording as unknown as EventListener);
    window.addEventListener(`stop-recording:${tabId}`, handleStopRecording as unknown as EventListener);

    return () => {
      window.removeEventListener(`start-recording:${tabId}`, handleStartRecording as unknown as EventListener);
      window.removeEventListener(`stop-recording:${tabId}`, handleStopRecording as unknown as EventListener);
    };
  }, [tabId, config?.general.recordingMode]);

  // Sync terminal size with backend upon connection
  useEffect(() => {
    if (isConnected && terminal && sessionId) {
      invoke('resize_terminal', {
        params: {
          session_id: sessionId,
          cols: terminal.cols,
          rows: terminal.rows,
        },
      }).catch((err) => console.error('Initial terminal resize failed:', err));
    }
  }, [isConnected, terminal, sessionId]);

  return (
    <div className="relative w-full h-full flex flex-col" style={{ backgroundColor: containerBg }}>
      <StatusBar
        leftText={statusText}
        rightText={server.username ? `${server.username}@${server.host}` : server.host}
        theme={theme}
        connected={isConnected}
      />
      <div className="relative flex-1" style={{ padding: '8px', minHeight: 0, overflow: 'hidden' }}>
        <div
          id={containerId}
          style={{
            display: isActive ? 'block' : 'none',
            width: '100%',
            height: '100%',
            overflow: 'hidden',
          }}
        />
        
        {showManualAuth && isActive && (
          <ManualAuthModal
            serverName={server.host}
            credentials={manualCredentials}
            onCredentialsChange={setManualCredentials}
            onConnect={() => {
              setIsCancelled(false);
              connectedRef.current = false;
              setConnectTrigger(prev => prev + 1);
            }}
            onCancel={() => {
              setIsCancelled(true);
              setShowManualAuth(false);
              setStatusText(t.terminalTab.connectionClosed);
            }}
          />
        )}
      </div>
    </div>
  );
});
