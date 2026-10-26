<script setup lang="ts">
import { HistoryIcon, RefreshCwIcon } from '@modrinth/assets'
import { Accordion, Button, defineMessages, useVIntl } from '@modrinth/ui'
import { computed, onMounted } from 'vue'

import { useLauncherReleases } from '@/composables/useLauncherReleases'

import UpdateAnnouncementContent from './UpdateAnnouncementContent.vue'

const props = defineProps<{ currentVersion: string }>()
const { formatMessage } = useVIntl()
const { releases, loading, failed, refresh } = useLauncherReleases()
const messages = defineMessages({
    title: {
        id: 'app.settings.updates.announcements.title',
        defaultMessage: 'Update announcements',
    },
    description: {
        id: 'app.settings.updates.announcements.description',
        defaultMessage: 'See what changed in this version and browse previous releases.',
    },
    history: {
        id: 'app.settings.updates.announcements.history',
        defaultMessage: 'Version history',
    },
    empty: {
        id: 'app.ysumc-releases.empty-history',
        defaultMessage: 'No other releases have been published.',
    },
    refresh: { id: 'app.ysumc-releases.refresh', defaultMessage: 'Refresh release notes' },
    failed: {
        id: 'app.ysumc-releases.refresh-failed',
        defaultMessage: 'Refresh failed. Saved release notes are shown when available.',
    },
    release: { id: 'app.ysumc-releases.title', defaultMessage: 'YSUMC Launcher {version}' },
})
const currentRelease = computed(() =>
    releases.value.find((item) => item.version === props.currentVersion),
)
const history = computed(() =>
    releases.value.filter((item) => item.version !== props.currentVersion),
)
onMounted(() => void refresh())
</script>

<template>
    <section
        class="flex min-w-0 flex-col gap-6 rounded-[var(--radius-md)] border border-surface-4 bg-surface-2 p-6"
    >
        <div class="flex flex-wrap items-start justify-between gap-3">
            <div class="flex min-w-0 flex-col gap-1">
                <h2 class="m-0 text-lg font-semibold text-[var(--color-text-primary)]">
                    {{ formatMessage(messages.title) }}
                </h2>
                <p class="m-0 text-[var(--color-text-tertiary)]">
                    {{ formatMessage(messages.description) }}
                </p>
            </div>
            <Button type="outlined" :disabled="loading" @click="refresh"
                ><RefreshCwIcon :class="{ 'animate-spin': loading }" />{{
                    formatMessage(messages.refresh)
                }}</Button
            >
        </div>
        <p v-if="failed" role="status" class="m-0 text-sm text-[var(--color-text-tertiary)]">
            {{ formatMessage(messages.failed) }}
        </p>
        <UpdateAnnouncementContent
            :release="currentRelease"
            :version="currentVersion"
            :loading="loading"
            :failed="failed"
        />
        <div class="flex min-w-0 flex-col gap-3">
            <h3
                class="m-0 flex items-center gap-2 text-base font-semibold text-[var(--color-text-primary)]"
            >
                <HistoryIcon class="size-4" />{{ formatMessage(messages.history) }}
            </h3>
            <p
                v-if="!loading && !failed && !history.length"
                class="m-0 text-sm text-[var(--color-text-tertiary)]"
            >
                {{ formatMessage(messages.empty) }}
            </p>
            <Accordion
                v-for="release in history"
                :key="release.version"
                class="min-w-0 overflow-hidden rounded-[var(--radius-sm)] border border-surface-4 bg-surface-3"
                button-class="flex w-full cursor-pointer items-center gap-3 border-0 bg-transparent px-4 py-3 text-left"
            >
                <template #title>
                    <div class="flex min-w-0 flex-1 flex-col gap-1">
                        <strong class="text-[var(--color-text-primary)]">{{
                            formatMessage(messages.release, { version: release.version })
                        }}</strong>
                        <time
                            :datetime="release.published_at"
                            class="text-sm text-[var(--color-text-tertiary)]"
                            >{{ new Date(release.published_at).toLocaleDateString() }}</time
                        >
                    </div>
                </template>
                <div class="border-t border-divider p-4">
                    <UpdateAnnouncementContent :release="release" :show-header="false" />
                </div>
            </Accordion>
        </div>
    </section>
</template>
