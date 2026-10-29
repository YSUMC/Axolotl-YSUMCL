<script setup lang="ts">
import {
    Button,
    commonMessages,
    defineMessages,
    NewModal,
    useModalStack,
    useVIntl,
} from '@modrinth/ui'
import { renderString } from '@modrinth/utils'
import { getVersion } from '@tauri-apps/api/app'
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'
import { openUrl } from '@tauri-apps/plugin-opener'
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue'

import { isMutmc, launcherBrand, launcherServices } from '@/branding'
import { launcherSiteDocument } from '@/helpers/launcher-site'
import {
    announcementKey,
    isAnnouncementActive,
    OPEN_REMOTE_ANNOUNCEMENT_CENTER_EVENT,
    parseAnnouncements,
    REMOTE_ANNOUNCEMENTS_UPDATED_EVENT,
    type RemoteAnnouncement,
    safeAnnouncementUrl,
} from '@/helpers/remote-announcements'
import { getUpdateChannel } from '@/helpers/settings'
import { ysumcAccount } from '@/helpers/ysumc-session'

const props = defineProps<{ ready: boolean; previewOnly?: boolean }>()
const { formatMessage } = useVIntl()
const { hasModal } = useModalStack()
const messages = defineMessages({
    view: { id: 'app.remote-announcements.view', defaultMessage: 'View announcement' },
    unread: { id: 'app.remote-announcements.unread', defaultMessage: 'Unread' },
    readAll: {
        id: 'app.remote-announcements.read-all',
        defaultMessage: 'Mark all announcements as read',
    },
    centerTitle: { id: 'app.remote-announcements.center-title', defaultMessage: 'Announcements' },
    empty: { id: 'app.remote-announcements.empty', defaultMessage: 'No announcements' },
    loading: { id: 'app.remote-announcements.loading', defaultMessage: 'Loading announcements…' },
    failed: {
        id: 'app.remote-announcements.failed',
        defaultMessage: 'Could not refresh announcements. Please try again.',
    },
    refresh: { id: 'app.remote-announcements.refresh', defaultMessage: 'Refresh announcements' },
    previewTitle: {
        id: 'app.remote-announcements.preview-title',
        defaultMessage: 'Announcement style preview',
    },
    previewSummary: {
        id: 'app.remote-announcements.preview-summary',
        defaultMessage:
            'This is a local preview. Select View announcement to preview the full Markdown content.',
    },
    previewContent: {
        id: 'app.remote-announcements.preview-content',
        defaultMessage:
            '## Announcement preview\n\nThis is **sample content**, not a published announcement.\n\n- Supports headings, lists, and links\n- Close buttons are always available\n\n> Previewing does not change real announcement read status.\n\n| Type | Display |\n| --- | --- |\n| Modal | Full Markdown content |\n| Popup | Summary, then full content |\n\n[Visit the website](https://axlmc.org)',
    },
    previewAction: {
        id: 'app.remote-announcements.preview-action',
        defaultMessage: 'Visit website',
    },
})
const modal = ref<InstanceType<typeof NewModal>>()
const selected = ref<RemoteAnnouncement | null>(null)
const active = ref(false)
const centerOpen = ref(false)
const startupNotice = ref<RemoteAnnouncement | null>(null)
const html = computed(() => renderString(selected.value?.content ?? ''))
const baseStateKey = `${launcherBrand.id}-remote-announcements-v2`
let stateKey = `${baseStateKey}:${ysumcAccount.value?.account_id ?? 'guest'}`
let audienceGeneration = 0
const read = reactive(new Set<string>())
const queuedThisSession = new Set<string>()
const startupNotified = new Set<string>()
const items = ref<RemoteAnnouncement[]>([])
const loading = ref(false)
const failed = ref(false)
let pending: RemoteAnnouncement[] = []
let cacheKey = ''
let endpoint: URL | undefined
let cacheLoaded = false
let inFlight = false
let disposed = false
let controller: AbortController | undefined
let interval: ReturnType<typeof setInterval> | undefined
let refreshInterval: ReturnType<typeof setInterval> | undefined
let lastRequestAt = 0
let advanceTimer: ReturnType<typeof setTimeout> | undefined

function loadReadState() {
    read.clear()
    if (props.previewOnly) return
    try {
        const saved = JSON.parse(localStorage.getItem(stateKey) ?? 'null')
        if (saved && Array.isArray(saved.read))
            for (const key of saved.read) if (typeof key === 'string') read.add(key)
    } catch {
        // Ignore malformed local read-state
    }
}

