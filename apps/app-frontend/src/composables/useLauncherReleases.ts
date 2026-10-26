import { fetch as tauriFetch } from '@tauri-apps/plugin-http'
import { ref, watch } from 'vue'

import { isMutmc, launcherBrand, launcherServices } from '@/branding'
import { type LauncherRelease, parseLauncherReleases } from '@/helpers/launcher-releases'
import { launcherSiteDocument } from '@/helpers/launcher-site'
import { ysumcAccount } from '@/helpers/ysumc-session'

const releases = ref<LauncherRelease[]>([])
const loading = ref(false)
const failed = ref(false)
const cacheKey = `${launcherBrand.id}-launcher-releases-v1`
let generation = 0
let initialized = false
let pending: Promise<void> | null = null

function loadCache() {
    if (initialized) return
    initialized = true
    try {
        if (!isMutmc) {
            localStorage.removeItem(cacheKey)
            return
        }
        releases.value =
            parseLauncherReleases(JSON.parse(localStorage.getItem(cacheKey) ?? 'null')) ?? []
    } catch {
        /* A missing or obsolete cache does not block online release notes. */
    }
}

function saveCache() {
    if (!isMutmc) return
    try {
        localStorage.setItem(cacheKey, JSON.stringify({ releases: releases.value }))
    } catch {
        /* Release notes remain usable if local storage is unavailable. */
    }
}

export function rememberLauncherRelease(release: LauncherRelease) {
    if (!parseLauncherReleases({ releases: [release] })) return
    loadCache()
    releases.value = [
        release,
        ...releases.value.filter((item) => item.version !== release.version),
    ].slice(0, 100)
    saveCache()
}

function refresh(): Promise<void> {
    const current = generation
    if (pending) return pending
    loading.value = true
    failed.value = false
    pending = (async () => {
        const controller = new AbortController()
        const timeout = setTimeout(() => controller.abort(), 10000)
        try {
            let document: unknown
            if (isMutmc) {
                const response = await tauriFetch(launcherServices.releasesUrl, {
                    signal: controller.signal,
                })
                if (!response.ok)
                    throw new Error(`Release notes request failed: ${response.status}`)
                document = await response.json()
            } else {
                document = await launcherSiteDocument('releases')
            }
            if (current !== generation) return
            const parsed = parseLauncherReleases(document)
            if (!parsed) throw new Error('Invalid launcher release notes')
            // A successful response replaces the cache, including removed or disabled releases.
            releases.value = parsed
            saveCache()
        } catch (error) {
            if (current !== generation) return
            if (!isMutmc) releases.value = []
            failed.value = true
            console.warn('Failed to load YSUMC release notes', error)
        } finally {
            clearTimeout(timeout)
            if (current === generation) {
                loading.value = false
                pending = null
            }
        }
    })()
    return pending
}

watch(
    () => ysumcAccount.value?.account_id,
    () => {
        if (isMutmc) return
        generation++
        releases.value = []
        pending = null
        void refresh()
    },
    { flush: 'sync' },
)

export function useLauncherReleases() {
    loadCache()
    return { releases, loading, failed, refresh }
}
