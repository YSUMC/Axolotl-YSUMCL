export const LAUNCHER_RELEASES_URL = 'https://skin.ysumc.net/api/launcher/update/releases'

export interface LauncherRelease {
    version: string
    notes: string | null
    published_at: string
}

export function parseLauncherReleases(value: unknown): LauncherRelease[] | null {
    if (!value || typeof value !== 'object' || !('releases' in value)) return null
    const rows = value.releases
    if (!Array.isArray(rows) || rows.length > 100) return null
    const versions = new Set<string>()
    const releases: LauncherRelease[] = []
    for (const row of rows) {
        if (
            !row ||
            typeof row !== 'object' ||
            typeof row.version !== 'string' ||
            row.version.length > 64 ||
            !/^\d+(\.\d+)*(-[0-9A-Za-z.-]+)?$/.test(row.version) ||
            versions.has(row.version) ||
            typeof row.published_at !== 'string' ||
            !Number.isFinite(Date.parse(row.published_at)) ||
            (row.notes !== null && (typeof row.notes !== 'string' || row.notes.length > 20000))
        )
            return null
        versions.add(row.version)
        releases.push({ version: row.version, notes: row.notes, published_at: row.published_at })
    }
    return releases
}

/** Manual checks always bypass the optional stable-release waiting period. */
export function shouldWaitForRelease(
    publishedAt: string | undefined,
    immediate: boolean,
    manual: boolean,
    force: boolean,
    now = Date.now(),
): boolean {
    if (immediate || manual || force) return false
    const published = Date.parse(publishedAt ?? '')
    return !Number.isFinite(published) || now < published + 24 * 60 * 60 * 1000
}
