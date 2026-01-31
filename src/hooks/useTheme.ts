import { useEffect, useRef } from 'react';

export function useTheme(theme?: 'light' | 'dark' | 'orange' | 'green' | 'system') {
  const prevThemeRef = useRef<string | null>(null);

  useEffect(() => {
    if (!theme) return;

    if (prevThemeRef.current === theme && prevThemeRef.current !== null) {
      return;
    }

    const root = document.documentElement;
    root.classList.remove('theme-light', 'theme-dark', 'theme-orange', 'theme-green', 'theme-system');

    if (theme === 'system') {
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      root.classList.add(prefersDark ? 'theme-dark' : 'theme-light');

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
  }, [theme]);
}
