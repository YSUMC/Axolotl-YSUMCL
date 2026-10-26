<script setup lang="ts">
import { GlobeIcon, PlayIcon, SettingsIcon } from '@modrinth/assets'
import { Button, ButtonLink, defineMessages, SkinPreviewRenderer, useVIntl } from '@modrinth/ui'
import { openUrl } from '@tauri-apps/plugin-opener'
import { computed, nextTick, onActivated, onMounted, onUnmounted, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'

import { isMutmc, launcherBrand } from '@/branding'
import { mutmcMessages } from '@/components/brands/mutmc/messages'
import MutmcHomeHero from '@/components/brands/mutmc/MutmcHomeHero.vue'
import MicrosoftLoginGate from '@/components/ui/login/MicrosoftLoginGate.vue'
import YsumcLoginGate from '@/components/ui/login/YsumcLoginGate.vue'
import InstanceSettingsModal from '@/components/ui/modal/InstanceSettingsModal.vue'
import YsumcWordmark from '@/components/ui/YsumcWordmark.vue'
import { YsumcConfig } from '@/config'
import { instance_listener } from '@/helpers/events'
import { get as getInstance, run } from '@/helpers/instance'
import {
    applyManagedInstanceAction,
    managed_list,
    managed_sync,
    type ManagedInstanceAction,
    type ManagedInstanceRecord,
} from '@/helpers/managed'
import {
    canInstallManagedSpec,
    groupManagedActions,
    managedServerAddress,
} from '@/helpers/managed-catalog'
import { reconcileManagedActions } from '@/helpers/managed-state'
import {
    get_available_capes,
    get_available_skins,
    get_normalized_skin_texture,
    type SkinModel,
} from '@/helpers/skins'
import type { GameInstance } from '@/helpers/types'
import { get_server_status, type ServerStatus } from '@/helpers/worlds'
import { daysSinceOpening as daysOpen } from '@/helpers/ysumc-server'
import {
    refreshYsumcIdentity,
    ysumcAccount as account,
    ysumcIdentity,
} from '@/helpers/ysumc-session'

const props = withDefaults(defineProps<{ partners?: boolean }>(), { partners: false })
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
    catalog: { id: 'ysumc.catalog.title', defaultMessage: 'Servers and modpacks' },
    partners: { id: 'ysumc.partners.title', defaultMessage: 'Partner club servers and modpacks' },
    partnerCount: { id: 'ysumc.partners.count', defaultMessage: '{sites} clubs · {count} entries' },
    partnersEmpty: {
        id: 'ysumc.partners.empty',
        defaultMessage: 'No partner servers or modpacks have been published yet.',
    },
    own: { id: 'ysumc.catalog.own', defaultMessage: 'YSUMC' },
    peer: { id: 'ysumc.catalog.peer', defaultMessage: 'From {site}' },
    stale: {
        id: 'ysumc.catalog.stale',
        defaultMessage:
            'This site is temporarily unavailable. Showing its last published information.',
    },
    skipped: {
        id: 'ysumc.catalog.skipped',
        defaultMessage: 'Some entries could not be loaded. Refresh to try again.',
    },
    unsupported: {
        id: 'ysumc.catalog.unsupported',
        defaultMessage: 'Installation of {format} packs is not supported yet',
    },
    serverOnly: { id: 'ysumc.catalog.serverOnly', defaultMessage: 'Server · no modpack required' },
    prepareServer: { id: 'ysumc.catalog.prepareServer', defaultMessage: 'Prepare game' },
    refreshServer: { id: 'ysumc.catalog.refreshServer', defaultMessage: 'Update game' },
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
let catalogTimer: ReturnType<typeof setInterval> | undefined
let lastRefresh = 0
const skippedEntries = ref(false)
let skinGeneration = 0
async function loadSkin() {
    const current = ++skinGeneration
    skinTexture.value = ''
    capeTexture.value = ''
    if (!account.value || props.partners) return
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
    return managedServerAddress(action.spec.server)
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
    if (!action.instance_id || launching.value || loading.value) return
    launching.value = action.spec.id
    error.value = ''
    try {
        await refresh()
        const current = actions.value.find((item) => item.spec.id === action.spec.id)
        if (!current || current.action !== 'current' || !current.instance_id) return
        await run(current.instance_id, serverAddress(current))
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
const manifestActions = ref<ManagedInstanceAction[]>([])
const localRecords = ref<ManagedInstanceRecord[]>([])
const groups = computed(() =>
    groupManagedActions(
        reconcileManagedActions(manifestActions.value, localRecords.value),
        props.partners,
    ),
)
const actions = computed(() => groups.value.flatMap((group) => group.actions))
let localRequest = 0
let unlistenInstances: (() => void) | undefined
async function refreshLocalRecords() {
    const request = ++localRequest
    try {
        const records = await managed_list()
        if (!disposed && request === localRequest) localRecords.value = records
    } catch (reason) {
        if (!disposed) error.value = String(reason)
    }
}
const offline = ref(false)
const syncedAt = ref('')
const loading = ref(false)
const error = ref('')
const busy = ref('')
let disposed = false

let catalogGeneration = 0
async function refresh() {
    if (loading.value) return
    const current = ++catalogGeneration
    loading.value = true
    error.value = ''
    void refreshYsumcIdentity()
    try {
        const report = await managed_sync(YsumcConfig.manifestUrl)
        if (!disposed && current === catalogGeneration) {
            manifestActions.value = report.instances
            skippedEntries.value = !!report.diagnostics?.length
            lastRefresh = Date.now()
            await refreshLocalRecords()
            void refreshStatuses()
            offline.value = report.offline
            syncedAt.value = report.synced_at
                ? new Date(report.synced_at * 1000).toLocaleString()
                : ''
        }
    } catch {
        if (!disposed && current === catalogGeneration) {
            if (!isMutmc) manifestActions.value = []
            offline.value = true
            error.value = t(isMutmc ? mutmcMessages.unavailable : messages.unavailable)
        }
    } finally {
        if (current === catalogGeneration) loading.value = false
    }
}

async function install(action: ManagedInstanceAction) {
    if (
        busy.value ||
        loading.value ||
        offline.value ||
        action.action === 'retired' ||
        !canInstallManagedSpec(action.spec)
    )
        return
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

function refreshOnFocus() {
    if (!document.hidden && Date.now() - lastRefresh > 60_000) void refresh()
}

watch(
    () => props.partners,
    () => {
        void refreshStatuses()
        void loadSkin()
    },
)
watch(
    () => account.value?.account_id,
    () => {
        ++catalogGeneration
        manifestActions.value = []
        statuses.value = {}
        loading.value = false
        void loadSkin()
        void refresh()
    },
    { flush: 'sync' },
)
onMounted(() => {
    void instance_listener((event: { event: string; instance_id: string }) => {
        if (disposed) return
        if (event.event === 'Removed') {
            ++localRequest
            localRecords.value = localRecords.value.filter(
                (record) => record.instance_id !== event.instance_id,
            )
            if (settingsInstance.value?.id === event.instance_id) {
                settingsModal.value?.hide()
                settingsInstance.value = null
            }
        }
        void refreshLocalRecords()
    })
        .then((unlisten) => {
            if (disposed) unlisten()
            else unlistenInstances = unlisten
        })
        .catch((reason) => {
            if (!disposed) error.value = String(reason)
        })
    void refreshLocalRecords()
    void refresh()
    void loadSkin()
    statusTimer = setInterval(() => void refreshStatuses(), 60_000)
    catalogTimer = setInterval(() => void refresh(), 6 * 60 * 60_000)
    window.addEventListener('focus', refreshOnFocus)
    document.addEventListener('visibilitychange', refreshOnFocus)
})
onActivated(() => {
    refreshOnFocus()
    void refreshLocalRecords()
    void loadSkin()
})
onUnmounted(() => {
    disposed = true
    skinGeneration++
    clearInterval(statusTimer)
    clearInterval(catalogTimer)
    window.removeEventListener('focus', refreshOnFocus)
    document.removeEventListener('visibilitychange', refreshOnFocus)
    unlistenInstances?.()
})
</script>

<template>
    <main class="flex flex-col gap-6 p-6">
        <MutmcHomeHero
            v-if="isMutmc"
            :name="account?.profile?.name"
            :texture="skinTexture"
            :variant="skinVariant"
            :cape="capeTexture"
            @switch-account="switchingAccount = true"
            @website="openUrl(launcherBrand.website)"
        />
        <div v-else-if="!partners" class="ysumc-portal">
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
                    <div v-if="account && ysumcIdentity" class="mt-2 flex flex-wrap gap-2">
                        <span
                            v-for="tag in ysumcIdentity.tags"
                            :key="tag"
                            class="rounded-full border border-divider bg-surface-3 px-3 py-1 text-sm font-semibold text-[var(--color-text-primary)]"
                            >{{ tag }}</span
                        >
                    </div>
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
            :data-onboarding-id="partners ? undefined : 'ysumc-official-packs'"
        >
            <h2 class="ysumc-heading m-0 text-2xl">
                {{
                    t(
                        isMutmc
                            ? mutmcMessages.packs
                            : partners
                              ? messages.partners
                              : messages.catalog,
                    )
                }}
            </h2>
            <span v-if="partners" class="ml-auto text-sm text-secondary" role="status">{{
                t(messages.partnerCount, { sites: groups.length, count: actions.length })
            }}</span>
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
        <p v-if="skippedEntries" role="status" class="m-0 text-secondary">
            {{ t(messages.skipped) }}
        </p>
        <p v-if="!loading && !actions.length && !error">
            {{ t(partners ? messages.partnersEmpty : messages.empty) }}
        </p>
        <section
            v-for="group in groups"
            :key="group.id"
            :aria-label="partners ? group.name : undefined"
            class="min-w-0"
            :class="
                partners
                    ? 'flex flex-col gap-5 rounded-lg border border-solid border-surface-4 bg-surface-2 p-5 md:p-6'
                    : ''
            "
        >
            <header v-if="partners" class="flex flex-wrap items-start justify-between gap-3">
                <div class="min-w-0 flex-1">
                    <h3 class="m-0 break-words text-xl font-semibold text-primary">
                        {{ group.name }}
                    </h3>
                    <p
                        v-if="group.description"
                        class="mb-0 mt-2 whitespace-pre-line break-words text-secondary"
                    >
                        {{ group.description }}
                    </p>
                </div>
                <a
                    :href="`https://${group.domain}`"
                    class="flex max-w-full items-center gap-2 break-all text-sm !text-[var(--color-text-tertiary)] hover:!text-[var(--color-text-primary)]"
                    @click.prevent="openUrl(`https://${group.domain}`)"
                    ><GlobeIcon class="h-4 w-4 shrink-0" />{{ group.domain }}</a
                >
            </header>
            <div
                :class="partners ? 'flex flex-col gap-3' : 'grid grid-cols-1 gap-4 xl:grid-cols-2'"
            >
                <article
                    v-for="action in group.actions"
                    :key="action.spec.id"
                    class="flex min-w-0 gap-5"
                    :class="
                        partners
                            ? 'flex-col rounded-lg border border-solid border-surface-4 bg-surface-3 p-5 lg:flex-row lg:items-center'
                            : isMutmc
                              ? 'flex-col rounded-2xl border border-solid border-divider bg-surface-2 p-6 md:p-8'
                              : 'flex-col border-0 border-t-4 border-solid border-brand bg-surface-2 p-6 md:p-8'
                    "
                >
                    <div class="flex min-w-0 flex-1 flex-col gap-4">
                        <img
                            v-if="action.spec.icon_url"
                            :src="action.spec.icon_url"
                            alt=""
                            class="h-16 w-16 object-contain"
                        />
                        <h3 class="ysumc-heading m-0 text-2xl">{{ action.spec.name }}</h3>
                        <p
                            v-if="!partners || action.spec.pack.kind === 'server'"
                            class="m-0 text-sm text-secondary"
                        >
                            {{
                                action.spec.source
                                    ? t(messages.peer, { site: action.spec.source.domain })
                                    : launcherBrand.shortName
                            }}
                            <span v-if="action.spec.pack.kind === 'server'">
                                · {{ t(messages.serverOnly) }}</span
                            >
                        </p>
                        <p
                            v-if="action.spec.source?.stale"
                            role="status"
                            class="m-0 text-sm text-secondary"
                        >
                            {{ t(messages.stale) }}
                        </p>
                        <p
                            v-if="action.spec.description"
                            class="m-0 whitespace-pre-line text-secondary"
                        >
                            {{ action.spec.description }}
                        </p>
                        <dl
                            class="m-0 grid grid-cols-1 gap-4 sm:grid-cols-2"
                            :class="{ 'xl:grid-cols-3': partners }"
                        >
                            <div v-if="action.spec.server?.address" class="min-w-0 sm:col-span-2">
                                <dt class="mb-1 text-sm text-secondary">
                                    {{ t(messages.serverAddress) }}
                                </dt>
                                <dd
                                    class="m-0 select-text break-all font-mono text-lg text-primary"
                                >
                                    {{ serverAddress(action) }}
                                </dd>
                            </div>
                            <div>
                                <dt class="mb-1 text-sm text-secondary">
                                    {{ t(messages.gameVersion) }}
                                </dt>
                                <dd class="m-0 font-medium">
                                    Minecraft {{ action.spec.minecraft.game_version }}
                                </dd>
                            </div>
                            <div v-if="action.spec.minecraft.loader">
                                <dt class="mb-1 text-sm text-secondary">
                                    {{ t(messages.loader) }}
                                </dt>
                                <dd class="m-0 font-medium">
                                    <span class="capitalize">{{
                                        action.spec.minecraft.loader
                                    }}</span>
                                    {{ action.spec.minecraft.loader_version }}
                                </dd>
                            </div>
                            <div v-if="action.spec.pack?.size">
                                <dt class="mb-1 text-sm text-secondary">
                                    {{ t(messages.packSize) }}
                                </dt>
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
                    </div>
                    <div
                        class="flex shrink-0 flex-col gap-3"
                        :class="partners ? 'lg:w-44' : 'mt-auto'"
                    >
                        <div
                            v-if="
                                action.action === 'current' &&
                                action.instance_id &&
                                canInstallManagedSpec(action.spec)
                            "
                            class="mt-auto flex items-end gap-3"
                        >
                            <Button
                                class="!h-auto min-h-10 flex-1"
                                :disabled="!!launching || !!busy || loading"
                                @click="launch(action)"
                                ><PlayIcon /><span class="min-w-0 whitespace-normal break-words">{{
                                    launching === action.spec.id
                                        ? '正在校验模组并启动…'
                                        : '启动游戏'
                                }}</span></Button
                            >
                            <Button icon-only aria-label="整合包设置" @click="openSettings(action)"
                                ><SettingsIcon
                            /></Button>
                        </div>
                        <Button
                            v-else
                            class="mt-auto !h-auto min-h-10 w-full"
                            :disabled="
                                !!busy ||
                                loading ||
                                offline ||
                                action.action === 'retired' ||
                                !canInstallManagedSpec(action.spec)
                            "
                            @click="install(action)"
                            ><span class="min-w-0 whitespace-normal break-words">{{
                                !canInstallManagedSpec(action.spec)
                                    ? t(messages.unsupported, { format: action.spec.pack.kind })
                                    : t(
                                          action.action === 'retired'
                                              ? messages.retired
                                              : busy === action.spec.id
                                                ? messages.installing
                                                : action.action === 'update'
                                                  ? action.spec.pack.kind === 'server'
                                                      ? messages.refreshServer
                                                      : messages.update
                                                  : action.spec.pack.kind === 'server'
                                                    ? messages.prepareServer
                                                    : messages.install,
                                      )
                            }}</span></Button
                        >
                    </div>
                </article>
            </div>
        </section>
        <InstanceSettingsModal
            v-if="settingsInstance"
            ref="settingsModal"
            :instance="settingsInstance"
        />
        <component
            :is="isMutmc ? MicrosoftLoginGate : YsumcLoginGate"
            v-if="switchingAccount"
            @complete="finishSwitchingAccount"
        />
        <Button
            v-if="switchingAccount"
            class="fixed right-6 top-16 z-[100]"
            @click="switchingAccount = false"
            >取消切换</Button
        >
    </main>
</template>