function persist() {
    if (props.previewOnly) return
    try {
        localStorage.setItem(
            stateKey,
            JSON.stringify({
                read: [...read].slice(-1000),
            }),
        )
    } catch {
        // localStorage may be unavailable (private mode / quota)
    }
}
function emitState() {
    window.dispatchEvent(
        new CustomEvent(REMOTE_ANNOUNCEMENTS_UPDATED_EVENT, {
            detail: {
                items: items.value,
                unreadKeys: items.value
                    .filter((item) => !read.has(announcementKey(item)))
                    .map(announcementKey),
            },
        }),
    )
}
function openCenter(item?: RemoteAnnouncement) {
    if (!props.ready || disposed) return
    centerOpen.value = true
    if (item) {
        startupNotice.value = null
        selectAnnouncement(item)
    } else if (!selected.value && items.value.length) {
        selectAnnouncement(items.value[0])
    }
    void nextTick(() => modal.value?.show())
}
function handleOpenCenter() {
    if (props.previewOnly) return
    openCenter()
    void refresh()
}
async function show(item: RemoteAnnouncement) {
    if (
        disposed ||
        !props.ready ||
        !isAnnouncementActive(item) ||
        (hasModal.value && !active.value && !centerOpen.value)
    )
        return
    centerOpen.value = true
    active.value = true
    selectAnnouncement(item)
    await nextTick()
    if (!disposed) modal.value?.show()
}
function selectAnnouncement(item: RemoteAnnouncement) {
    selected.value = item
    const key = announcementKey(item)
    read.add(key)
    pending = pending.filter((entry) => announcementKey(entry) !== key)
    persist()
    emitState()
}
function markAllRead() {
    for (const item of items.value) {
        read.add(announcementKey(item))
    }
    pending = []
    persist()
    emitState()
}
function advance() {
    if (disposed || !props.ready || hasModal.value || active.value) return
    const next = pending.shift()
    if (next) {
        void show(next)
        return
    }
    for (const item of items.value) {
        const key = announcementKey(item)
        if (
            item.type === 'notification' &&
            !startupNotified.has(key) &&
            !read.has(key) &&
            isAnnouncementActive(item)
        ) {
            startupNotice.value = item
            startupNotified.add(key)
            setTimeout(() => {
                if (startupNotice.value === item) {
                    startupNotice.value = null
                    advance()
                }
            }, 8000)
            break
        }
    }
}
function closed() {
    active.value = false
    centerOpen.value = false
    if (advanceTimer) clearTimeout(advanceTimer)
    advanceTimer = setTimeout(advance, 350)
}
function sync(next: RemoteAnnouncement[], fresh: boolean) {
    items.value = next.filter((item) => isAnnouncementActive(item))
    const keys = new Set(items.value.map(announcementKey))
    pending = pending.filter((item) => keys.has(announcementKey(item)))
    if (selected.value) {
        const selectedKey = announcementKey(selected.value)
        selected.value = items.value.find((item) => announcementKey(item) === selectedKey) ?? null
    }
    if (startupNotice.value && !keys.has(announcementKey(startupNotice.value)))
        startupNotice.value = null
    if (centerOpen.value && !selected.value && items.value.length)
        selectAnnouncement(items.value[0])
    emitState()
    if (fresh) {
        for (const item of items.value) {
            const key = announcementKey(item)
            if (item.type === 'modal' && !queuedThisSession.has(key) && !read.has(key)) {
                queuedThisSession.add(key)
                pending.push(item)
            }
        }
        advance()
    }
}
function loadCache() {
    if (!isMutmc || cacheLoaded || !cacheKey) return
    cacheLoaded = true
    try {
        const cached = JSON.parse(localStorage.getItem(cacheKey) ?? 'null')
        if (
            cached &&
            typeof cached.savedAt === 'number' &&
            Date.now() - cached.savedAt < 86400000
        ) {
            const parsed = parseAnnouncements(cached.items)
            if (parsed) sync(parsed, false)
        }
    } catch {
        // Ignore malformed or expired cache payloads
    }
}
async function refresh() {
    if (inFlight || disposed || props.previewOnly) return
    const generation = audienceGeneration
    inFlight = true
    loading.value = true
    failed.value = false
    lastRequestAt = Date.now()
    const abort = new AbortController()
    controller = abort
    const timeout = setTimeout(() => abort.abort(), 10000)
    try {
        if (!endpoint) {
            const [version, channel] = await Promise.all([getVersion(), getUpdateChannel()])
            endpoint = new URL(
                import.meta.env.VITE_AXO_ANNOUNCEMENTS_URL || launcherServices.announcementsUrl,
            )
            endpoint.searchParams.set('version', version)
            endpoint.searchParams.set('channel', channel === 'release' ? 'stable' : 'beta')
            cacheKey = stateKey + ':cache:' + endpoint.href
        }
        if (disposed || abort.signal.aborted) return
        loadCache()
        let result: unknown
        if (isMutmc) {
            const response = await tauriFetch(endpoint.href, { signal: abort.signal })
            if (!response.ok) throw new Error(`Announcements request failed: ${response.status}`)
            const text = await response.text()
            if (text.length > 4500000) throw new Error('Announcements response is too large')
            result = JSON.parse(text)
        } else {
            result = await launcherSiteDocument(
                'announcements',
                endpoint.searchParams.get('version') ?? undefined,
                endpoint.searchParams.get('channel') ?? undefined,
            )
        }
        if (generation !== audienceGeneration) return
        const entries = Array.isArray(result)
            ? result
            : result && typeof result === 'object' && 'announcements' in result
              ? result.announcements
              : null
        const parsed = parseAnnouncements(entries)
        if (!parsed) throw new Error('Invalid announcements response')
        if (disposed) return
        sync(parsed, true)
        try {
            if (isMutmc)
                localStorage.setItem(
                    cacheKey,
                    JSON.stringify({ savedAt: Date.now(), items: parsed }),
                )
        } catch {
            // Ignore cache write failures
        }
    } catch (error) {
        if (!disposed && generation === audienceGeneration) {
            if (!isMutmc) sync([], false)
            failed.value = true
            console.warn('Failed to refresh launcher announcements', error)
        }
    } finally {
        clearTimeout(timeout)
        if (generation === audienceGeneration) {
            inFlight = false
            loading.value = false
            controller = undefined
        }
    }
}
function handleFocus() {
    if (Date.now() - lastRequestAt >= 30000) void refresh()
}
function handleOnline() {
    void refresh()
}
async function openLink(value: unknown) {
    const url = safeAnnouncementUrl(value)
    if (!url) return
    try {
        await openUrl(url)
    } catch {
        // Opener may reject unknown schemes
    }
}
function contentClick(event: MouseEvent) {
    const link = event.target instanceof Element ? event.target.closest('a') : null
    if (!link) return
    event.preventDefault()
    event.stopPropagation()
    void openLink(link.getAttribute('href'))
}
function preview(type: RemoteAnnouncement['type'], withAction = false) {
    if (!props.previewOnly || !props.ready || hasModal.value || disposed) return
    const now = new Date().toISOString()
    const item: RemoteAnnouncement = {
        id: 'local-preview-' + type,
        title: formatMessage(messages.previewTitle),
        summary: formatMessage(messages.previewSummary),
        content: formatMessage(messages.previewContent),
        type,
        priority: 'normal',
        starts_at: now,
        ends_at: null,
        published_at: now,
        action_label: withAction ? formatMessage(messages.previewAction) : null,
        action_url: withAction ? 'https://axlmc.org' : null,
    }
    read.clear()
    queuedThisSession.clear()
    sync([item], true)
}
defineExpose({ preview })
onMounted(() => {
    if (props.previewOnly) return
    loadReadState()
    window.addEventListener(OPEN_REMOTE_ANNOUNCEMENT_CENTER_EVENT, handleOpenCenter)
    window.addEventListener('focus', handleFocus)
    window.addEventListener('online', handleOnline)
    const start = () => {
        if (disposed) return
        void refresh()
        interval = setInterval(() => {
            sync(items.value, false)
            advance()
        }, 15000)
        refreshInterval = setInterval(() => void refresh(), 60000)
    }
    // Defer the startup request until after first paint.
    if (typeof requestIdleCallback === 'function') {
        requestIdleCallback(start, { timeout: 2000 })
    } else {
        setTimeout(start, 500)
    }
})
watch([() => props.ready, hasModal], () => {
    if (advanceTimer) clearTimeout(advanceTimer)
    advanceTimer = setTimeout(advance, 350)
})
watch(
    () => ysumcAccount.value?.account_id,
    () => {
        if (isMutmc || props.previewOnly) return
        audienceGeneration++
        controller?.abort()
        inFlight = false
        stateKey = `${baseStateKey}:${ysumcAccount.value?.account_id ?? 'guest'}`
        loadReadState()
        queuedThisSession.clear()
        startupNotified.clear()
        pending = []
        sync([], false)
        void refresh()
    },
    { flush: 'sync' },
)
onUnmounted(() => {
    disposed = true
    controller?.abort()
    if (interval) clearInterval(interval)
    if (refreshInterval) clearInterval(refreshInterval)
    if (advanceTimer) clearTimeout(advanceTimer)
    window.removeEventListener(OPEN_REMOTE_ANNOUNCEMENT_CENTER_EVENT, handleOpenCenter)
    window.removeEventListener('focus', handleFocus)
    window.removeEventListener('online', handleOnline)
})
</script>

