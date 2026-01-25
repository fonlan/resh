import { useEffect, useRef } from 'react';
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import 'xterm/css/xterm.css';

export const useTerminal = (containerId: string) => {
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    const container = document.getElementById(containerId);
    if (!container) return;

    // Create terminal
    const term = new Terminal({
      cursorBlink: true,
      theme: {
        background: '#000000',
        foreground: '#ffffff',
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
  }, [containerId]);

  return {
    terminal: terminalRef.current,
    write: (data: string) => terminalRef.current?.write(data),
    dispose: () => terminalRef.current?.dispose(),
  };
};
