export interface Server {
  id: string
  name: string
  group?: string | null
  host: string
  port: number
  username: string
  authId: string | null
  proxyId: string | null
  jumphostId: string | null
  portForwards: PortForward[]
  keepAlive: number
  autoExecCommands: string[]
  snippets?: import("./snippet").Snippet[]
  sftpFavoritePaths?: string[]
  additionalPrompt?: string | null
  synced: boolean
  updatedAt: string
}

export interface PortForward {
  local: number
  remote: number
}
