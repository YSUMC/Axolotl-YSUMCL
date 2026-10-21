import { invoke } from '@tauri-apps/api/core'
import { computed, ref } from 'vue'

import { YsumcConfig } from '@/config'

import { set_default_user, users } from './auth'
import type { MinecraftAccount } from './minecraft-account'

export const ysumcAccount = ref<MinecraftAccount | null>(null)
export const ysumcSessionChecked = ref(false)
export const ysumcSignedIn = computed(() => !!ysumcAccount.value)
let generation = 0

export function isYsumcAccount(account: MinecraftAccount) {
    return (
        account.account_type === 'yggdrasil' &&
        account.yggdrasil?.api_root?.replace(/\/$/, '') === YsumcConfig.yggdrasilApiRoot
    )
}

export async function refreshYsumcSession() {
    const current = ++generation
    try {
        const id = await invoke<string | null>('plugin:auth|validate_ysumc_session')
        const accounts = id ? await users(false, true) : []
        if (current !== generation) return
        ysumcAccount.value =
            accounts.find((account) => account.account_id === id && isYsumcAccount(account)) ?? null
    } catch {
        if (current === generation) ysumcAccount.value = null
    } finally {
        if (current === generation) ysumcSessionChecked.value = true
    }
}

export async function acceptYsumcSession(account: MinecraftAccount) {
    if (!isYsumcAccount(account)) throw new Error('请使用 YSUMC 账号登录。')
    await set_default_user(account.account_id)
    generation++
    ysumcAccount.value = account
    ysumcSessionChecked.value = true
}
