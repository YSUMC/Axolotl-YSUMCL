import { invoke } from '@tauri-apps/api/core'
import { ref } from 'vue'

import { installJobInstanceId, type InstallJobSnapshot, wait_for_install_job } from './install'

/**
 * Mirror of the server manifest documented in `docs/launcher-protocol.md`.
 *
 * The society backend decides which instances exist, at which `revision` and from
 * which pack file; the launcher only reconciles and installs.
 */
export interface ManagedLauncherPolicy {
    min_version: string | null
    latest_version: string | null
    force_update: boolean
}

export interface ManagedMinecraftSpec {
    game_version: string
    loader: string | null
    loader_version: string | null
}

export interface ManagedPackSpec {
    kind: string
    url: string
    sha1: string | null
    size: number | null
}

export interface ManagedServerSpec {
    address: string
    opened_on?: string | null
    port: number | null
}

export interface ManagedInstanceSpec {
    id: string
    name: string
    description: string | null
    icon_url: string | null
    sort: number
    revision: number
    minecraft: ManagedMinecraftSpec
    pack: ManagedPackSpec
    server: ManagedServerSpec | null
    required: boolean
}

export interface ManagedManifest {
    cloud_map_url?: string | null
    schema: number
    generated_at: string | null
    launcher: ManagedLauncherPolicy | null
    instances: ManagedInstanceSpec[]
}

export type ManagedInstanceActionKind = 'create' | 'update' | 'current' | 'retired'

export interface ManagedInstanceAction {
    spec: ManagedInstanceSpec
    instance_id: string | null
    applied_revision: number | null
    action: ManagedInstanceActionKind
}

export interface ManagedSyncReport {
    cloud_map_url?: string | null
    schema: number
    offline: boolean
    synced_at: number
    launcher: ManagedLauncherPolicy | null
    instances: ManagedInstanceAction[]
}

export interface ManagedInstanceRecord {
    server_instance_id: string
    instance_id: string
    revision: number
    target_revision: number
    name: string
    description: string
    icon_url: string
    sort: number
    required: boolean
    retired: boolean
    server_address: string
    server_port: number
    updated: number
}

/** Only pack format the launcher can install today. */
export const MANAGED_PACK_KIND = 'mrpack'

/**
 * Local instance ids the society server owns, from the last successful sync.
 *
 * This is a UI cache; launch eligibility is checked separately by the runtime.
 */
const managedInstanceIds = ref<Set<string>>(new Set())

/** Re-reads the managed instance ids. Safe to call before any sync. */
export async function refreshManagedInstanceIds(): Promise<Set<string>> {
    try {
        const records = await managed_list()
        managedInstanceIds.value = new Set(
            records.map((record) => record.instance_id).filter((id) => id.length > 0),
        )
    } catch {
        // Offline, or the launcher never synced: keep whatever was known last.
    }

    return managedInstanceIds.value
}

/** Whether a local instance is owned by the society server, per the last sync. */
export function isManagedInstance(instanceId: string | null | undefined): boolean {
    if (!instanceId) return false
    return managedInstanceIds.value.has(instanceId)
}

export async function managed_fetch_manifest(manifestUrl: string) {
    return await invoke<ManagedManifest>('plugin:instance|managed_fetch_manifest', { manifestUrl })
}

export async function managed_sync(manifestUrl: string) {
    return await invoke<ManagedSyncReport>('plugin:instance|managed_sync', { manifestUrl })
}

export async function managed_list() {
    return await invoke<ManagedInstanceRecord[]>('plugin:instance|managed_list')
}

export async function managed_mark_installed(
    serverInstanceId: string,
    instanceId: string,
    revision: number,
) {
    return await invoke<void>('plugin:instance|managed_mark_installed', {
        serverInstanceId,
        instanceId,
        revision,
    })
}

export async function managed_register_instance(
    serverInstanceId: string,
    instanceId: string,
    revision: number,
) {
    return await invoke<void>('plugin:instance|managed_register_instance', {
        serverInstanceId,
        instanceId,
        revision,
    })
}

/**
 * Refuses to launch an instance the server has taken out of service or moved to
 * a newer revision. Purely local, so it also holds when the network is down.
 */
export async function managed_ensure_runnable(instanceId: string) {
    return await invoke<void>('plugin:instance|managed_ensure_runnable', { instanceId })
}

/** Actions that require downloading or installing something. */
export function managedPendingActions(report: ManagedSyncReport): ManagedInstanceAction[] {
    return report.instances.filter(
        (action) => action.action === 'create' || action.action === 'update',
    )
}

/**
 * Packs recommended by the server. Installation still requires a player action.
 */
export function managedRequiredActions(report: ManagedSyncReport): ManagedInstanceAction[] {
    return managedPendingActions(report).filter((action) => action.spec.required)
}

/**
 * Pending actions the player starts themselves.
 *
 * An instance the server does not require is only downloaded once the player
 * asks for it, which keeps optional packs off the disk until they are wanted.
 */
export function managedOptionalActions(report: ManagedSyncReport): ManagedInstanceAction[] {
    return managedPendingActions(report).filter((action) => !action.spec.required)
}

/**
 * Installs or updates one managed instance.
 *
 * The pack travels into the install job as a URL plus its declared SHA-1 and
 * size, so the job's own downloader owns the transfer: a pack of hundreds of
 * megabytes is fetched with progress, several connections, a resumed partial
 * file and retries, instead of a separate blocking download that reports
 * nothing and has to succeed in one request. The install then runs through the
 * same pipeline as a manual install, so rollback behaves identically. Returns
 * the local instance id the action now refers to.
 */
export async function applyManagedInstanceAction(action: ManagedInstanceAction): Promise<string> {
    const job = await invoke<InstallJobSnapshot>('plugin:instance|managed_install', {
        serverInstanceId: action.spec.id,
    })
    const settled = await wait_for_install_job(job.job_id)
    const instanceId = installJobInstanceId(settled)
    if (!instanceId) throw new Error('官方整合包安装未返回实例，请在下载页重试。')
    await refreshManagedInstanceIds()
    return instanceId
}
