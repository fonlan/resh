import React, { createContext, useContext, useCallback, useEffect, useState, ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Config } from '../types';
import { logger } from '../utils/logger';

interface ConfigContextType {
  config: Config | null;
  loading: boolean;
  error: string | null;
  loadConfig: () => Promise<void>;
  saveConfig: (config: Config) => Promise<void>;
  triggerSync: () => Promise<Config>;
}

const ConfigContext = createContext<ConfigContextType | undefined>(undefined);

export const ConfigProvider: React.FC<{ children: ReactNode }> = ({ children }) => {
  const [config, setConfig] = useState<Config | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadConfig = useCallback(async () => {
    let loadedConfig: Config | null = null;
    try {
      setLoading(true);
      logger.info('[ConfigProvider] Loading config...');
      loadedConfig = await invoke<Config>('get_config');
      logger.info('[ConfigProvider] Loaded config', { version: loadedConfig.version });
      setConfig(loadedConfig);
      setError(null);
    } catch (err) {
      logger.error('[ConfigProvider] Failed to load config', err);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }

    // Trigger background sync if enabled
    if (loadedConfig?.general?.webdav?.enabled && loadedConfig?.general?.webdav?.url) {
      logger.info('[ConfigProvider] Initiating background startup sync...');
      invoke<Config>('trigger_sync')
        .then((syncedConfig) => {
          logger.info('[ConfigProvider] Startup sync completed');
          setConfig(syncedConfig);
        })
        .catch((err) => {
          logger.warn('[ConfigProvider] Startup sync failed', err);
        });
    }
  }, []);

  const saveConfig = useCallback(async (newConfig: Config) => {
    try {
      logger.info('[ConfigProvider] Saving config...');
      await invoke('save_config', { config: newConfig });
      logger.info('[ConfigProvider] Config saved successfully');

      setConfig(newConfig);
      setError(null);
    } catch (err) {
      logger.error('[ConfigProvider] Failed to save config', err);
      setError(err instanceof Error ? err.message : String(err));
      throw err;
    }
  }, []);

  const triggerSync = useCallback(async () => {
    try {
      logger.info('[ConfigProvider] Triggering sync...');
      const cfg = await invoke<Config>('trigger_sync');
      logger.info('[ConfigProvider] Sync successful');
      setConfig(cfg);
      return cfg;
    } catch (err) {
      logger.error('[ConfigProvider] Sync failed', err);
      throw err;
    }
  }, []);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    
    listen<Config>('config-updated', (event) => {
      logger.info('[ConfigProvider] Config updated from background sync');
      setConfig(event.payload);
    }).then(fn => { unlisten = fn; });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  return (
    <ConfigContext.Provider value={{ config, loading, error, loadConfig, saveConfig, triggerSync }}>
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
