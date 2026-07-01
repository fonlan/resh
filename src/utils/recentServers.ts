import { Server } from "../types"

/**
 * Get the most recent servers from the list
 * Returns up to `limit` servers, falling back to the first servers if no recent list exists
 */
export function getRecentServers(
  recentServerIds: string[] | undefined,
  allServers: Server[],
  limit: number = 3,
): Server[] {
  if (!recentServerIds || recentServerIds.length === 0) {
    return allServers.slice(0, limit)
  }
  return recentServerIds
    .map((id) => allServers.find((s) => s.id === id))
    .filter((s): s is Server => s !== undefined)
    .slice(0, limit)
}
