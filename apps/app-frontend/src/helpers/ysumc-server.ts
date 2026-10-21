/** Opening dates use the club's calendar (Asia/Shanghai), including DST-free UTC+8. */
export function daysSinceOpening(date: string | null | undefined, now = Date.now()): number | null {
    if (!date || !/^\d{4}-\d{2}-\d{2}$/.test(date)) return null
    const start = Date.parse(`${date}T00:00:00Z`)
    if (!Number.isFinite(start) || new Date(start).toISOString().slice(0, 10) !== date) return null
    const today = Math.floor((now + 8 * 3600_000) / 86400_000)
    const opened = Math.floor(start / 86400_000)
    return today >= opened ? today - opened + 1 : null
}
