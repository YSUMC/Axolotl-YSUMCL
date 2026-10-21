<script setup lang="ts">
import { Button } from '@modrinth/ui'
import { openUrl } from '@tauri-apps/plugin-opener'
import { onMounted, onUnmounted, ref } from 'vue'

import { YsumcConfig } from '@/config'
import { managed_sync } from '@/helpers/managed'

const url = ref('')
const error = ref('')
const frameKey = ref(0)
let timer: ReturnType<typeof setInterval> | undefined
let disposed = false
async function refresh() {
    try {
        const config = await managed_sync(YsumcConfig.manifestUrl)
        if (disposed) return
        if (!config.cloud_map_url) {
            url.value = ''
            error.value = ''
            return
        }
        const target = new URL(config.cloud_map_url)
        if (
            target.protocol !== 'https:' ||
            target.username ||
            target.password ||
            target.origin === location.origin
        )
            throw new Error('云地图网址无效，请联系管理员。')
        url.value = target.href
        error.value = config.offline ? '连接暂时不可用，正在使用上次配置。' : ''
    } catch (reason) {
        if (!disposed) error.value = String(reason)
    }
}
function reloadMap() {
    void refresh()
    frameKey.value++
}
onMounted(() => {
    void refresh()
    timer = setInterval(() => void refresh(), 60_000)
})
onUnmounted(() => {
    disposed = true
    clearInterval(timer)
})
</script>

<template>
    <main class="flex h-[calc(100vh_-_var(--top-bar-height))] flex-col p-4">
        <header class="mb-4 flex flex-wrap items-center justify-between gap-3">
            <h1 class="ysumc-heading m-0 text-2xl">云地图</h1>
            <div class="flex gap-3">
                <Button @click="reloadMap">刷新</Button>
                <Button v-if="url" @click="openUrl(url)">在浏览器打开</Button>
            </div>
        </header>
        <p v-if="error" role="status">{{ error }}</p>
        <iframe
            v-if="url"
            :key="`${url}:${frameKey}`"
            :src="url"
            title="云地图"
            sandbox="allow-scripts allow-same-origin allow-forms"
            referrerpolicy="no-referrer"
            class="min-h-0 w-full flex-1 border-0 bg-surface-2"
        />
        <div v-else class="flex flex-1 items-center justify-center bg-surface-2 p-8 text-secondary">
            管理员尚未配置云地图，发布后将自动显示。
        </div>
        <p v-if="url" class="mb-0 text-xs text-secondary">
            若地图网站不允许嵌入，请使用“在浏览器打开”。
        </p>
    </main>
</template>
