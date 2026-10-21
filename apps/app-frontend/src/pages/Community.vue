<script setup lang="ts">
import { PlayIcon, SettingsIcon } from '@modrinth/assets'
import { Button, ButtonLink, defineMessages, SkinPreviewRenderer, useVIntl } from '@modrinth/ui'
import { openUrl } from '@tauri-apps/plugin-opener'
import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'

import YsumcLoginGate from '@/components/ui/login/YsumcLoginGate.vue'
import InstanceSettingsModal from '@/components/ui/modal/InstanceSettingsModal.vue'
import YsumcWordmark from '@/components/ui/YsumcWordmark.vue'
import { YsumcConfig } from '@/config'
import { get as getInstance, run } from '@/helpers/instance'
import {
    applyManagedInstanceAction,
    managed_sync,
    type ManagedInstanceAction,
} from '@/helpers/managed'
import {
    get_available_capes,
    get_available_skins,
    get_normalized_skin_texture,
    type SkinModel,
} from '@/helpers/skins'
import type { GameInstance } from '@/helpers/types'
import { get_server_status, type ServerStatus } from '@/helpers/worlds'
import { daysSinceOpening as daysOpen } from '@/helpers/ysumc-server'
import { ysumcAccount as account } from '@/helpers/ysumc-session'

const { formatMessage: t } = useVIntl()
const messages = defineMessages({
    slogan: { id: 'ysumc.slogan', defaultMessage: '方块虽小，亦是精彩之源。' },
    intro: {
        id: 'ysumc.intro',
        defaultMessage:
            '从这里，进入我们的世界。管理你的角色，获取官方整合包，也可以自由下载和创建自己的实例。',
    },
    login: { id: 'ysumc.login', defaultMessage: '登录 YSUMC 账号' },
    register: { id: 'ysumc.register', defaultMessage: '创建游戏账号' },
    library: { id: 'ysumc.library', defaultMessage: '我的游戏' },
    download: { id: 'ysumc.download', defaultMessage: '下载与发现' },
    skins: { id: 'ysumc.skins', defaultMessage: '皮肤与披风' },
    official: { id: 'ysumc.official', defaultMessage: '官方整合包' },
    refresh: { id: 'ysumc.refresh', defaultMessage: '刷新清单' },
    loading: { id: 'ysumc.loading', defaultMessage: '正在读取整合包清单…' },
    empty: {
        id: 'ysumc.empty',
        defaultMessage: '暂未发布官方整合包。你可以先创建实例或前往下载页。',
    },
    unavailable: {
        id: 'ysumc.unavailable',
        defaultMessage: '暂时无法连接 YSUMC 服务。请稍后重试；已安装的游戏仍可从“我的游戏”打开。',
    },
    cached: {
        id: 'ysumc.cached',
        defaultMessage:
            '当前显示上次同步的清单。已安装的实例可以打开；安装或更新前请重新连接并刷新。',
    },
    lastSync: { id: 'ysumc.lastSync', defaultMessage: '上次同步' },
    retired: { id: 'ysumc.retired', defaultMessage: '已停止提供' },
    install: { id: 'ysumc.install', defaultMessage: '安装整合包' },
    update: { id: 'ysumc.update', defaultMessage: '更新整合包' },
    open: { id: 'ysumc.open', defaultMessage: '打开实例' },
    installing: { id: 'ysumc.installing', defaultMessage: '正在安装 · 下载页可查看进度' },
    approve: { id: 'ysumc.approve', defaultMessage: '在账号站确认下面的代码，并选择游戏角色。' },
    browser: { id: 'ysumc.browser', defaultMessage: '打开授权页面' },
    cancel: { id: 'ysumc.cancel', defaultMessage: '取消登录' },
    expired: { id: 'ysumc.expired', defaultMessage: '授权码已过期，请重新登录。' },
    account: { id: 'ysumc.account', defaultMessage: '当前角色' },
    change: { id: 'ysumc.change', defaultMessage: '切换 YSUMC 账号' },
    website: { id: 'ysumc.website', defaultMessage: '协会官网' },
    serverAddress: { id: 'ysumc.server.address', defaultMessage: '服务器地址' },
    gameVersion: { id: 'ysumc.server.game-version', defaultMessage: '游戏版本' },
    loader: { id: 'ysumc.server.loader', defaultMessage: '模组加载器' },
    packSize: { id: 'ysumc.server.pack-size', defaultMessage: '整合包大小' },
})

