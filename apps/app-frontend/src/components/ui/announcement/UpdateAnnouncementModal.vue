<script setup lang="ts">
import { Button, commonMessages, defineMessages, NewModal, useVIntl } from '@modrinth/ui'
import { computed, ref } from 'vue'

import { useLauncherReleases } from '@/composables/useLauncherReleases'

import UpdateAnnouncementContent from './UpdateAnnouncementContent.vue'

const emit = defineEmits<{
    closed: [version: string]
}>()

const { formatMessage } = useVIntl()
const modal = ref<InstanceType<typeof NewModal>>()
const version = ref<string | null>(null)
const { releases, loading, failed, refresh } = useLauncherReleases()
const release = computed(() => releases.value.find((item) => item.version === version.value))

const messages = defineMessages({
    retry: { id: 'app.ysumc-releases.refresh', defaultMessage: 'Refresh release notes' },
    header: {
        id: 'app.update-announcement.modal.header',
        defaultMessage: "What's new",
    },
})

function show(nextVersion: string) {
    version.value = nextVersion
    modal.value?.show()
    void refresh()
}

function close() {
    modal.value?.hide()
}

function handleHide() {
    if (version.value) emit('closed', version.value)
}

defineExpose({ show, close })
</script>

<template>
    <NewModal
        ref="modal"
        :header="formatMessage(messages.header)"
        :on-hide="handleHide"
        max-width="760px"
        max-content-height="min(68vh, 42rem)"
        scrollable
        actions-divider
    >
        <UpdateAnnouncementContent
            :release="release"
            :version="version"
            :loading="loading"
            :failed="failed"
        />

        <template #actions>
            <div class="flex justify-end gap-2">
                <Button v-if="failed" type="outlined" :disabled="loading" @click="refresh">{{
                    formatMessage(messages.retry)
                }}</Button>
                <Button type="colored" color="brand" @click="close"
                    >{{ formatMessage(commonMessages.closeButton) }}
                </Button>
            </div>
        </template>
    </NewModal>
</template>
