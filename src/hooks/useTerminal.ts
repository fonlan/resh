import { useEffect, useRef, useCallback, useMemo, useState } from 'react';
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { WebglAddon } from 'xterm-addon-webgl';
import 'xterm/css/xterm.css';
import { TerminalSettings } from '../types/config';

export const useTerminal = (
  containerId: string, 
  settings?: TerminalSettings, 
  theme?: 'light' | 'dark' | 'system',
  onData?: (data: string) => void,
  onResize?: (cols: number, rows: number) => void
) => {
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const webglAddonRef = useRef<WebglAddon | null>(null);
  const [isReady, setIsReady] = useState(false);

  const onDataRef = useRef(onData);
  const onResizeRef = useRef(onResize);

  useEffect(() => {
    onDataRef.current = onData;
  }, [onData]);

  useEffect(() => {
    onResizeRef.current = onResize;
  }, [onResize]);

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
      onDataRef.current?.(data);
    });

    if (container.clientWidth > 0 && container.clientHeight > 0) {
      term.open(container);
      fitAddon.fit();
      
      // Initialize WebGL Addon after opening
      try {
        const webglAddon = new WebglAddon();
        term.loadAddon(webglAddon);
        webglAddonRef.current = webglAddon;
        // term.write('Connected to terminal (WebGL Enabled)\r\n');
      } catch (e) {
        console.warn('WebGL renderer could not be loaded, falling back to canvas', e);
        // term.write('Connected to terminal (Canvas Fallback)\r\n');
      }
    }
    
    const resizeObserver = new ResizeObserver(() => {
      if (container.clientWidth > 0 && container.clientHeight > 0) {
        if (!term.element) {
          term.open(container);
          // Try loading webgl if not already loaded
          if (!webglAddonRef.current) {
            try {
              const webglAddon = new WebglAddon();
              term.loadAddon(webglAddon);
              webglAddonRef.current = webglAddon;
            } catch (e) {
              console.warn('WebGL renderer could not be loaded', e);
            }
          }
        }
        
        // Fit terminal to container
        try {
            fitAddon.fit();
            onResizeRef.current?.(term.cols, term.rows);
        } catch (e) {
            console.warn('Fit failed', e);
        }
      }
    });
    resizeObserver.observe(container);

    terminalRef.current = term;
    fitAddonRef.current = fitAddon;
    setIsReady(true);

    return () => {
      disposable.dispose();
      resizeObserver.disconnect();
      webglAddonRef.current?.dispose();
      term.dispose();
      terminalRef.current = null;
      webglAddonRef.current = null;
      setIsReady(false);
    };
  }, [containerId, settings, theme]); // onData and onResize removed from dependencies

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
