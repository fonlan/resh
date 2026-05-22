import type { FileEntry } from "./types"

export const SFTP_PATH_MIME_TYPE = "application/x-resh-sftp-path"
export const SFTP_ENTRY_MIME_TYPE = "application/x-resh-sftp-entry"
export const COPY_DATA_UNSUPPORTED_ERROR = "SFTP_COPY_DATA_UNSUPPORTED"
export const TREE_RENDER_BATCH_SIZE = 250
export const SFTP_LISTING_PAGE_SIZE = 400
export const CONTEXT_SUBMENU_WIDTH = 220
export const CONTEXT_MENU_VIEWPORT_PADDING = 8
export const CONTEXT_SUBMENU_GAP = 4

export const normalizeSftpPath = (path: string): string =>
  path.replace(/\\/g, "/")

export const normalizeRemotePath = (path: string): string => {
  const normalized = path.replace(/\\/g, "/").replace(/\/+/g, "/")
  if (normalized === "") return "/"
  if (normalized.length > 1 && normalized.endsWith("/")) {
    return normalized.slice(0, -1)
  }
  return normalized
}

export const getParentPath = (path: string): string => {
  const normalized = normalizeRemotePath(path)
  if (normalized === "/" || normalized === ".") return "/"
  const lastSlash = normalized.lastIndexOf("/")
  return lastSlash <= 0 ? "/" : normalized.substring(0, lastSlash)
}

export const joinRemotePath = (parentPath: string, itemName: string): string => {
  const normalizedParentPath = normalizeRemotePath(parentPath)
  const normalizedItemName = itemName.replace(/^\/+/, "").replace(/\/+$/, "")
  if (normalizedParentPath === "/") {
    return `/${normalizedItemName}`
  }
  return `${normalizedParentPath}/${normalizedItemName}`
}

export const getPathAncestors = (path: string): string[] => {
  const normalizedPath = normalizeRemotePath(path)
  if (normalizedPath === "/" || normalizedPath === ".") {
    return []
  }

  const parts = normalizedPath.split("/").filter(Boolean)
  const ancestors: string[] = []
  let current = ""
  parts.forEach((part) => {
    current = `${current}/${part}`
    ancestors.push(current)
  })

  return ancestors
}

export const isDirectory = (entry: FileEntry): boolean => {
  return (
    entry.is_dir || Boolean(entry.is_symlink && entry.target_is_dir === true)
  )
}

export const formatPermissions = (entry: FileEntry): string => {
  const mode = entry.permissions
  if (mode === undefined) return ""

  let type = "-"
  if (entry.is_dir) type = "d"
  else if (entry.is_symlink) type = "l"

  const r = (m: number) => (m & 4 ? "r" : "-")
  const w = (m: number) => (m & 2 ? "w" : "-")
  const x = (m: number) => (m & 1 ? "x" : "-")
  const part = (m: number) => r(m) + w(m) + x(m)

  return type + part((mode >> 6) & 7) + part((mode >> 3) & 7) + part(mode & 7)
}

export const updateTreeNodeByPath = (
  nodes: FileEntry[],
  targetPath: string,
  updater: (node: FileEntry) => FileEntry,
): { nodes: FileEntry[]; found: boolean } => {
  const normalizedTargetPath = normalizeSftpPath(targetPath)

  const walk = (
    list: FileEntry[],
  ): { nodes: FileEntry[]; found: boolean } => {
    for (let index = 0; index < list.length; index += 1) {
      const node = list[index]
      if (normalizeSftpPath(node.path) === normalizedTargetPath) {
        const nextNode = updater(node)
        if (nextNode === node) {
          return { nodes: list, found: false }
        }
        const nextList = list.slice()
        nextList[index] = nextNode
        return { nodes: nextList, found: true }
      }

      if (node.children && node.children.length > 0) {
        const childResult = walk(node.children)
        if (childResult.found) {
          const nextList = list.slice()
          nextList[index] = { ...node, children: childResult.nodes }
          return { nodes: nextList, found: true }
        }
      }
    }

    return { nodes: list, found: false }
  }

  return walk(nodes)
}