const switchingAccount = ref(false)
const skinTexture = ref('')
const skinVariant = ref<SkinModel>('CLASSIC')
const capeTexture = ref('')
const statuses = ref<Record<string, ServerStatus | null>>({})
const launching = ref('')
const settingsInstance = ref<GameInstance | null>(null)
const settingsModal = ref<InstanceType<typeof InstanceSettingsModal>>()
let statusTimer: ReturnType<typeof setInterval> | undefined
let skinGeneration = 0
async function loadSkin() {
    const current = ++skinGeneration
    skinTexture.value = ''
    capeTexture.value = ''
    if (!account.value) return
    try {
        const [skins, capes] = await Promise.all([get_available_skins(), get_available_capes()])
        const equipped = skins.find((skin) => skin.is_equipped)
        if (!equipped) return
        const texture = await get_normalized_skin_texture(equipped)
        if (disposed || current !== skinGeneration) return
        skinTexture.value = texture
        skinVariant.value = equipped.variant
        capeTexture.value =
            capes.find((cape) => cape.id === equipped.cape_id || cape.is_equipped)?.texture ?? ''
    } catch {
        /* Account details remain usable if its texture is unavailable. */
    }
}
function serverAddress(action: ManagedInstanceAction) {
    const server = action.spec.server
    return server
        ? `${server.address}${server.port && server.port !== 25565 ? `:${server.port}` : ''}`
        : null
}
async function refreshStatuses() {
    await Promise.all(
        actions.value.map(async (action) => {
            const address = serverAddress(action)
            if (!address) return
            const status = await get_server_status(address).catch(() => null)
            if (!disposed) statuses.value[action.spec.id] = status
        }),
    )
}
async function launch(action: ManagedInstanceAction) {
    if (!action.instance_id || launching.value) return
    launching.value = action.spec.id
    error.value = ''
    try {
        await run(action.instance_id, serverAddress(action))
    } catch (reason) {
        error.value = String(reason)
    } finally {
        launching.value = ''
    }
}
function finishSwitchingAccount() {
    switchingAccount.value = false
    void loadSkin()
}
async function openSettings(action: ManagedInstanceAction) {
    if (!action.instance_id) return
    try {
        settingsInstance.value = await getInstance(action.instance_id)
        await nextTick()
        settingsModal.value?.show()
    } catch (reason) {
        error.value = String(reason)
    }
}
const actions = ref<ManagedInstanceAction[]>([])
const offline = ref(false)
const syncedAt = ref('')
const loading = ref(false)
const error = ref('')
const busy = ref('')
let disposed = false

async function refresh() {
    if (loading.value) return
    loading.value = true
    error.value = ''
    try {
        const report = await managed_sync(YsumcConfig.manifestUrl)
        if (!disposed) {
            actions.value = report.instances
            void refreshStatuses()
            offline.value = report.offline
            syncedAt.value = report.synced_at
                ? new Date(report.synced_at * 1000).toLocaleString()
                : ''
        }
    } catch {
        if (!disposed) {
            offline.value = true
            error.value = t(messages.unavailable)
        }
    } finally {
        loading.value = false
    }
}

async function install(action: ManagedInstanceAction) {
    if (busy.value || loading.value || offline.value || action.action === 'retired') return
    busy.value = action.spec.id
    error.value = ''
    try {
        await applyManagedInstanceAction(action)
        await refresh()
    } catch (reason) {
        error.value = String(reason)
    } finally {
        busy.value = ''
    }
}

watch(
    () => account.value?.account_id,
    () => void loadSkin(),
)
onMounted(() => {
    void refresh()
    void loadSkin()
    statusTimer = setInterval(() => void refreshStatuses(), 60_000)
})
onUnmounted(() => {
    disposed = true
    skinGeneration++
    clearInterval(statusTimer)
})
</script>

