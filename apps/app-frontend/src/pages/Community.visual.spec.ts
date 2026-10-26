import '@/test/visual-styles.scss'
import '@/assets/stylesheets/ysumc.scss'

import { mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { page } from 'vitest/browser'
import { h } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'

import type { MinecraftAccount } from '@/helpers/minecraft-account'
import { ysumcAccount } from '@/helpers/ysumc-session'

import Community from './Community.vue'

const calls = vi.hoisted(() => ({
    sync: vi.fn(),
    install: vi.fn(),
    invoke: vi.fn(),
    run: vi.fn(),
    list: vi.fn(),
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
        formatMessage: (
            message: { defaultMessage: string },
            values: Record<string, unknown> = {},
        ) => message.defaultMessage.replace(/\{(\w+)\}/g, (_, key) => String(values[key] ?? key)),
    }),
}))
vi.mock('@/components/ui/modal/InstanceSettingsModal.vue', () => ({
    default: { render: () => null },
}))
vi.mock('@/components/ui/login/MicrosoftLoginGate.vue', () => ({ default: { render: () => null } }))
vi.mock('@/components/ui/login/YsumcLoginGate.vue', () => ({ default: { render: () => null } }))
vi.mock('@/helpers/instance', () => ({ run: calls.run, get: vi.fn() }))
vi.mock('@/helpers/events', () => ({ instance_listener: async () => () => {} }))
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
    managed_list: calls.list,
    applyManagedInstanceAction: calls.install,
}))

let wrapper: ReturnType<typeof mount> | undefined
beforeEach(() => {
    ysumcAccount.value = null
    vi.resetAllMocks()
    calls.list.mockResolvedValue([])
    calls.sync.mockResolvedValue({ instances: [], offline: false })
    calls.status.mockResolvedValue({ players: { online: 7, max: 50 } })
})
afterEach(() => {
    wrapper?.unmount()
    ysumcAccount.value = null
    vi.clearAllMocks()
})

