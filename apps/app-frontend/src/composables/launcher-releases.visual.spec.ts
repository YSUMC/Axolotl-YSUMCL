import { mount } from '@vue/test-utils'
import { afterEach, expect, it, vi } from 'vitest'
import { h } from 'vue'

import UpdateAnnouncementContent from '@/components/ui/announcement/UpdateAnnouncementContent.vue'
import UpdateAnnouncementHistory from '@/components/ui/announcement/UpdateAnnouncementHistory.vue'
import { useLauncherReleases } from '@/composables/useLauncherReleases'
import type { MinecraftAccount } from '@/helpers/minecraft-account'
import { ysumcAccount } from '@/helpers/ysumc-session'
import {
    getNextAppUpdatePopupTime,
    markAppUpdateActionable,
    markAppUpdatePopupShown,
} from '@/providers/app-update'

const mocks = vi.hoisted(() => ({ fetch: vi.fn(), open: vi.fn() }))
vi.mock('@/helpers/launcher-site', () => ({ launcherSiteDocument: mocks.fetch }))
vi.mock('@/helpers/ysumc-session', async () => ({ ysumcAccount: (await import('vue')).ref(null) }))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: mocks.open }))
vi.mock('@modrinth/ui', () => ({
    defineMessages: (value: unknown) => value,
    useVIntl: () => ({
        formatMessage: (message: { defaultMessage: string }, values: Record<string, string> = {}) =>
            message.defaultMessage.replace(/\{(\w+)\}/g, (_, key) => values[key] ?? ''),
    }),
    Admonition: { props: ['body'], setup: (props) => () => h('p', props.body) },
    Button: {
        setup:
            (_, { slots }) =>
            () =>
                h('button', slots.default?.()),
    },
    Accordion: {
        setup:
            (_, { slots }) =>
            () =>
                h('div', [slots.title?.(), slots.default?.()]),
    },
}))
afterEach(() => {
    localStorage.clear()
    vi.clearAllMocks()
})
const release = {
    version: '1.0.1',
    notes: '## 更新\n\n- 自动切源\n- 修复预览',
    published_at: '2026-10-21T10:48:00Z',
}

it('loads the site release history and does not show the unrelated Axolotl catalog', async () => {
    mocks.fetch.mockResolvedValue({ releases: [release] })
    const wrapper = mount(UpdateAnnouncementHistory, { props: { currentVersion: '1.0.1' } })
    await vi.waitFor(() => expect(wrapper.text()).toContain('自动切源'))
    expect(mocks.fetch.mock.calls[0][0]).toBe('releases')
    expect(wrapper.text()).toContain('YSUMC Launcher 1.0.1')
    expect(wrapper.text()).toContain('自动切源')
    expect(wrapper.text()).not.toContain('1.10.0-beta')
    expect(wrapper.text()).not.toContain('No release notes')
    wrapper.unmount()
})
it('clears unverifiable notes on failure and honors empty server history', async () => {
    const store = useLauncherReleases()
    store.releases.value = [release]
    mocks.fetch.mockRejectedValue(new Error('offline'))
    await store.refresh()
    expect(store.failed.value).toBe(true)
    expect(store.releases.value).toEqual([])
    mocks.fetch.mockResolvedValue({ releases: [] })
    await store.refresh()
    expect(store.failed.value).toBe(false)
    expect(store.releases.value).toEqual([])
    expect(localStorage.getItem('ysumc-launcher-releases-v1')).toBeNull()
})
it('renders server Markdown as safe content and opens allowed links externally', async () => {
    const wrapper = mount(UpdateAnnouncementContent, {
        props: {
            release: {
                ...release,
                notes:
                    release.notes +
                    '\n\n[官网](https://ysumc.club)\n\n<script>alert(1)</script><img src="x" onerror="alert(1)">',
            },
        },
    })
    expect(wrapper.findAll('li')).toHaveLength(2)
    expect(wrapper.find('script').exists()).toBe(false)
    expect(wrapper.find('[onerror]').exists()).toBe(false)
    await wrapper.find('a').trigger('click')
    expect(mocks.open).toHaveBeenCalledWith('https://ysumc.club/')
    wrapper.unmount()
})
it('shows an actionable update immediately and prompts only once for each stage', () => {
    const now = Date.now()
    markAppUpdateActionable('1.0.2', 'available', now)
    expect(getNextAppUpdatePopupTime('1.0.2', 'available')).toBe(now)
    markAppUpdatePopupShown('1.0.2', 'available', now)
    expect(getNextAppUpdatePopupTime('1.0.2', 'available')).toBeNull()
    markAppUpdateActionable('1.0.2', 'downloaded', now + 100)
    expect(getNextAppUpdatePopupTime('1.0.2', 'downloaded')).toBe(now + 100)
})

it('discards a previous account response that arrives after account switching', async () => {
    const store = useLauncherReleases()
    let finish!: (value: unknown) => void
    mocks.fetch.mockReturnValueOnce(
        new Promise((resolve) => {
            finish = resolve
        }),
    )
    const old = store.refresh()
    mocks.fetch.mockResolvedValue({ releases: [] })
    ysumcAccount.value = { account_id: 'next-account' } as MinecraftAccount
    expect(store.releases.value).toEqual([])
    await vi.waitFor(() => expect(store.loading.value).toBe(false))
    finish({ releases: [release] })
    await old
    expect(store.releases.value).toEqual([])
})