<template>
    <main class="flex flex-col gap-6 p-6">
        <div class="ysumc-portal">
            <section data-onboarding-id="ysumc-welcome">
                <YsumcWordmark class="mb-8 w-64 max-w-full" />
                <div v-if="account" class="float-right ml-4 h-72 w-40 max-w-[40%] xl:w-48">
                    <SkinPreviewRenderer
                        v-if="skinTexture"
                        :texture-src="skinTexture"
                        :variant="skinVariant"
                        :cape-src="capeTexture"
                        :initial-rotation="Math.PI / 8"
                    />
                </div>
                <h1 class="ysumc-heading my-5 text-3xl">{{ t(messages.slogan) }}</h1>
                <p class="my-6 max-w-lg text-secondary">{{ t(messages.intro) }}</p>
                <div class="flex flex-wrap gap-3">
                    <RouterLink v-slot="{ href, navigate }" to="/library" custom>
                        <ButtonLink :href="href" @click="navigate">{{
                            t(messages.library)
                        }}</ButtonLink>
                    </RouterLink>
                    <RouterLink v-slot="{ href, navigate }" to="/browse/modpack" custom>
                        <ButtonLink :href="href" @click="navigate">{{
                            t(messages.download)
                        }}</ButtonLink>
                    </RouterLink>
                    <RouterLink v-slot="{ href, navigate }" to="/skins" custom>
                        <ButtonLink :href="href" @click="navigate">{{
                            t(messages.skins)
                        }}</ButtonLink>
                    </RouterLink>
                </div>
            </section>
            <section
                class="ysumc-brand-panel"
                :class="{ 'is-signed-in': account }"
                data-onboarding-id="ysumc-account"
            >
                <div>
                    <p class="text-xs tracking-widest">01 / ACCOUNT</p>
                    <h2 class="ysumc-heading text-2xl">YSUMC 用户中心</h2>
                    <p v-if="account">
                        {{ t(messages.account) }} ·
                        {{ account.profile?.name }}
                    </p>
                    <p v-else>游戏账号 · 角色 · 皮肤</p>
                </div>
                <div class="flex flex-col items-start gap-3">
                    <Button @click="switchingAccount = true">{{
                        t(account ? messages.change : messages.login)
                    }}</Button>
                    <Button
                        v-if="!account"
                        @click="openUrl(`${YsumcConfig.siteUrl}/auth/register`)"
                        >{{ t(messages.register) }}</Button
                    >
                    <Button @click="openUrl(YsumcConfig.websiteUrl)">{{
                        t(messages.website)
                    }}</Button>
                </div>
            </section>
        </div>
        <div
            class="flex flex-wrap items-center justify-between gap-3"
            data-onboarding-id="ysumc-official-packs"
        >
            <h2 class="ysumc-heading m-0 text-2xl">{{ t(messages.official) }}</h2>
            <Button :disabled="loading || !!busy" @click="refresh">{{
                t(messages.refresh)
            }}</Button>
        </div>
        <p
            v-if="error"
            role="alert"
            class="m-0 border-0 border-l-4 border-solid border-brand bg-surface-2 p-4"
        >
            {{ error }}
        </p>
        <div v-if="offline && !error" role="status" class="bg-surface-2 p-4">
            <p class="m-0">{{ t(messages.cached) }}</p>
            <p v-if="syncedAt" class="mb-0 text-sm text-secondary">
                {{ t(messages.lastSync) }} · {{ syncedAt }}
            </p>
        </div>
        <p v-if="loading && !actions.length" role="status">{{ t(messages.loading) }}</p>
        <p v-else-if="!actions.length && !error">{{ t(messages.empty) }}</p>
        <div class="grid grid-cols-1 gap-4 xl:grid-cols-2">
            <article
                v-for="action in actions"
                :key="action.spec.id"
                class="flex min-w-0 flex-col gap-5 border-0 border-t-4 border-solid border-brand bg-surface-2 p-6 md:p-8"
            >
                <img
                    v-if="action.spec.icon_url"
                    :src="action.spec.icon_url"
                    alt=""
                    class="h-16 w-16 object-contain"
                />
                <h3 class="ysumc-heading m-0 text-2xl">{{ action.spec.name }}</h3>
                <p v-if="action.spec.description" class="m-0 whitespace-pre-line text-secondary">
                    {{ action.spec.description }}
                </p>
                <dl class="m-0 grid grid-cols-1 gap-4 sm:grid-cols-2">
                    <div v-if="action.spec.server?.address" class="min-w-0 sm:col-span-2">
                        <dt class="mb-1 text-sm text-secondary">{{ t(messages.serverAddress) }}</dt>
                        <dd class="m-0 select-text break-all font-mono text-lg text-primary">
                            {{ action.spec.server.address
                            }}{{
                                action.spec.server.port && action.spec.server.port !== 25565
                                    ? `:${action.spec.server.port}`
                                    : ''
                            }}
                        </dd>
                    </div>
                    <div>
                        <dt class="mb-1 text-sm text-secondary">{{ t(messages.gameVersion) }}</dt>
                        <dd class="m-0 font-medium">
                            Minecraft {{ action.spec.minecraft.game_version }}
                        </dd>
                    </div>
                    <div v-if="action.spec.minecraft.loader">
                        <dt class="mb-1 text-sm text-secondary">{{ t(messages.loader) }}</dt>
                        <dd class="m-0 font-medium">
                            <span class="capitalize">{{ action.spec.minecraft.loader }}</span>
                            {{ action.spec.minecraft.loader_version }}
                        </dd>
                    </div>
                    <div v-if="action.spec.pack?.size">
                        <dt class="mb-1 text-sm text-secondary">{{ t(messages.packSize) }}</dt>
                        <dd class="m-0 font-medium">
                            {{ (action.spec.pack.size / 1024 / 1024).toFixed(1) }} MiB
                        </dd>
                    </div>
                    <div v-if="action.spec.server">
                        <dt class="mb-1 text-sm text-secondary">在线人数</dt>
                        <dd class="m-0 font-medium">
                            {{
                                statuses[action.spec.id]?.players
                                    ? `${statuses[action.spec.id]?.players?.online} / ${statuses[action.spec.id]?.players?.max}`
                                    : statuses[action.spec.id] === null
                                      ? '暂时无法连接'
                                      : '正在查询…'
                            }}
                        </dd>
                    </div>
                    <div v-if="daysOpen(action.spec.server?.opened_on) !== null">
                        <dt class="mb-1 text-sm text-secondary">开服天数</dt>
                        <dd class="m-0 font-medium">
                            第 {{ daysOpen(action.spec.server?.opened_on) }} 天
                        </dd>
                    </div>
                </dl>
                <div
                    v-if="action.action === 'current' && action.instance_id"
                    class="mt-auto flex items-end gap-3"
                >
                    <Button class="flex-1" :disabled="!!launching || !!busy" @click="launch(action)"
                        ><PlayIcon />{{
                            launching === action.spec.id ? '正在校验模组并启动…' : '启动游戏'
                        }}</Button
                    >
                    <Button icon-only aria-label="整合包设置" @click="openSettings(action)"
                        ><SettingsIcon
                    /></Button>
                </div>
                <Button
                    v-else
                    class="mt-auto w-full"
                    :disabled="!!busy || loading || offline || action.action === 'retired'"
                    @click="install(action)"
                    >{{
                        t(
                            action.action === 'retired'
                                ? messages.retired
                                : busy === action.spec.id
                                  ? messages.installing
                                  : action.action === 'update'
                                    ? messages.update
                                    : messages.install,
                        )
                    }}</Button
                >
            </article>
        </div>
        <InstanceSettingsModal
            v-if="settingsInstance"
            ref="settingsModal"
            :instance="settingsInstance"
        />
        <YsumcLoginGate v-if="switchingAccount" @complete="finishSwitchingAccount" />
        <Button
            v-if="switchingAccount"
            class="fixed right-6 top-16 z-[100]"
            @click="switchingAccount = false"
            >取消切换</Button
        >
    </main>
</template>
