import React, { createContext, useContext, useCallback, useEffect, useState, ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Config } from '../types/config';

interface ConfigContextType {
  config: Config | null;
  loading: boolean;
  error: string | null;
  loadConfig: () => Promise<void>;
  saveConfig: (syncPart: Config, localPart: Config) => Promise<void>;
}

const ConfigContext = createContext<ConfigContextType | undefined>(undefined);

export const ConfigProvider: React.FC<{ children: ReactNode }> = ({ children }) => {
  const [config, setConfig] = useState<Config | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadConfig = useCallback(async () => {
    try {
      setLoading(true);
      console.log('[ConfigProvider] Loading config...');
      const merged = await invoke<Config>('get_merged_config');
      console.log('[ConfigProvider] Loaded config:', merged);
      setConfig(merged);
      setError(null);
    } catch (err) {
      console.error('[ConfigProvider] Failed to load config:', err);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  const saveConfig = useCallback(async (syncPart: Config, localPart: Config) => {
    try {
      console.log('[ConfigProvider] Saving config...');
      await invoke('save_config', { syncPart, localPart });
      console.log('[ConfigProvider] Config saved successfully');
      
      // Update global state
      setConfig(localPart);
      setError(null);
    } catch (err) {
      console.error('[ConfigProvider] Failed to save config:', err);
      setError(err instanceof Error ? err.message : String(err));
      throw err;
    }
  }, []);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  return (
    <ConfigContext.Provider value={{ config, loading, error, loadConfig, saveConfig }}>
      {children}
    </ConfigContext.Provider>
  );
};

export const useConfig = () => {
  const context = useContext(ConfigContext);
  if (context === undefined) {
    throw new Error('useConfig must be used within a ConfigProvider');
  }
  return context;
};
