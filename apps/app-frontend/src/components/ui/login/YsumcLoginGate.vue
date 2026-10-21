<script setup lang="ts">
import { Button } from '@modrinth/ui'
import { invoke } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'
import { onUnmounted, ref } from 'vue'

import YsumcWordmark from '@/components/ui/YsumcWordmark.vue'
import { YsumcConfig } from '@/config'
import { begin_yggdrasil_login, finish_yggdrasil_login } from '@/helpers/auth'
import type { MinecraftAccount } from '@/helpers/minecraft-account'
import { acceptYsumcSession } from '@/helpers/ysumc-session'

const emit = defineEmits<{ complete: [] }>()
const busy = ref(false)
const error = ref('')
const passwordMode = ref(false)
const loginName = ref('')
const password = ref('')
const profiles = ref<{ id: string; name: string }[]>([])
const profileFlow = ref('')
const flow = ref<{
    flow_id: string
    user_code: string
    verification_uri: string
    verification_uri_complete: string
    expires_in: number
    interval: number
} | null>(null)
let generation = 0
let timer: ReturnType<typeof setTimeout> | undefined

function cancel() {
    generation++
    clearTimeout(timer)
    flow.value = null
    profiles.value = []
    busy.value = false
}
function switchMode() {
    cancel()
    passwordMode.value = !passwordMode.value
}
async function complete(account: MinecraftAccount) {
    await acceptYsumcSession(account)
    password.value = ''
    cancel()
    emit('complete')
}
async function openAuthorization() {
    if (!flow.value) return
    const url = new URL(flow.value.verification_uri_complete || flow.value.verification_uri)
    if (url.origin !== YsumcConfig.siteUrl || url.username || url.password)
        throw new Error('授权地址无效')
    await openUrl(url.href)
}
async function start() {
    cancel()
    const current = generation
    busy.value = true
    error.value = ''
    try {
        const started = await invoke<NonNullable<typeof flow.value>>(
            'plugin:auth|begin_ysumc_device_login',
        )
        if (current !== generation) return
        flow.value = started
        const deadline = Date.now() + Math.min(started.expires_in, 900) * 1000
        let interval = Math.max(started.interval, 1) * 1000
        const poll = async () => {
            if (current !== generation) return
            try {
                if (Date.now() >= deadline) throw new Error('授权码已过期，请重试。')
                const result = await invoke<
                    | { status: 'pending'; slow_down: boolean }
                    | { status: 'complete'; credentials: MinecraftAccount }
                >('plugin:auth|poll_ysumc_device_login', { flowId: started.flow_id })
                if (current !== generation) return
                if (result.status === 'complete') {
                    await complete(result.credentials)
                    return
                }
                if (result.slow_down) interval += 5000
                timer = setTimeout(() => void poll(), interval)
            } catch (reason) {
                if (current === generation) {
                    cancel()
                    error.value = String(reason)
                }
            }
        }
        timer = setTimeout(() => void poll(), interval)
        await openAuthorization().catch((reason) => {
            error.value = String(reason)
        })
    } catch (reason) {
        if (current === generation) {
            cancel()
            error.value = String(reason)
        }
    }
}
async function submit(profileId?: string) {
    const current = ++generation
    busy.value = true
    error.value = ''
    try {
        if (profileId) {
            const account = await finish_yggdrasil_login(profileFlow.value, profileId)
            if (current !== generation) return
            await complete(account)
        } else {
            const result = await begin_yggdrasil_login(
                YsumcConfig.yggdrasilApiRoot,
                loginName.value.trim(),
                password.value,
            )
            if (current !== generation) return
            password.value = ''
            if (result.status === 'select_profile') {
                profiles.value = result.profiles
                profileFlow.value = result.flow_id
            } else await complete(result.credentials)
        }
    } catch (reason) {
        if (current === generation) error.value = String(reason)
    } finally {
        if (current === generation) {
            busy.value = false
            password.value = ''
        }
    }
}
onUnmounted(cancel)
</script>

<template>
    <main
        class="fixed inset-0 z-[90] flex items-center justify-center overflow-y-auto bg-surface-1 p-6 pt-14"
    >
        <section
            class="w-full max-w-lg border-0 border-t-4 border-solid border-brand bg-surface-2 p-8"
        >
            <YsumcWordmark class="mb-8 w-72 max-w-full" />
            <h1 class="ysumc-heading text-2xl">登录 YSUMC</h1>
            <p class="mb-6 text-secondary">登录并选择角色，开始你的方块之旅。</p>
            <div class="flex flex-col gap-4">
                <template v-if="profiles.length">
                    <p>请选择游戏角色</p>
                    <Button
                        v-for="profile in profiles"
                        :key="profile.id"
                        :disabled="busy"
                        @click="submit(profile.id)"
                        >{{ profile.name }}</Button
                    >
                </template>
                <form
                    v-else-if="passwordMode"
                    class="flex flex-col gap-4"
                    @submit.prevent="submit()"
                >
                    <label class="flex flex-col gap-2"
                        >邮箱或用户名<input v-model="loginName" autocomplete="username" required
                    /></label>
                    <label class="flex flex-col gap-2"
                        >密码<input
                            v-model="password"
                            type="password"
                            autocomplete="current-password"
                            required
                    /></label>
                    <Button submit :disabled="busy || !loginName.trim() || !password">登录</Button>
                </form>
                <template v-else-if="flow">
                    <p>在浏览器确认授权并选择角色：</p>
                    <strong class="select-text text-center font-mono text-3xl tracking-widest">{{
                        flow.user_code
                    }}</strong>
                    <Button @click="openAuthorization().catch((reason) => (error = String(reason)))"
                        >打开授权页面</Button
                    >
                    <p role="status" class="text-sm text-secondary">正在等待授权…</p>
                    <Button @click="cancel">取消并重试</Button>
                </template>
                <Button v-else :disabled="busy" @click="start">{{
                    busy ? '正在连接…' : '使用 YSUMC 账号登录'
                }}</Button>
                <p v-if="error" role="alert" class="break-words text-red">{{ error }}</p>
                <Button type="quiet" @click="switchMode">{{
                    passwordMode ? '浏览器授权登录' : '使用账号密码登录'
                }}</Button>
                <Button type="quiet" @click="openUrl(`${YsumcConfig.siteUrl}/auth/register`)"
                    >创建游戏账号</Button
                >
            </div>
        </section>
    </main>
</template>
