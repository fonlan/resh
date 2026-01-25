import { invoke } from '@tauri-apps/api/tauri';
import { useEffect, useState } from 'react';
import { Config } from '../types/config';

export const useConfig = () => {
  const [config, setConfig] = useState<Config | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadConfig = async () => {
    try {
      setLoading(true);
      const merged = await invoke<Config>('get_merged_config');
      setConfig(merged);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  const saveConfig = async (syncPart: Config, localPart: Config) => {
    try {
      await invoke('save_config', { syncPart, localPart });
      await loadConfig();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      throw err;
    }
  };

  useEffect(() => {
    loadConfig();
  }, []);

  return { config, loading, error, loadConfig, saveConfig };
};
