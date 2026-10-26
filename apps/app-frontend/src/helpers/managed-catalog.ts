import type { ManagedInstanceAction, ManagedInstanceSpec, ManagedServerSpec } from './managed'

export function groupManagedActions(actions: ManagedInstanceAction[], peers: boolean) {
    const groups = new Map<
        string,
        {
            id: string
            name: string
            description: string
            domain: string
            actions: ManagedInstanceAction[]
        }
    >()
    for (const action of actions) {
        const source = action.spec.source
        if (!!source !== peers) continue
        const id = source ? `${source.siteId}/${source.domain}` : 'ysumc'
        let group = groups.get(id)
        if (!group) {
            group = {
                id,
                name: source?.name?.trim() || source?.domain || 'YSUMC',
                description: source?.description?.trim() || '',
                domain: source?.domain || '',
                actions: [],
            }
            groups.set(id, group)
        }
        group.actions.push(action)
    }
    return [...groups.values()]
}

export function managedServerAddress(server: ManagedServerSpec | null): string | null {
    if (!server) return null
    const address =
        server.address.includes(':') && !server.address.startsWith('[')
            ? `[${server.address}]`
            : server.address
    // An explicit port (including 25565) must not trigger SRV discovery.
    return server.port ? `${address}:${server.port}` : address
}

export function canInstallManagedSpec(spec: ManagedInstanceSpec): boolean {
    return spec.pack.kind === 'mrpack' || spec.pack.kind === 'server'
}
