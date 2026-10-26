<script setup lang="ts">
import {
    ChangeSkinIcon,
    CompassIcon,
    GlobeIcon,
    HomeIcon,
    ImageIcon,
    LibraryIcon,
    SettingsIcon,
    SunIcon,
} from '@modrinth/assets'
import { ref, watch } from 'vue'
import { useRoute } from 'vue-router'

import MutmcLoginPanel from '@/components/brands/mutmc/MutmcLoginPanel.vue'
import Community from '@/pages/Community.vue'

import { previewState } from './state'

const page = ref('home')
const dark = ref(false)
const route = useRoute()
watch(
    () => route.path,
    (path) => {
        if (path !== '/')
            previewState.notice =
                '该入口在正式版本中使用共用功能页。本次预览集中展示新的首页和登录页。'
    },
)
function toggleTheme() {
    dark.value = !dark.value
    document.documentElement.className = dark.value ? 'dark-mode' : 'light-mode'
}
const nav = [
    { label: '首页', icon: HomeIcon },
    { label: '皮肤', icon: ChangeSkinIcon },
    { label: '实例库', icon: LibraryIcon },
    { label: '发现内容', icon: CompassIcon },
    { label: '截图', icon: ImageIcon },
    { label: '云地图', icon: GlobeIcon },
]
</script>

<template>
    <div class="min-h-screen bg-surface-1 text-[var(--color-text-primary)]">
        <div
            class="flex flex-wrap items-center justify-between gap-3 border-0 border-b border-solid border-divider bg-surface-2 px-6 py-3 text-sm"
        >
            <div class="flex items-center gap-3">
                <strong>MUTMCL 设计预览</strong
                ><span class="text-secondary">演示账号与数据 · 不执行下载</span>
            </div>
            <div class="flex flex-wrap items-center gap-3">
                <button
                    class="rounded-lg border border-solid border-divider bg-surface-3 px-3 py-2 text-[var(--color-text-primary)]"
                    @click="page = page === 'home' ? 'login' : 'home'"
                >
                    {{ page === 'home' ? '查看登录页' : '查看首页' }}
                </button>
                <select
                    v-model="previewState.catalog"
                    aria-label="整合包预览状态"
                    class="rounded-lg border border-solid border-divider bg-surface-3 p-2 text-[var(--color-text-primary)]"
                >
                    <option value="empty">空清单（初始状态）</option>
                    <option value="sample">演示整合包</option>
                    <option value="installed">演示已安装</option>
                </select>
                <button
                    class="flex items-center gap-2 rounded-lg border border-solid border-divider bg-surface-3 px-3 py-2 text-[var(--color-text-primary)]"
                    @click="toggleTheme"
                >
                    <SunIcon class="h-4 w-4" />{{ dark ? '切换浅色' : '切换深色' }}
                </button>
            </div>
        </div>
        <div class="mx-auto max-w-[1500px] px-4 py-6 lg:px-10">
            <header
                class="mb-4 flex h-16 items-center justify-between rounded-2xl border border-solid border-divider bg-surface-2 px-5"
            >
                <div class="flex items-center gap-4">
                    <img
                        src="/brands/mutmc/logo.webp"
                        alt="MUT"
                        class="h-9 w-24 object-contain"
                    /><span
                        class="border-0 border-l border-solid border-divider pl-4 text-sm font-bold tracking-widest"
                        >MUTMCL</span
                    >
                </div>
                <span class="text-xs text-secondary">{{
                    page === 'login' ? '欢迎来到 MC高校锦标赛' : 'MUT_Player · Java 正版账号'
                }}</span>
            </header>
            <MutmcLoginPanel v-if="page === 'login'" @login="page = 'home'" />
            <div
                v-else
                class="grid grid-cols-[5rem_minmax(0,1fr)] gap-4 lg:grid-cols-[10rem_minmax(0,1fr)]"
            >
                <aside
                    class="flex flex-col gap-2 rounded-2xl border border-solid border-divider bg-surface-2 p-3"
                >
                    <button
                        v-for="(item, index) in nav"
                        :key="item.label"
                        :aria-label="item.label"
                        class="flex items-center gap-3 rounded-xl border-0 px-3 py-3 text-sm font-semibold"
                        :class="
                            index === 0 ? 'mutmc-primary-link' : 'bg-transparent text-secondary'
                        "
                        @click="
                            index === 0
                                ? (page = 'home')
                                : (previewState.notice =
                                      '该功能继续使用两种启动器共用的页面。本次预览展示首页、登录页和品牌主题。')
                        "
                    >
                        <component :is="item.icon" class="h-5 w-5 shrink-0" /><span
                            class="hidden lg:inline"
                            >{{ item.label }}</span
                        >
                    </button>
                    <div class="min-h-20 flex-1" />
                    <button
                        class="flex items-center gap-3 rounded-xl border-0 bg-transparent px-3 py-3 text-secondary"
                        @click="toggleTheme"
                    >
                        <SettingsIcon class="h-5 w-5" /><span class="hidden lg:inline">外观</span>
                    </button>
                </aside>
                <Community :key="previewState.catalog" class="!p-0" />
            </div>
            <p class="mb-0 mt-5 text-center text-xs text-secondary">
                MC UNIVERSITY TOURNAMENT · 基于 MUT 官网视觉，共用 YSUMC 启动器核心
            </p>
        </div>
        <button
            v-if="previewState.notice"
            role="status"
            class="fixed bottom-5 left-1/2 z-[120] max-w-xl -translate-x-1/2 rounded-xl border border-solid border-divider bg-surface-2 p-4 text-sm text-[var(--color-text-primary)] shadow-lg"
            @click="previewState.notice = ''"
        >
            {{ previewState.notice }} ×
        </button>
    </div>
</template>
