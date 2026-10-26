import { invoke } from '@tauri-apps/api/core'

export interface LauncherIdentity {
    role: string
    role_label: string
    department: string | null
    department_label: string | null
    tags: string[]
    player: string
}

// A fixed native command owns credentials and the destination allowlist.
export function launcherSiteDocument<T = unknown>(
    document: 'identity' | 'announcements' | 'releases',
    version?: string,
    channel?: string,
) {
    return invoke<T>('plugin:auth|launcher_site_document', { document, version, channel })
}
