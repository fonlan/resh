import { useEffect, useRef, useCallback, useMemo } from 'react';
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import 'xterm/css/xterm.css';
import { TerminalSettings } from '../types/config';

export const useTerminal = (containerId: string, settings?: TerminalSettings, theme?: 'light' | 'dark' | 'system') => {
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    const container = document.getElementById(containerId);
    if (!container) return;

    // Determine actual theme
    let actualTheme: 'light' | 'dark' = 'dark';
    if (theme === 'system') {
      actualTheme = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    } else if (theme === 'light') {
      actualTheme = 'light';
    }

    // Create terminal with user settings and theme
    const term = new Terminal({
      cursorBlink: true,
      fontSize: settings?.fontSize || 14,
      fontFamily: settings?.fontFamily || 'Consolas, monospace',
      cursorStyle: (settings?.cursorStyle as 'block' | 'underline' | 'bar') || 'block',
      scrollback: settings?.scrollback || 5000,
      theme: actualTheme === 'light' ? {
        background: '#ffffff',
        foreground: '#1a202c',
        cursor: '#1a202c',
        cursorAccent: '#ffffff',
        selectionBackground: 'rgba(0, 245, 255, 0.3)',
      } : {
        background: '#000000',
        foreground: '#ffffff',
        cursor: '#00f5ff',
        cursorAccent: '#000000',
        selectionBackground: 'rgba(0, 245, 255, 0.3)',
      },
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    fitAddon.fit();
    
    // Use ResizeObserver to handle container size changes
    // This is more robust than window 'resize' event as it handles
    // cases like switching tabs (display: none -> display: block)
    const resizeObserver = new ResizeObserver(() => {
      if (container.clientWidth > 0 && container.clientHeight > 0) {
    term.open(container);
    fitAddon.fit();
      }
    });
    resizeObserver.observe(container);

    term.write('Connected to terminal\r\n');

    terminalRef.current = term;
    fitAddonRef.current = fitAddon;

    return () => {
      resizeObserver.disconnect();
      term.dispose();
    };
  }, [containerId, settings, theme]);

  const write = useCallback((data: string) => {
    terminalRef.current?.write(data);
  }, []);

  const dispose = useCallback(() => {
    terminalRef.current?.dispose();
  }, []);

  return useMemo(() => ({
    terminal: terminalRef.current,
    write,
    dispose,
  }), [write, dispose]);
};
