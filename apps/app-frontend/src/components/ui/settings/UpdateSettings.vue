<script setup lang="ts">
import { EyeIcon, RefreshCwIcon } from '@modrinth/assets'
import { Button, defineMessages, injectNotificationManager, Toggle, useVIntl } from '@modrinth/ui'
import { getVersion } from '@tauri-apps/api/app'
import { computed, inject, ref } from 'vue'

import UpdateAnnouncementHistory from '@/components/ui/announcement/UpdateAnnouncementHistory.vue'
import AppUpdateButton from '@/components/ui/app-update-button/index.vue'
import { useLauncherReleases } from '@/composables/useLauncherReleases'
import { getUpdatePreferences, setUpdatePreferences } from '@/helpers/settings'
import { isDev } from '@/helpers/utils'
import {
    type AppUpdateCheckResult,
    appUpdateState,
    checkForAppUpdate,
} from '@/providers/app-update'

import SettingsRow from './SettingsRow.vue'
import SettingsSection from './SettingsSection.vue'

const { formatMessage } = useVIntl()
const { handleError } = injectNotificationManager()
const [initialPreferences, currentVersion, isDevEnvironment] = await Promise.all([
    getUpdatePreferences(),
    getVersion(),
    isDev(),
])
const updatePreferences = ref(initialPreferences)
const checking = ref(false)
const savingPreferences = ref(false)
const checkResult = ref<AppUpdateCheckResult | 'failed' | null>(null)
const previewUpdateAnnouncement = inject<(version: string) => void>('previewUpdateAnnouncement')
const { releases, loading, failed, refresh } = useLauncherReleases()
const latestVersion = computed(() => releases.value[0]?.version)

const messages = defineMessages({
    preferencesTitle: {
        id: 'app.settings.updates.preferences.title',
        defaultMessage: 'Update behavior',
    },
    checkTitle: { id: 'app.settings.updates.check.title', defaultMessage: 'Check for updates' },
    currentVersion: {
        id: 'app.settings.updates.current-version',
        defaultMessage: 'Current version {version}',
    },
    latest: { id: 'app.ysumc-releases.latest', defaultMessage: 'Latest release {version}' },
    checkingLatest: {
        id: 'app.ysumc-releases.loading-latest',
        defaultMessage: 'Loading the latest release…',
    },
    latestUnavailable: {
        id: 'app.ysumc-releases.latest-unavailable',
        defaultMessage: 'The latest release is temporarily unavailable.',
    },
    noReleases: {
        id: 'app.ysumc-releases.no-releases',
        defaultMessage: 'No launcher releases have been published yet.',
    },
    check: { id: 'app.settings.updates.check', defaultMessage: 'Check for updates' },
    checking: { id: 'app.settings.updates.checking', defaultMessage: 'Checking for updates…' },
    available: { id: 'app.settings.updates.available', defaultMessage: 'An update is available.' },
    upToDate: {
        id: 'app.ysumc-releases.up-to-date',
        defaultMessage: 'YSUMC Launcher is up to date.',
    },
    disabled: {
        id: 'app.settings.updates.disabled',
        defaultMessage: 'Updates are disabled in this build.',
    },
    offline: {
        id: 'app.settings.updates.offline',
        defaultMessage: 'Connect to the internet to check for updates.',
    },
    failed: {
        id: 'app.settings.updates.failed',
        defaultMessage: 'Could not check for updates. Please try again.',
    },
    paused: { id: 'app.settings.updates.paused', defaultMessage: 'Updates are paused.' },
    security: {
        id: 'app.settings.updates.security',
        defaultMessage: 'Only updates with a valid cryptographic signature will be installed.',
    },
    preview: {
        id: 'app.settings.updates.announcements.preview',
        defaultMessage: 'Preview update announcement',
    },
    immediateFetch: {
        id: 'app.settings.updates.immediate-fetch',
        defaultMessage: 'Get updates as soon as they are available',
    },
    immediateDescription: {
        id: 'app.ysumc-releases.immediate-description',
        defaultMessage:
            'Receive new releases immediately. Turn this off to wait 24 hours; manual checks are always immediate.',
    },
    pause: { id: 'app.settings.updates.pause', defaultMessage: 'Pause updates' },
    pauseDescription: {
        id: 'app.settings.updates.pause-description',
        defaultMessage:
            'Stop automatic update checks, downloads, and update notifications until you resume updates.',
    },
})
const resultMessages: Record<AppUpdateCheckResult | 'failed', keyof typeof messages> = {
    available: 'available',
    'up-to-date': 'upToDate',
    disabled: 'disabled',
    offline: 'offline',
    failed: 'failed',
    paused: 'paused',
}
async function saveUpdatePreferences() {
    savingPreferences.value = true
    try {
        await setUpdatePreferences(updatePreferences.value)
        appUpdateState.updatesPaused.value = updatePreferences.value.updatesPaused
        checkResult.value = null
        if (!updatePreferences.value.updatesPaused && updatePreferences.value.immediateUpdateFetch)
            await checkForUpdates()
    } catch (error) {
        updatePreferences.value = await getUpdatePreferences()
        handleError(error)
    } finally {
        savingPreferences.value = false
    }
}
async function checkForUpdates() {
    checking.value = true
    checkResult.value = null
    try {
        const [result] = await Promise.all([checkForAppUpdate(), refresh()])
        checkResult.value = result
    } catch (error) {
        checkResult.value = 'failed'
        handleError(error)
    } finally {
        checking.value = false
    }
}
</script>

