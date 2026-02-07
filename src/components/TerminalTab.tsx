import React, { useEffect, useRef, useState, useCallback, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useTerminal } from '../hooks/useTerminal';
import { useConfig } from '../hooks/useConfig'; // Import useConfig
import { Server, Authentication, ProxyConfig, TerminalSettings, ManualAuthCredentials } from '../types';
import { v4 as uuidv4 } from 'uuid';
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
  theme?: 'light' | 'dark' | 'orange' | 'green' | 'system';
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
  const { config, saveConfig } = useConfig(); // Use config
  const containerId = `terminal-${tabId}`;

  // Memoize settings to prevent noisy updates from object identity changes
  const memoizedSettings = useMemo(() => {
    if (!terminalSettings) return undefined
    return { ...terminalSettings }
  }, [
    terminalSettings?.fontSize,
    terminalSettings?.fontFamily,
    terminalSettings?.cursorStyle,
    terminalSettings?.scrollback,
  ])

  const [sessionId, setSessionId] = useState<string | null>(null);
  const [isConnected, setIsConnected] = useState(false);
  
  // Status Bar State
  const [statusText, setStatusText] = useState<string>('');
  const inputBufferRef = useRef<string>('');
  const isInputModeRef = useRef<boolean>(false);
  const sessionIdRef = useRef<string | null>(null);
  const queuedTerminalInputRef = useRef<string>('');
  const inputFlushTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isFlushingQueuedInputRef = useRef<boolean>(false);

  const applySendError = useCallback((err: unknown) => {
    const errorStr = String(err);
    if (errorStr.includes('Connection lost') || errorStr.includes('reconnect')) {
      setStatusText('Reconnecting...');
    } else {
      setIsConnected(false);
    }
  }, []);

  const flushQueuedInput = useCallback(async () => {
    if (isFlushingQueuedInputRef.current || !sessionIdRef.current || !queuedTerminalInputRef.current) {
      return;
    }

    if (inputFlushTimerRef.current) {
      clearTimeout(inputFlushTimerRef.current);
      inputFlushTimerRef.current = null;
    }

    isFlushingQueuedInputRef.current = true;
    let payload = '';

    try {
      while (sessionIdRef.current && queuedTerminalInputRef.current) {
        payload = queuedTerminalInputRef.current;
        queuedTerminalInputRef.current = '';

        await invoke('send_command', {
          params: {
            session_id: sessionIdRef.current,
            command: payload,
          },
        });

        payload = '';
      }
    } catch (err) {
      if (payload) {
        queuedTerminalInputRef.current = payload + queuedTerminalInputRef.current;
      }
      applySendError(err);
    } finally {
      isFlushingQueuedInputRef.current = false;
    }
  }, [applySendError]);

  const scheduleQueuedInputFlush = useCallback(() => {
    if (inputFlushTimerRef.current || isFlushingQueuedInputRef.current) {
      return;
    }

    inputFlushTimerRef.current = setTimeout(() => {
      inputFlushTimerRef.current = null;
      void flushQueuedInput();
    }, 12);
  }, [flushQueuedInput]);

  // Define handleData before useTerminal
  const handleData = useCallback((data: string) => {
    const { newBuffer, commandExecuted } = processInputBuffer(data, inputBufferRef.current);
    
    inputBufferRef.current = newBuffer;
    isInputModeRef.current = true;
    
    if (commandExecuted !== null) {
      setStatusText(commandExecuted);
    }

    if (!sessionIdRef.current) {
      return;
    }

    queuedTerminalInputRef.current += data;

    const shouldFlushImmediately =
      data.includes('\r') ||
      data.includes('\n') ||
      data.includes('\u0003') ||
      data.includes('\u001b');

    if (shouldFlushImmediately) {
      void flushQueuedInput();
    } else {
      scheduleQueuedInputFlush();
    }
  }, [flushQueuedInput, scheduleQueuedInputFlush]);

  const handleResize = useCallback((cols: number, rows: number) => {
    if (sessionIdRef.current) {
      invoke('resize_terminal', { params: { session_id: sessionIdRef.current, cols, rows } })
        .catch(() => {
          // Terminal resize failed
        });
    }
  }, []);

  const { terminal, isReady, write, focus, getBufferText } = useTerminal(containerId, sessionIdRef, memoizedSettings, theme, handleData, handleResize);

  // Determine container background based on theme
  const containerBg = useMemo(() => {
    if (theme === 'light') return '#ffffff';
    if (theme === 'dark') return '#000000';
    if (theme === 'orange') return '#1c1917';
    if (theme === 'green') return '#0a0f0d';
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? '#000000' : '#ffffff';
  }, [theme]);

  const [showManualAuth, setShowManualAuth] = useState(false);
  const [isCancelled, setIsCancelled] = useState(false);
  const [isAuthRetry, setIsAuthRetry] = useState(false);
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
  const serverRef = useRef(server)
  const manualCredentialsRef = useRef(manualCredentials)
  const configRef = useRef(config)
  const saveConfigRef = useRef(saveConfig)
  const onSessionChangeRef = useRef(onSessionChange)
  const tRef = useRef(t)

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

  useEffect(() => {
    serverRef.current = server
  }, [server])

  useEffect(() => {
    manualCredentialsRef.current = manualCredentials
  }, [manualCredentials])

  useEffect(() => {
    configRef.current = config
  }, [config])

  useEffect(() => {
    saveConfigRef.current = saveConfig
  }, [saveConfig])

  useEffect(() => {
    onSessionChangeRef.current = onSessionChange
  }, [onSessionChange])

  useEffect(() => {
    tRef.current = t
  }, [t])

  const serverConnectionKey = useMemo(
    () => [
      server.id,
      server.host,
      server.port,
      server.username || '',
      server.authId || '',
      server.proxyId || '',
      server.jumphostId || '',
    ].join('|'),
    [
      server.id,
      server.host,
      server.port,
      server.username,
      server.authId,
      server.proxyId,
      server.jumphostId,
    ]
  )

  // Reset cancellation when server config changes
  useEffect(() => {
    setIsCancelled(false);
  }, [serverConnectionKey]);

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
        const currentServer = serverRef.current
        const currentT = tRef.current

        connectedRef.current = true;
        // Reset input mode on new connection attempt
        isInputModeRef.current = false;
        inputBufferRef.current = '';

        const connectingMsg = currentT.terminalTab.connecting.replace('{name}', currentServer.name);
        updateStatus(connectingMsg);

        let password = manualCreds?.password;
        let private_key = manualCreds?.privateKey;
        let passphrase = manualCreds?.passphrase;
        let username = manualCreds?.username || currentServer.username;

        // Check if manual auth is needed (missing username or auth)
        if (!manualCreds && (!currentServer.username || !currentServer.authId)) {
          setShowManualAuth(true);
          connectedRef.current = false;
          return;
        }

        if (!manualCreds) {
          const auth = authenticationsRef.current.find(a => a.id === currentServer.authId);
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

        // Get proxy from jumphost if configured, otherwise from target server
        // When using jumphost, the proxy should follow the jumphost's configuration
        let proxy = null;
        if (currentServer.jumphostId) {
          const jhServer = serversRef.current.find(s => s.id === currentServer.jumphostId);
          if (jhServer) {
            proxy = proxiesRef.current.find(p => p.id === jhServer.proxyId);
          }
        }
        // Fallback to target server's proxy if no jumphost or jumphost has no proxy
        if (!proxy) {
          proxy = proxiesRef.current.find(p => p.id === currentServer.proxyId);
        }

        let jumphost = null;
        if (currentServer.jumphostId) {
          const jhServer = serversRef.current.find(s => s.id === currentServer.jumphostId);
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
              host: currentServer.host,
              port: currentServer.port,
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
        onSessionChangeRef.current?.(sid);
        setShowManualAuth(false);
        setIsConnected(true);
        const connectedMsg = currentT.terminalTab.connected.replace('{id}', sid);
        updateStatus(connectedMsg);

        // Save credentials asynchronously if rememberMe is true
        const currentConfig = configRef.current
        if (manualCreds?.rememberMe && currentConfig) {
          const authId = uuidv4();
          const newAuth: Authentication = {
            id: authId,
            name: `${currentServer.name} Auth`,
            type: manualCreds.privateKey ? 'key' : 'password',
            keyContent: manualCreds.privateKey,
            passphrase: manualCreds.passphrase,
            password: manualCreds.password,
            synced: false,
            updatedAt: new Date().toISOString(),
          };

          const updatedConfig = {
            ...currentConfig,
            authentications: [...currentConfig.authentications, newAuth],
            servers: currentConfig.servers.map((s) =>
              s.id === currentServer.id ? {
                ...s, 
                authId, 
                username: manualCreds.username,
                updatedAt: new Date().toISOString()
              } : s
            ),
          };

          // Save credentials in background without blocking
          saveConfigRef.current(updatedConfig).catch(() => {
            // Silently fail - credential saving is non-critical
          });
        }

        outputUnlistener = await listen<string>(`terminal-output:${sid}`, (event) => {
          writeRef.current(event.payload);
        });

        closedUnlistener = await listen(`connection-closed:${sid}`, () => {
          updateStatus(tRef.current.terminalTab.connectionClosed);
          setIsConnected(false);
          connectedRef.current = false;
        });

      } catch (err) {
        const errorStr = String(err);
        const errorMsg = tRef.current.terminalTab.error.replace('{error}', errorStr);
        writeRef.current('\r\n' + errorMsg + '\r\n');

        // Check if authentication failed and password is required
        if (errorStr.includes('AUTH_PASSWORD_REQUIRED')) {
          setManualCredentials(prev => ({ ...prev, password: '', privateKey: '', passphrase: '' }));
          setIsAuthRetry(true);
          setShowManualAuth(true);
          updateStatus(tRef.current.manualAuth.title);
          connectedRef.current = false;
          setIsConnected(false);
        } else {
          updateStatus(`Error: ${errorStr}`);
          connectedRef.current = false;
          setIsConnected(false);
        }
      }
    };

    if (connectTrigger > 0) {
      const currentManualCredentials = manualCredentialsRef.current
      connect(currentManualCredentials.password || currentManualCredentials.privateKey ? currentManualCredentials : undefined);
    } else if (!showManualAuth && !isCancelled) {
      connect();
    }

    return () => {
      if (outputUnlistener) outputUnlistener();
      if (closedUnlistener) closedUnlistener();
      const currentSid = sessionIdRef.current;
      if (currentSid) {
        invoke('close_session', { session_id: currentSid }).catch(() => {
          // Failed to close session
        });
        sessionIdRef.current = null
        setSessionId(null)
        onSessionChangeRef.current?.(null);
      }
    };
  }, [serverId, serverConnectionKey, showManualAuth, isCancelled, connectTrigger]);

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

  useEffect(() => {
    return () => {
      if (inputFlushTimerRef.current) {
        clearTimeout(inputFlushTimerRef.current)
        inputFlushTimerRef.current = null
      }
      queuedTerminalInputRef.current = ''
      isFlushingQueuedInputRef.current = false
    }
  }, []);

  // Listen for export logs event
  useEffect(() => {
    const handleExportLogs = async () => {
      if (!isReady) return;
      const content = getBufferText();
      const defaultPath = `resh-log-${server.name.replace(/[^a-z0-9]/gi, '_').toLowerCase()}-${new Date().toISOString().replace(/[:.]/g, '-')}.txt`;
      
      try {
        await invoke('export_terminal_log', { content, defaultPath });
      } catch (err) {
        // Failed to export logs
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
          // Failed to start recording
        }
      }
    };

    const handleStopRecording = async () => {
      if (sessionIdRef.current) {
        try {
          await invoke('stop_recording', { sessionId: sessionIdRef.current });
        } catch (err) {
          // Failed to stop recording
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

  // Listen for reconnect event
  useEffect(() => {
    const handleReconnect = async () => {
      if (!sessionIdRef.current) return;
      
      try {
        await invoke('reconnect_session', { sessionId: sessionIdRef.current });
        setIsConnected(true);
        const reconnectedMsg = t.terminalTab.connected.replace('{id}', sessionIdRef.current);
        setStatusText(reconnectedMsg);
      } catch (err) {
        const errorStr = String(err);
        const errorMsg = t.terminalTab.error.replace('{error}', errorStr);
        writeRef.current('\r\n' + errorMsg + '\r\n');
        setIsConnected(false);
      }
    };

    window.addEventListener(`reconnect:${tabId}`, handleReconnect as unknown as EventListener);

    return () => {
      window.removeEventListener(`reconnect:${tabId}`, handleReconnect as unknown as EventListener);
    };
  }, [tabId, t]);

  // Sync terminal size with backend upon connection
  useEffect(() => {
    if (isConnected && terminal && sessionId) {
      invoke('resize_terminal', {
        params: {
          session_id: sessionId,
          cols: terminal.cols,
          rows: terminal.rows,
          },
        }).catch(() => {
          // Initial terminal resize failed
        });
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
            isRetry={isAuthRetry}
            onConnect={() => {
              setIsCancelled(false);
              setIsAuthRetry(false);
              connectedRef.current = false;
              setConnectTrigger(prev => prev + 1);
            }}
            onCancel={() => {
              setIsCancelled(true);
              setShowManualAuth(false);
              setIsAuthRetry(false);
              setStatusText(t.terminalTab.connectionClosed);
            }}
          />
        )}
      </div>
    </div>
  );
});
