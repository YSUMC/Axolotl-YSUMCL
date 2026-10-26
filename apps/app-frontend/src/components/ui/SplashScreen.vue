<template>
    <Transition name="splash-fade" @after-leave="onAfterLeave">
        <div v-if="!doneLoading" class="fixed inset-0 z-[10000] bg-white text-[#3b3b3b]">
            <div
                class="absolute h-screen w-full flex flex-col justify-center items-center gap-4 z-[9998]"
                data-tauri-drag-region
            >
                <img
                    class="h-[min(12rem,35vh)] w-[min(12rem,35vw)] object-contain"
                    :src="isMutmc ? '/brands/mutmc/icon.webp' : '/ysumc/logo.svg'"
                    :alt="launcherBrand.shortName"
                />
                <ProgressBar
                    class="max-w-xs text-[#1f296a]"
                    :progress="Math.min(loadingProgress, 100)"
                    :max="100"
                />
                <span v-if="message">{{ message }}</span>
            </div>
        </div>
    </Transition>
</template>

<script setup>
import { defineMessages, injectLoadingState, ProgressBar, useVIntl } from '@modrinth/ui'
import { ref, watch } from 'vue'

import { isMutmc, launcherBrand } from '@/branding'
import { loading_listener } from '@/helpers/events.js'

const doneLoading = ref(false)
const loadingProgress = ref(0)
const message = ref()

const MIN_DISPLAY_MS = 500
const mountedAt = Date.now()

const loading = injectLoadingState()
const { formatMessage } = useVIntl()

const messages = defineMessages({
    updatingAppDirectory: {
        id: 'app.splash.updating-app-directory',
        defaultMessage: 'Updating app directory...',
    },
    checkingForUpdates: {
        id: 'app.splash.checking-for-updates',
        defaultMessage: 'Checking for updates...',
    },
})

function onAfterLeave() {
    loading.setEnabled(true)
}

watch(
    [loading.barEnabled, loading.pending],
    ([barEnabled, pending]) => {
        if (barEnabled) {
            return
        }

        if (pending) {
            loadingProgress.value = 0
            fakeLoadingIncrease()
            return
        }

        const elapsed = Date.now() - mountedAt
        const delay = Math.max(0, MIN_DISPLAY_MS - elapsed)

        setTimeout(() => {
            if (loading.pending.value) {
                return
            }
            doneLoading.value = true
        }, delay)
    },
    { immediate: true },
)

function fakeLoadingIncrease() {
    if (loadingProgress.value < 95) {
        setTimeout(() => {
            loadingProgress.value += 2
            fakeLoadingIncrease()
        }, 5)
    }
}

loading_listener(async (e) => {
    if (e.event.type === 'directory_move') {
        loadingProgress.value = 100 * (e.fraction ?? 1)
        message.value = formatMessage(messages.updatingAppDirectory)
    } else if (e.event.type === 'checking_for_updates') {
        loadingProgress.value = 100 * (e.fraction ?? 1)
        message.value = formatMessage(messages.checkingForUpdates)
    }
})
</script>

<style scoped lang="scss">
.splash-fade-leave-active {
    transition: opacity 0.3s ease-in-out;
}

.splash-fade-leave-to {
    opacity: 0;
}
</style>
