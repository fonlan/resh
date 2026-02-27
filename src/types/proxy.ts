export interface ProxyConfig {
  id: string
  name: string
  type: "http" | "socks5"
  host: string
  port: number
  username?: string
  password?: string
  ignoreSslErrors: boolean
  synced: boolean
  updatedAt: string
}
