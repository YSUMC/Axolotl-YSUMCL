<script setup lang="ts">
import { Admonition, defineMessages, useVIntl } from '@modrinth/ui'
import { renderString } from '@modrinth/utils'
import { openUrl } from '@tauri-apps/plugin-opener'
import { computed } from 'vue'

import type { LauncherRelease } from '@/helpers/launcher-releases'
import { safeAnnouncementUrl } from '@/helpers/remote-announcements'

const props = withDefaults(
    defineProps<{
        release?: LauncherRelease
        version?: string | null
        showHeader?: boolean
        loading?: boolean
        failed?: boolean
    }>(),
    { release: undefined, version: null, showHeader: true, loading: false, failed: false },
)

const { formatMessage } = useVIntl()
const messages = defineMessages({
    title: { id: 'app.ysumc-releases.title', defaultMessage: 'YSUMC Launcher {version}' },
    empty: {
        id: 'app.ysumc-releases.empty-notes',
        defaultMessage: 'No release notes have been published for this version.',
    },
    loading: { id: 'app.ysumc-releases.loading', defaultMessage: 'Loading release notes…' },
    failed: {
        id: 'app.ysumc-releases.failed',
        defaultMessage: 'Release notes could not be loaded. Please try again.',
    },
})
const html = computed(() => renderString(props.release?.notes ?? ''))
const dateLabel = computed(() =>
    props.release ? new Date(props.release.published_at).toLocaleDateString() : '',
)
async function openReleaseLink(event: MouseEvent) {
    const anchor = (event.target as Element | null)?.closest('a')
    if (!anchor) return
    event.preventDefault()
    const url = safeAnnouncementUrl(anchor.getAttribute('href'))
    if (url) await openUrl(url)
}
</script>

<template>
    <div class="flex min-w-0 flex-col gap-4 text-[var(--color-text-default)]">
        <header v-if="showHeader" class="flex min-w-0 flex-col gap-2">
            <h2 class="m-0 break-words text-xl font-semibold text-[var(--color-text-primary)]">
                {{ formatMessage(messages.title, { version: release?.version ?? version ?? '—' }) }}
            </h2>
            <time
                v-if="release"
                :datetime="release.published_at"
                class="text-sm text-[var(--color-text-tertiary)]"
                >{{ dateLabel }}</time
            >
        </header>
        <div
            v-if="release?.notes?.trim()"
            class="markdown-body min-w-0 break-words"
            @click="openReleaseLink"
            v-html="html"
        />
        <p v-else-if="loading" class="m-0 text-sm text-[var(--color-text-tertiary)]" role="status">
            {{ formatMessage(messages.loading) }}
        </p>
        <Admonition
            v-else
            type="info"
            :body="formatMessage(failed ? messages.failed : messages.empty)"
        />
    </div>
</template>
