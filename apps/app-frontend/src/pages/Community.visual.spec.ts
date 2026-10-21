import '@/test/visual-styles.scss'
import '@/assets/stylesheets/ysumc.scss'

import { mount } from '@vue/test-utils'
import { afterEach, expect, it, vi } from 'vitest'
import { page } from 'vitest/browser'
import { h } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'

import Community from './Community.vue'

const calls = vi.hoisted(() => ({
    sync: vi.fn(),
    install: vi.fn(),
    invoke: vi.fn(),
    run: vi.fn(),
    status: vi.fn().mockResolvedValue({ players: { online: 7, max: 50 } }),
}))
vi.mock('@modrinth/ui', async () => ({
    Button: (await import('../../../../packages/ui/src/components/base/buttons/Button.vue'))
        .default,
    ButtonLink: (await import('../../../../packages/ui/src/components/base/buttons/ButtonLink.vue'))
        .default,
    SkinPreviewRenderer: { render: () => h('div', { 'data-testid': 'skin-character' }) },
    defineMessages: (messages: unknown) => messages,
    useVIntl: () => ({
        formatMessage: (message: { defaultMessage: string }) => message.defaultMessage,
    }),
}))
vi.mock('@/components/ui/modal/InstanceSettingsModal.vue', () => ({
    default: { render: () => null },
}))
vi.mock('@/components/ui/login/YsumcLoginGate.vue', () => ({ default: { render: () => null } }))
vi.mock('@/helpers/instance', () => ({ run: calls.run, get: vi.fn() }))
vi.mock('@/helpers/worlds', () => ({ get_server_status: calls.status }))
vi.mock('@/helpers/skins', () => ({
    get_available_skins: async () => [{ is_equipped: true, variant: 'SLIM' }],
    get_available_capes: async () => [],
    get_normalized_skin_texture: async () => 'data:image/png;base64,test',
}))
vi.mock('@tauri-apps/api/core', () => ({ invoke: calls.invoke }))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }))
vi.mock('@/helpers/auth', () => ({
    get_default_user: async () => null,
    users: async () => [],
    set_default_user: vi.fn(),
}))
vi.mock('@/helpers/managed', () => ({
    managed_sync: calls.sync,
    applyManagedInstanceAction: calls.install,
}))

let wrapper: ReturnType<typeof mount> | undefined
afterEach(() => {
    wrapper?.unmount()
    vi.clearAllMocks()
})

async function render() {
    document.documentElement.className = 'dark-mode'
    const router = createRouter({
        history: createMemoryHistory(),
        routes: [{ path: '/:pathMatch(.*)*', component: { render: () => null } }],
    })
    await router.push('/')
    wrapper = mount(Community, { attachTo: document.body, global: { plugins: [router] } })
    return wrapper
}

it('keeps downloads accessible when the placeholder service is offline', async () => {
    calls.sync.mockRejectedValueOnce(new Error('offline'))
    const view = await render()
    await vi.waitFor(() => expect(view.find('[role="alert"]').exists()).toBe(true))
    expect(view.get('a[href="/browse/modpack"]').text()).toBe('下载与发现')
    expect(view.get('a[href="/library"]').exists()).toBe(true)
    expect(view.text()).not.toContain('AI')
    expect(calls.invoke).not.toHaveBeenCalled()
})

it('renders official packs without forcing a download, including narrow windows', async () => {
    calls.sync.mockResolvedValue({
        schema: 1,
        offline: false,
        synced_at: 1792512000,
        instances: [
            {
                action: 'create',
                instance_id: null,
                applied_revision: null,
                spec: {
                    id: 'survival',
                    name: '燕大生存服',
                    description: '和朋友一起，从第一块方块开始。',
                    revision: 3,
                    minecraft: {
                        game_version: '1.21.1',
                        loader: 'fabric',
                        loader_version: '0.16.10',
                    },
                    server: { address: 'play.southmount.cn', port: 25565 },
                    pack: { size: 19289037 },
                },
            },
        ],
    })
    await page.viewport(1280, 960)
    const view = await render()
    await vi.waitFor(() => expect(view.text()).toContain('燕大生存服'))
    expect(calls.install).not.toHaveBeenCalled()
    expect(view.text()).toContain('play.southmount.cn')
    expect(view.text()).toContain('0.16.10')
    expect(view.text()).toContain('18.4 MiB')
    await vi.waitFor(() => expect(view.text()).toContain('7 / 50'))
    expect(view.text()).not.toContain('· r3')
    expect(view.text()).not.toContain('YSUMC / LAUNCHER')
    expect(view.text()).not.toContain('Block is small')
    expect(view.get('article').element.getBoundingClientRect().width).toBeGreaterThan(480)
    if (import.meta.env.VITE_AXOLOTL_VISUAL_EVIDENCE) {
        await page.screenshot({
            path: `${import.meta.env.VITE_AXOLOTL_VISUAL_EVIDENCE}/ysumc-launcher-desktop.png`,
        })
    }
    await page.viewport(620, 1100)
    expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(620)
    if (import.meta.env.VITE_AXOLOTL_VISUAL_EVIDENCE) {
        await page.screenshot({
            path: `${import.meta.env.VITE_AXOLOTL_VISUAL_EVIDENCE}/ysumc-launcher-narrow.png`,
        })
    }
})

