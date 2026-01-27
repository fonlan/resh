import { useEffect, useRef, useCallback, useMemo, useState } from 'react';
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import 'xterm/css/xterm.css';
import { TerminalSettings } from '../types/config';

export const useTerminal = (
  containerId: string, 
  settings?: TerminalSettings, 
  theme?: 'light' | 'dark' | 'system',
  onData?: (data: string) => void
) => {
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const [isReady, setIsReady] = useState(false);

  useEffect(() => {
    const container = document.getElementById(containerId);
    if (!container) return;

    // ... (theme logic remains the same)
    let actualTheme: 'light' | 'dark' = 'dark';
    if (theme === 'system') {
      actualTheme = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    } else if (theme === 'light') {
      actualTheme = 'light';
    }

    const term = new Terminal({
      cursorBlink: true,
      fontSize: settings?.fontSize || 14,
      fontFamily: settings?.fontFamily || 'Consolas, monospace',
      cursorStyle: (settings?.cursorStyle as 'block' | 'underline' | 'bar') || 'block',
      scrollback: settings?.scrollback || 5000,
      theme: actualTheme === 'light' ? {
        background: '#ffffff', foreground: '#1a202c', cursor: '#1a202c', cursorAccent: '#ffffff', selectionBackground: 'rgba(0, 245, 255, 0.3)',
      } : {
        background: '#000000', foreground: '#ffffff', cursor: '#00f5ff', cursorAccent: '#000000', selectionBackground: 'rgba(0, 245, 255, 0.3)',
      },
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    // Register onData inside the hook to ensure it's always attached to the current term
    const disposable = term.onData((data) => {
      if (onData) onData(data);
    });

    if (container.clientWidth > 0 && container.clientHeight > 0) {
      term.open(container);
      fitAddon.fit();
    }
    
    const resizeObserver = new ResizeObserver(() => {
      if (container.clientWidth > 0 && container.clientHeight > 0) {
        if (!term.element) term.open(container);
        fitAddon.fit();
      }
    });
    resizeObserver.observe(container);

    terminalRef.current = term;
    fitAddonRef.current = fitAddon;
    setIsReady(true);

    return () => {
      disposable.dispose();
      resizeObserver.disconnect();
      term.dispose();
      terminalRef.current = null;
      setIsReady(false);
    };
  }, [containerId, settings, theme, onData]); // onData is now a dependency

  const write = useCallback((data: string) => {
    terminalRef.current?.write(data);
  }, []);

  const focus = useCallback(() => {
    terminalRef.current?.focus();
  }, []);

  return useMemo(() => ({
    terminal: terminalRef.current,
    isReady,
    write,
    focus,
  }), [isReady, write, focus]);
};