<template>
    <div class="flex min-w-0 flex-col gap-6">
        <SettingsSection :title="formatMessage(messages.checkTitle)">
            <div
                class="flex min-w-0 flex-col gap-4 rounded-[var(--radius-sm)] border border-surface-4 bg-surface-3 p-4"
            >
                <div class="flex flex-wrap justify-between gap-2">
                    <strong class="text-[var(--color-text-primary)]">{{
                        formatMessage(messages.currentVersion, { version: currentVersion })
                    }}</strong>
                    <span class="text-[var(--color-text-tertiary)]">{{
                        latestVersion
                            ? formatMessage(messages.latest, { version: latestVersion })
                            : formatMessage(
                                  loading
                                      ? messages.checkingLatest
                                      : failed
                                        ? messages.latestUnavailable
                                        : messages.noReleases,
                              )
                    }}</span>
                </div>
                <p class="m-0 text-sm text-[var(--color-text-tertiary)]">
                    {{ formatMessage(messages.security) }}
                </p>
                <div class="flex flex-wrap items-center gap-2">
                    <Button
                        type="colored"
                        color="brand"
                        :disabled="checking"
                        @click="checkForUpdates"
                    >
                        <RefreshCwIcon :class="{ 'animate-spin': checking }" />{{
                            formatMessage(checking ? messages.checking : messages.check)
                        }}
                    </Button>
                    <AppUpdateButton />
                    <Button
                        v-if="isDevEnvironment && previewUpdateAnnouncement"
                        type="outlined"
                        @click="previewUpdateAnnouncement(currentVersion)"
                    >
                        <EyeIcon />{{ formatMessage(messages.preview) }}
                    </Button>
                </div>
                <p
                    v-if="checkResult"
                    class="m-0 text-sm text-[var(--color-text-tertiary)]"
                    role="status"
                >
                    {{ formatMessage(messages[resultMessages[checkResult]]) }}
                </p>
            </div>
        </SettingsSection>
        <SettingsSection :title="formatMessage(messages.preferencesTitle)">
            <SettingsRow>
                <template #label>{{ formatMessage(messages.immediateFetch) }}</template>
                <template #description>{{ formatMessage(messages.immediateDescription) }}</template>
                <template #control
                    ><Toggle
                        id="immediate-update-fetch"
                        v-model="updatePreferences.immediateUpdateFetch"
                        :disabled="savingPreferences"
                        @update:model-value="saveUpdatePreferences"
                /></template>
            </SettingsRow>
            <SettingsRow>
                <template #label>{{ formatMessage(messages.pause) }}</template>
                <template #description>{{ formatMessage(messages.pauseDescription) }}</template>
                <template #control
                    ><Toggle
                        id="pause-updates"
                        v-model="updatePreferences.updatesPaused"
                        :disabled="savingPreferences"
                        @update:model-value="saveUpdatePreferences"
                /></template>
            </SettingsRow>
        </SettingsSection>
        <UpdateAnnouncementHistory :current-version="currentVersion" />
    </div>
</template>