it('opens installed cached instances and waits for a fresh catalog before installing', async () => {
    const spec = {
        id: 'survival',
        name: '燕大生存服',
        revision: 3,
        minecraft: { game_version: '1.21.1' },
    }
    calls.sync.mockResolvedValueOnce({
        schema: 1,
        offline: true,
        synced_at: 1792512000,
        instances: [
            { spec, action: 'current', instance_id: 'local-survival', applied_revision: 3 },
            {
                spec: { ...spec, id: 'creative', name: '创造服' },
                action: 'create',
                instance_id: null,
            },
        ],
    })
    const view = await render()
    await vi.waitFor(() => expect(view.text()).toContain('当前显示上次同步的清单'))
    const launch = view.findAll('button').find((button) => button.text() === '启动游戏')!
    expect(launch.exists()).toBe(true)
    await launch.trigger('click')
    expect(calls.run).toHaveBeenCalledWith('local-survival', null)
    const install = view.findAll('button').find((button) => button.text() === '安装整合包')!
    expect(install.attributes('disabled')).toBeDefined()
    await install.trigger('click')
    expect(calls.install).not.toHaveBeenCalled()

    calls.sync.mockResolvedValueOnce({
        schema: 1,
        offline: false,
        synced_at: 1792512600,
        instances: [{ spec, action: 'create', instance_id: null }],
    })
    await view
        .findAll('button')
        .find((button) => button.text() === '刷新清单')!
        .trigger('click')
    await vi.waitFor(() => expect(view.text()).not.toContain('当前显示上次同步的清单'))
    expect(
        view
            .findAll('button')
            .find((button) => button.text() === '安装整合包')!
            .attributes('disabled'),
    ).toBeUndefined()
})

it('does not offer reinstalling a retired pack from a cached snapshot', async () => {
    calls.sync.mockResolvedValueOnce({
        schema: 1,
        offline: true,
        synced_at: 1792512000,
        instances: [
            {
                action: 'retired',
                instance_id: 'old-game',
                spec: {
                    id: 'old',
                    name: '旧周目',
                    revision: 1,
                    minecraft: { game_version: '1.20.1' },
                },
            },
        ],
    })
    const view = await render()
    await vi.waitFor(() => expect(view.text()).toContain('已停止提供'))
    expect(
        view
            .findAll('button')
            .find((button) => button.text() === '已停止提供')!
            .attributes('disabled'),
    ).toBeDefined()
    expect(calls.install).not.toHaveBeenCalled()
})

it('shows the signed-in navy panel, character and no register action', async () => {
    const { ysumcAccount } = await import('@/helpers/ysumc-session')
    ysumcAccount.value = {
        account_id: 'ysu',
        account_type: 'yggdrasil',
        active: true,
        profile: { id: 'player', name: 'TestPlayer', skins: [], capes: [] },
        yggdrasil: {
            api_root: 'https://skin.ysumc.net/api/yggdrasil',
            server_name: 'YSUMC',
            login: 'test',
        },
    }
    calls.sync.mockResolvedValue({ instances: [], offline: false })
    const view = await render()
    await vi.waitFor(() => expect(view.find('[data-testid="skin-character"]').exists()).toBe(true))
    expect(view.text()).not.toContain('创建游戏账号')
    expect(view.text()).toContain('TestPlayer')
    expect(getComputedStyle(view.get('.ysumc-brand-panel').element).backgroundColor).toBe(
        'rgb(31, 41, 106)',
    )
    ysumcAccount.value = null
})
