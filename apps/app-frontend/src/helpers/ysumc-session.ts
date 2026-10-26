import { invoke } from '@tauri-apps/api/core'
import { computed, ref, watch } from 'vue'

import { isMutmc } from '@/branding'
import { YsumcConfig } from '@/config'

import { set_default_user, users } from './auth'
import { type LauncherIdentity, launcherSiteDocument } from './launcher-site'
import type { MinecraftAccount } from './minecraft-account'

export const ysumcAccount = ref<MinecraftAccount | null>(null)
export const ysumcSessionChecked = ref(false)
export const ysumcSignedIn = computed(() => !!ysumcAccount.value)
let generation = 0

export function isYsumcAccount(account: MinecraftAccount) {
    if (isMutmc) return account.account_type === 'microsoft'
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
    if (!isYsumcAccount(account))
        throw new Error(isMutmc ? '请使用 Microsoft 正版账号登录。' : '请使用 YSUMC 账号登录。')
    await set_default_user(account.account_id)
    generation++
    ysumcAccount.value = account
    ysumcSessionChecked.value = true
}

export const ysumcIdentity = ref<LauncherIdentity | null>(null)
let identityGeneration = 0
export async function refreshYsumcIdentity() {
    const current = ++identityGeneration
    const account = ysumcAccount.value
    ysumcIdentity.value = null
    if (isMutmc || !account) return
    try {
        const identity = await launcherSiteDocument<LauncherIdentity>('identity')
        if (current !== identityGeneration || ysumcAccount.value?.account_id !== account.account_id)
            return
        if (
            identity &&
            identity.player === account.profile?.name &&
            Array.isArray(identity.tags) &&
            identity.tags.length <= 2 &&
            identity.tags.every((tag) => typeof tag === 'string' && tag.length <= 30)
        ) {
            ysumcIdentity.value = identity
        }
    } catch {
        // Do not infer a privilege from a cached or unverified player profile.
    }
}
watch(ysumcAccount, () => void refreshYsumcIdentity(), { flush: 'sync' })
