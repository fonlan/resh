import { useEffect, useRef, useCallback, useMemo, useState } from 'react';
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { WebglAddon } from 'xterm-addon-webgl';
import 'xterm/css/xterm.css';
import { TerminalSettings } from '../types/config';
import { debounce } from '../utils/common';

export const useTerminal = (
  containerId: string, 
  settings?: TerminalSettings, 
  theme?: 'light' | 'dark' | 'orange' | 'green' | 'system',
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
    let actualTheme: 'light' | 'dark' | 'orange' | 'green' = 'dark';
    if (theme === 'system') {
      actualTheme = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    } else if (theme === 'light') {
      actualTheme = 'light';
    } else if (theme === 'orange') {
      actualTheme = 'orange';
    } else if (theme === 'green') {
      actualTheme = 'green';
    }

    const term = new Terminal({
      cursorBlink: true,
      fontSize: settings?.fontSize || 14,
      fontFamily: settings?.fontFamily || 'Consolas, monospace',
      cursorStyle: (settings?.cursorStyle as 'block' | 'underline' | 'bar') || 'block',
      scrollback: settings?.scrollback || 5000,
      theme: actualTheme === 'light' ? {
        background: '#ffffff', foreground: '#1a202c', cursor: '#1a202c', cursorAccent: '#ffffff', selectionBackground: 'rgba(0, 245, 255, 0.3)',
      } : actualTheme === 'orange' ? {
        background: '#1c1917', foreground: '#fafaf9', cursor: '#f97316', cursorAccent: '#1c1917', selectionBackground: 'rgba(249, 115, 22, 0.3)',
      } : actualTheme === 'green' ? {
        background: '#0a0f0d', foreground: '#f0fdf4', cursor: '#86efac', cursorAccent: '#0a0f0d', selectionBackground: 'rgba(134, 239, 172, 0.3)',
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

    const selectionDisposable = term.onSelectionChange(debounce(() => {
        if (term.hasSelection()) {
            const selection = term.getSelection();
            if (selection) {
                navigator.clipboard.writeText(selection).catch(() => {
                    // Failed to copy
                });
            }
        }
    }, 500));

    // Handle OSC 52 (Clipboard)
    const oscDisposable = term.parser.registerOscHandler(52, (data) => {
        try {
            const parts = data.split(';');
            if (parts.length < 2) return false;
            
            const b64Data = parts.slice(1).join(';');
            if (b64Data === '?') return true; 

            const text = new TextDecoder().decode(
                Uint8Array.from(atob(b64Data), c => c.charCodeAt(0))
            );
            
            navigator.clipboard.writeText(text).catch(() => {
                // OSC 52 write failed
            });
            
            return true;
        } catch (e) {
            return false;
        }
    });

    const initTerminal = () => {
        if (!term.element && container.clientWidth > 0 && container.clientHeight > 0) {
            term.open(container);
            fitAddon.fit();
            
            // Initialize WebGL Addon after opening
            if (!webglAddonRef.current) {
                try {
                    const webglAddon = new WebglAddon();
                    term.loadAddon(webglAddon);
                    webglAddonRef.current = webglAddon;
                } catch (e) {
                    // WebGL renderer could not be loaded
                }
            }
        }
    };

    // Initial check
    initTerminal();
    
    const handleResize = debounce(() => {
      if (container.clientWidth > 0 && container.clientHeight > 0) {
        if (!term.element) {
          initTerminal();
        }
        try {
            fitAddon.fit();
            onResizeRef.current?.(term.cols, term.rows);
        } catch (e) {
            // Fit failed
        }
      }
    }, 100);
    
    const resizeObserver = new ResizeObserver(handleResize);
    resizeObserver.observe(container);

    terminalRef.current = term;
    fitAddonRef.current = fitAddon;
    setIsReady(true);

    return () => {
      disposable.dispose();
      selectionDisposable.dispose();
      oscDisposable.dispose();
      resizeObserver.disconnect();
      webglAddonRef.current?.dispose();
      term.dispose();
      terminalRef.current = null;
      webglAddonRef.current = null;
      setIsReady(false);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [containerId]);

  // Update terminal settings dynamically
  useEffect(() => {
    const term = terminalRef.current;
    if (!term) return;

    if (settings) {
      term.options.fontSize = settings.fontSize || 14;
      term.options.fontFamily = settings.fontFamily || 'Consolas, monospace';
      term.options.cursorStyle = (settings.cursorStyle as 'block' | 'underline' | 'bar') || 'block';
      term.options.scrollback = settings.scrollback || 5000;
    }

    // Determine actual theme
    let actualTheme: 'light' | 'dark' | 'orange' | 'green' = 'dark';
    if (theme === 'system') {
      actualTheme = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    } else if (theme === 'light') {
      actualTheme = 'light';
    } else if (theme === 'orange') {
      actualTheme = 'orange';
    } else if (theme === 'green') {
      actualTheme = 'green';
    }

    term.options.theme = actualTheme === 'light' ? {
        background: '#ffffff', foreground: '#1a202c', cursor: '#1a202c', cursorAccent: '#ffffff', selectionBackground: 'rgba(0, 245, 255, 0.3)',
    } : actualTheme === 'orange' ? {
        background: '#1c1917', foreground: '#fafaf9', cursor: '#f97316', cursorAccent: '#1c1917', selectionBackground: 'rgba(249, 115, 22, 0.3)',
    } : actualTheme === 'green' ? {
        background: '#0a0f0d', foreground: '#f0fdf4', cursor: '#86efac', cursorAccent: '#0a0f0d', selectionBackground: 'rgba(134, 239, 172, 0.3)',
    } : {
        background: '#000000', foreground: '#ffffff', cursor: '#00f5ff', cursorAccent: '#000000', selectionBackground: 'rgba(0, 245, 255, 0.3)',
    };

    // Re-fit after settings change (e.g. font size)
    // Small timeout to ensure DOM update if necessary
    setTimeout(() => {
        fitAddonRef.current?.fit();
        // Also notify backend about resize if needed
        if (term.element && onResizeRef.current) {
             onResizeRef.current(term.cols, term.rows);
        }
    }, 10);
    
  }, [settings, theme]);

  const write = useCallback((data: string) => {
    terminalRef.current?.write(data);
  }, []);

  const focus = useCallback(() => {
    terminalRef.current?.focus();
  }, []);

  const getBufferText = useCallback(() => {
    if (!terminalRef.current) return '';
    const buffer = terminalRef.current.buffer.active;
    let text = '';
    for (let i = 0; i < buffer.length; i++) {
      const line = buffer.getLine(i);
      if (line) {
        text += line.translateToString(true) + '\n';
      }
    }
    return text;
  }, []);

  return useMemo(() => ({
    terminal: terminalRef.current,
    isReady,
    write,
    focus,
    getBufferText,
  }), [isReady, write, focus, getBufferText]);
};
