import { Server } from "../types"

export interface ServerGroup {
  id: string
  name: string
  servers: Server[]
  isDefault: boolean
}

const normalizeGroupName = (group?: string | null): string =>
  group?.trim() || ""

export const getServerGroupName = (
  server: Server,
  defaultGroupName: string,
): string => {
  const groupName = normalizeGroupName(server.group)
  return groupName || defaultGroupName
}

export const groupServersByName = (
  servers: Server[],
  defaultGroupName: string,
): ServerGroup[] => {
  const groups = new Map<string, ServerGroup>()

  servers.forEach((server) => {
    const groupName = normalizeGroupName(server.group)
    const groupId = groupName

    if (!groups.has(groupId)) {
      groups.set(groupId, {
        id: groupId,
        name: groupName || defaultGroupName,
        servers: [],
        isDefault: !groupName,
      })
    }

    groups.get(groupId)?.servers.push(server)
  })

  return Array.from(groups.values())
    .map((group) => ({
      ...group,
      servers: [...group.servers].sort((a, b) =>
        a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
      ),
    }))
    .sort((a, b) =>
      a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
    )
}
