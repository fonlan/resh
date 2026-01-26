import { useEffect, useRef } from 'react';
import './App.css';
import { MainWindow } from './components/MainWindow';
import { useConfig } from './hooks/useConfig';

function App() {
  const { config } = useConfig();

  // Track previous config to detect changes
  const prevThemeRef = useRef<string | null>(null);

  // Apply theme based on config
  useEffect(() => {
    if (!config) return;

    const theme = config.general.theme;

    // Check if theme actually changed
    if (prevThemeRef.current === theme && prevThemeRef.current !== null) {
      return;
    }

    const root = document.documentElement;

    // Remove existing theme classes
    root.classList.remove('theme-light', 'theme-dark', 'theme-system');

    if (theme === 'system') {
      // Use system preference
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      root.classList.add(prefersDark ? 'theme-dark' : 'theme-light');

      // Listen for system theme changes
      const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
      const handleChange = (e: MediaQueryListEvent) => {
        root.classList.remove('theme-light', 'theme-dark');
        root.classList.add(e.matches ? 'theme-dark' : 'theme-light');
      };
      mediaQuery.addEventListener('change', handleChange);
      return () => mediaQuery.removeEventListener('change', handleChange);
    } else {
      root.classList.add(`theme-${theme}`);
    }

    prevThemeRef.current = theme;
  }, [config]);

  return (
    <div className="app-container">
      <MainWindow />
    </div>
  );
}

export default App;
