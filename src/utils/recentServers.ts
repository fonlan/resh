import { Server, GeneralSettings } from '../types/config';

export const MAX_RECENT_SERVERS = 10;

/**
 * Add a server to the recent servers list
 * Moves the server to the front if it already exists
 * Limits the list to MAX_RECENT_SERVERS items
 */
export function addRecentServer(general: GeneralSettings, serverId: string): GeneralSettings {
  const current = general.recentServerIds || [];
  const filtered = current.filter(id => id !== serverId);
  const updated = [serverId, ...filtered].slice(0, MAX_RECENT_SERVERS);
  return { ...general, recentServerIds: updated };
}

/**
 * Get the most recent servers from the list
 * Returns up to `limit` servers, falling back to the first servers if no recent list exists
 */
export function getRecentServers(
  recentServerIds: string[] | undefined,
  allServers: Server[],
  limit: number = 3
): Server[] {
  if (!recentServerIds || recentServerIds.length === 0) {
    return allServers.slice(0, limit);
  }
  return recentServerIds
    .map(id => allServers.find(s => s.id === id))
    .filter((s): s is Server => s !== undefined)
    .slice(0, limit);
}
