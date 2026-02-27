export interface Authentication {
  id: string
  name: string
  type: "key" | "password"
  keyContent?: string
  passphrase?: string
  password?: string
  synced: boolean
  updatedAt: string
}