async function render(partners = false) {
    document.documentElement.className = 'dark-mode'
    const router = createRouter({
        history: createMemoryHistory(),
        routes: [{ path: '/:pathMatch(.*)*', component: { render: () => null } }],
    })
    await router.push('/')
    wrapper = mount(Community, {
        props: { partners },
        attachTo: document.body,
        global: { plugins: [router] },
    })
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
                    pack: { kind: 'mrpack', size: 19289037 },
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
        pack: { kind: 'mrpack' },
        minecraft: { game_version: '1.21.1' },
    }
    calls.list.mockResolvedValue([
        { server_instance_id: 'survival', instance_id: 'local-survival', revision: 3 },
    ])
    calls.sync.mockResolvedValue({
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
    await vi.waitFor(() => expect(calls.run).toHaveBeenCalledWith('local-survival', null))
    const install = view.findAll('button').find((button) => button.text() === '安装整合包')!
    expect(install.attributes('disabled')).toBeDefined()
    await install.trigger('click')
    expect(calls.install).not.toHaveBeenCalled()

    calls.list.mockResolvedValue([])
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
                    pack: { kind: 'mrpack' },
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

it('renders peer sources, stale metadata and server-only actions while disabling unsupported formats', async () => {
    const base = {
        id: 'sitmc:survival',
        name: 'SIT Survival',
        revision: 12,
        minecraft: { game_version: '26.2', loader: 'fabric', loader_version: '0.19.5' },
        source: {
            siteId: 'sitmc',
            domain: 'skin.sitmc.club',
            name: 'SIT-Minecraft',
            description: '上海应用技术大学Minecraft社',
            stale: true,
        },
        server: { address: 'play.sitmc.club', port: null },
        pack: { kind: 'mrpack', size: 100 },
    }
    calls.sync.mockResolvedValue({
        instances: [
            { spec: base, action: 'create' },
            {
                spec: {
                    ...base,
                    id: 'sitmc:plain',
                    name: 'Plain server',
                    pack: { kind: 'server' },
                },
                action: 'create',
            },
            {
                spec: {
                    ...base,
                    id: 'sitmc:zip',
                    name: 'Other pack',
                    pack: { kind: 'curseforge-zip' },
                },
                action: 'create',
            },
        ],
        offline: false,
    })
    const view = await render(true)
    await vi.waitFor(() => expect(view.findAll('article')).toHaveLength(3))
    const cards = view.findAll('article')
    expect(view.text()).toContain('SIT-Minecraft')
    expect(view.text()).toContain('上海应用技术大学Minecraft社')
    expect(view.get('a[href="https://skin.sitmc.club"]').text()).toBe('skin.sitmc.club')
    expect(view.find('.ysumc-portal').exists()).toBe(false)
    expect(view.text()).toContain('1 clubs · 3 entries')
    expect(cards[0].text()).toContain('temporarily unavailable')
    expect(cards[1].get('button').text()).toBe('Prepare game')
    expect(cards[2].get('button').attributes('disabled')).toBeDefined()
    const buttonBounds = cards[2].get('button').element.getBoundingClientRect()
    const labelBounds = cards[2].get('button span').element.getBoundingClientRect()
    expect(labelBounds.top).toBeGreaterThanOrEqual(buttonBounds.top)
    expect(labelBounds.bottom).toBeLessThanOrEqual(buttonBounds.bottom)
    expect(calls.status).toHaveBeenCalledWith('play.sitmc.club')
    await cards[0].get('button').trigger('click')
    await vi.waitFor(() => expect(calls.install).toHaveBeenCalled())
    expect(calls.install.mock.calls[0][0].spec.id).toBe('sitmc:survival')
    await page.viewport(620, 1100)
    expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(620)
    if (import.meta.env.VITE_AXOLOTL_VISUAL_EVIDENCE) {
        await page.screenshot({
            path: `${import.meta.env.VITE_AXOLOTL_VISUAL_EVIDENCE}/ysumc-partners-narrow.png`,
        })
        await page.viewport(1440, 1000)
        await page.screenshot({
            path: `${import.meta.env.VITE_AXOLOTL_VISUAL_EVIDENCE}/ysumc-partners-desktop.png`,
        })
    }
})

it('refreshes the catalog before launch and blocks a newly outdated instance', async () => {
    const spec = {
        id: 'survival',
        name: 'YSU',
        revision: 1,
        minecraft: { game_version: '26.3' },
        pack: { kind: 'mrpack' },
    }
    calls.list.mockResolvedValue([
        { server_instance_id: 'survival', instance_id: 'local', revision: 1 },
    ])
    calls.sync.mockResolvedValueOnce({ instances: [{ spec, action: 'current' }], offline: false })
    calls.sync.mockResolvedValueOnce({
        instances: [{ spec: { ...spec, revision: 2 }, action: 'update' }],
        offline: false,
    })
    const view = await render()
    await vi.waitFor(() =>
        expect(view.findAll('button').some((b) => b.text() === '启动游戏')).toBe(true),
    )
    await view
        .findAll('button')
        .find((b) => b.text() === '启动游戏')!
        .trigger('click')
    await vi.waitFor(() => expect(view.text()).toContain('更新整合包'))
    expect(calls.run).not.toHaveBeenCalled()
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

it('separates home and partner catalogs when switching tabs without remounting', async () => {
    const spec = {
        id: 'survival',
        name: 'YSUMC official',
        revision: 1,
        minecraft: { game_version: '26.3' },
        pack: { kind: 'mrpack' },
    }
    calls.sync.mockResolvedValue({
        instances: [
            { spec, action: 'create' },
            {
                spec: {
                    ...spec,
                    id: 'sitmc:survival',
                    name: 'Partner survival',
                    source: {
                        siteId: 'sitmc',
                        domain: 'skin.sitmc.club',
                        stale: false,
                    },
                },
                action: 'create',
            },
        ],
        offline: false,
    })
    const view = await render()
    await vi.waitFor(() => expect(view.text()).toContain('YSUMC official'))
    expect(view.text()).not.toContain('Partner survival')
    await view.setProps({ partners: true })
    expect(view.text()).not.toContain('YSUMC official')
    expect(view.text()).toContain('Partner survival')
    expect(view.get('header h3').text()).toBe('skin.sitmc.club')
    calls.sync.mockResolvedValue({ instances: [{ spec, action: 'create' }], offline: false })
    await view
        .findAll('button')
        .find((button) => button.text() === '刷新清单')!
        .trigger('click')
    await vi.waitFor(() => expect(view.text()).toContain('No partner servers or modpacks'))
    expect(view.text()).not.toContain('YSUMC official')
})

it('displays verified rank and department below the selected character', async () => {
    calls.invoke.mockResolvedValue({
        player: 'TestPlayer',
        role: 'minister',
        department: 'technology',
        tags: ['部长', '技术部'],
    })
    ysumcAccount.value = {
        account_id: 'member-account',
        account_type: 'yggdrasil',
        profile: { name: 'TestPlayer' },
    } as MinecraftAccount
    const view = await render()
    await vi.waitFor(() =>
        expect(view.get('[data-onboarding-id="ysumc-account"]').text()).toContain('技术部'),
    )
    const panel = view.get('[data-onboarding-id="ysumc-account"]')
    expect(panel.text()).toContain('部长')
    expect(panel.text()).toContain('TestPlayer')
    ysumcAccount.value = null
    await vi.waitFor(() => expect(panel.text()).not.toContain('技术部'))
})
