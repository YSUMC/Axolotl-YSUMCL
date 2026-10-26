<script setup lang="ts">
import { ref } from 'vue'

import MutmcLoginPanel from '@/components/brands/mutmc/MutmcLoginPanel.vue'
import MinecraftLoginModal from '@/components/ui/MinecraftLoginModal.vue'
import { users } from '@/helpers/auth'
import type { MinecraftAccount } from '@/helpers/minecraft-account'
import { acceptYsumcSession } from '@/helpers/ysumc-session'

const emit = defineEmits<{ complete: [] }>()
const modal = ref<InstanceType<typeof MinecraftLoginModal>>()
const error = ref('')
const busy = ref(false)

async function complete(credentials: { profile: { id: string } }) {
    busy.value = true
    error.value = ''
    try {
        const accounts: MinecraftAccount[] = await users(false, true)
        const account = accounts.find(
            (item) =>
                item.account_type === 'microsoft' && item.profile.id === credentials.profile.id,
        )
        if (!account) throw new Error('Minecraft account was not returned. Please sign in again.')
        await acceptYsumcSession(account)
        emit('complete')
    } catch (reason) {
        error.value = String(reason)
    } finally {
        busy.value = false
    }
}
</script>

<template>
    <div class="fixed inset-x-0 bottom-0 top-12 z-[90] overflow-auto bg-surface-1 p-6">
        <MutmcLoginPanel
            class="mx-auto max-w-6xl"
            :error="error"
            :busy="busy"
            @login="modal?.showDeviceLogin()"
        />
        <MinecraftLoginModal ref="modal" @complete="complete" />
    </div>
</template>
