import { useEffect, useRef } from 'react';
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

    term.open(container);
    fitAddon.fit();

    term.write('Connected to terminal\r\n');

    terminalRef.current = term;
    fitAddonRef.current = fitAddon;

    // Handle resize
    const handleResize = () => {
      fitAddon.fit();
    };
    window.addEventListener('resize', handleResize);

    return () => {
      window.removeEventListener('resize', handleResize);
      term.dispose();
    };
  }, [containerId, settings, theme]);

  return {
    terminal: terminalRef.current,
    write: (data: string) => terminalRef.current?.write(data),
    dispose: () => terminalRef.current?.dispose(),
  };
};