<template>
    <div
        v-if="startupNotice"
        class="fixed right-4 top-16 z-50 w-[22rem] max-w-[calc(100vw-2rem)] rounded-xl border border-surface-5 bg-surface-3 p-3 shadow-lg"
    >
        <button class="w-full text-left" @click="startupNotice && openCenter(startupNotice)">
            <div class="mb-1 text-xs font-semibold uppercase text-brand">
                {{ formatMessage(messages.unread) }}
            </div>
            <div class="font-semibold text-[var(--color-text-primary)]">
                {{ startupNotice.title }}
            </div>
            <div
                v-if="startupNotice.summary"
                class="mt-1 line-clamp-2 text-sm text-[var(--color-text-tertiary)]"
            >
                {{ startupNotice.summary }}
            </div>
        </button>
    </div>
    <NewModal
        ref="modal"
        :header="formatMessage(messages.centerTitle)"
        :on-hide="closed"
        max-width="80vw"
        width="80vw"
        max-content-height="calc(80vh - 8rem)"
        scrollable
    >
        <div class="flex h-[calc(80vh-8rem)] min-h-0 gap-3">
            <aside
                class="flex min-h-0 w-[32%] min-w-[14rem] flex-col border-r border-surface-5 pr-3"
            >
                <div class="mb-2 flex items-center justify-between">
                    <span class="font-semibold text-[var(--color-text-primary)]">{{
                        formatMessage(messages.centerTitle)
                    }}</span>
                    <Button
                        v-if="items.some((item) => !read.has(announcementKey(item)))"
                        @click="markAllRead"
                        >{{ formatMessage(messages.readAll) }}</Button
                    >
                </div>
                <div
                    v-if="!items.length"
                    class="py-8 text-center text-sm text-[var(--color-text-tertiary)]"
                >
                    {{
                        formatMessage(
                            loading ? messages.loading : failed ? messages.failed : messages.empty,
                        )
                    }}
                </div>
                <div v-else class="flex min-h-0 flex-col gap-1 overflow-auto">
                    <button
                        v-for="item in items"
                        :key="announcementKey(item)"
                        class="flex items-start gap-2 rounded-lg p-2 text-left hover:bg-surface-4"
                        :class="{
                            'bg-surface-4':
                                selected && announcementKey(selected) === announcementKey(item),
                        }"
                        @click="selectAnnouncement(item)"
                    >
                        <span
                            class="mt-1.5 size-2 shrink-0 rounded-full"
                            :class="read.has(announcementKey(item)) ? 'opacity-0' : 'bg-red'"
                        />
                        <span class="min-w-0 flex-1">
                            <span
                                class="block truncate font-medium text-[var(--color-text-primary)]"
                                >{{ item.title }}</span
                            >
                            <span
                                class="block line-clamp-2 text-xs text-[var(--color-text-tertiary)]"
                                >{{ item.summary || item.content }}</span
                            >
                        </span>
                    </button>
                </div>
            </aside>
            <section v-if="selected" class="min-h-0 min-w-0 flex-1 overflow-auto px-1">
                <h2 class="mb-4 text-xl font-semibold text-[var(--color-text-primary)]">
                    {{ selected.title }}
                </h2>
                <div
                    class="markdown-body break-words"
                    @click="contentClick"
                    @auxclick="contentClick"
                    v-html="html"
                />
                <div class="mt-4 flex justify-end">
                    <Button
                        v-if="selected.action_url && selected.action_label"
                        type="colored"
                        color="brand"
                        @click="openLink(selected.action_url)"
                        >{{ selected.action_label }}</Button
                    >
                </div>
            </section>
            <div
                v-else
                class="flex flex-1 items-center justify-center text-sm text-[var(--color-text-tertiary)]"
            >
                {{
                    formatMessage(
                        loading ? messages.loading : failed ? messages.failed : messages.empty,
                    )
                }}
            </div>
        </div>
        <template #actions>
            <div class="flex flex-wrap justify-end gap-2">
                <span
                    v-if="failed && items.length"
                    role="status"
                    class="mr-auto self-center text-sm text-[var(--color-text-tertiary)]"
                    >{{ formatMessage(messages.failed) }}</span
                >
                <Button v-if="!previewOnly" :disabled="loading" @click="refresh">{{
                    formatMessage(messages.refresh)
                }}</Button>
                <Button @click="modal?.hide()"
                    >{{ formatMessage(commonMessages.closeButton) }}
                </Button>
            </div>
        </template>
    </NewModal>
</template>
