import { VueQueryPlugin } from '@tanstack/vue-query'
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks'
import { createPinia } from 'pinia'
import { createApp } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'

import { previewState } from './state'

// This entry is not imported by index.html and is excluded from production builds.
if (!import.meta.env.DEV) throw new Error('The design preview is development-only.')

const demo = {
    id: 'mutmc-demo',
    name: 'MUT 官方整合包 · 演示',
    description: '这是用于预览布局的示例。正式整合包发布后，将自动显示名称、介绍和服务器信息。',
    icon_url: '/brands/mutmc/icon.webp',
    sort: 0,
    revision: 1,
    minecraft: { game_version: '1.21.1', loader: 'fabric', loader_version: '0.16.10' },
    pack: {
        kind: 'mrpack',
        url: 'https://example.invalid/demo.mrpack',
        sha1: '0'.repeat(40),
        size: 150 * 1024 * 1024,
    },
    server: { address: 'preview.example.invalid', port: null, opened_on: '2026-10-01' },
    required: false,
}
mockWindows('main')
mockIPC(
    (cmd) => {
        switch (cmd) {
            case 'plugin:instance|managed_sync':
                return {
                    schema: 1,
                    offline: false,
                    synced_at: Date.now() / 1000,
                    cloud_map_url: null,
                    instances:
                        previewState.catalog === 'empty'
                            ? []
                            : [
                                  {
                                      spec: demo,
                                      action: 'create',
                                      instance_id: null,
                                      applied_revision: null,
                                  },
                              ],
                }
            case 'plugin:instance|managed_list':
                return previewState.catalog === 'installed'
                    ? [
                          {
                              server_instance_id: demo.id,
                              instance_id: 'preview-instance',
                              revision: 1,
                              target_revision: 1,
                              name: demo.name,
                              retired: false,
                          },
                      ]
                    : []
            case 'plugin:worlds|get_server_status':
                return { players: { online: 24, max: 100 } }
            case 'plugin:minecraft-skins|get_available_skins':
            case 'plugin:minecraft-skins|get_available_capes':
                return []
            case 'plugin:opener|open_url':
                previewState.notice = '这是页面预览；外部链接将在正式启动器中打开。'
                return
            default:
                previewState.notice =
                    '当前是设计预览，不会登录账号、下载整合包或启动游戏。可以用顶部选项查看不同状态。'
                throw new Error('Design preview: native action is unavailable.')
        }
    },
    { shouldMockEvents: true },
)

async function bootPreview() {
    const root = document.getElementById('app')!
    root.textContent = '正在载入启动器界面…'
    const [{ default: Preview }, { default: i18n }, { setAppLocale }, { ysumcAccount }] =
        await Promise.all([
            import('./Preview.vue'),
            import('@/plugins/i18n'),
            import('@/i18n.config'),
            import('@/helpers/ysumc-session'),
        ])
    ysumcAccount.value = {
        account_id: 'preview',
        account_type: 'microsoft',
        active: true,
        profile: { id: 'preview', name: 'MUT_Player', skins: [], capes: [] },
    }
    root.textContent = '正在载入界面语言…'
    await setAppLocale('zh-CN')
    root.textContent = '正在打开预览…'
    const router = createRouter({
        history: createMemoryHistory(),
        routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }],
    })
    const app = createApp(Preview)
    app.use(createPinia()).use(VueQueryPlugin).use(router).use(i18n)
    await router.push('/')
    app.mount('#app')
}
void bootPreview().catch((error) => {
    document.getElementById('app')!.textContent = `预览载入失败：${String(error)}`
    console.error(error)
})
