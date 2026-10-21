/** Serialized game account returned by the auth plugin. */
export interface MinecraftAccount {
    account_id: string
    account_type: 'microsoft' | 'offline' | 'yggdrasil'
    profile: { id: string; name: string; skins: unknown[]; capes: unknown[] }
    active: boolean
    yggdrasil?: { api_root: string; server_name: string; login: string } | null
}
