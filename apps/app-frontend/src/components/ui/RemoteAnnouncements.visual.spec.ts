import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { h, ref } from 'vue'

import { OPEN_REMOTE_ANNOUNCEMENT_CENTER_EVENT } from '@/helpers/remote-announcements'

import RemoteAnnouncements from './RemoteAnnouncements.vue'

const mocks = vi.hoisted(() => ({ fetch: vi.fn(), show: vi.fn() }))
vi.mock('@/helpers/launcher-site', () => ({ launcherSiteDocument: mocks.fetch }))
vi.mock('@/helpers/ysumc-session', async () => ({ ysumcAccount: (await import('vue')).ref(null) }))
vi.mock('@tauri-apps/api/app', () => ({ getVersion: async () => '1.0.5' }))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }))
vi.mock('@/helpers/settings', () => ({ getUpdateChannel: async () => 'release' }))
vi.mock('@modrinth/ui', () => ({
    defineMessages: (value: unknown) => value,
    useVIntl: () => ({
        formatMessage: (message: { defaultMessage: string }) => message.defaultMessage,
    }),
    useModalStack: () => ({ hasModal: ref(false) }),
    commonMessages: { closeButton: { defaultMessage: 'Close' } },
    Button: {
        setup:
            (_, { slots }) =>
            () =>
                h('button', slots.default?.()),
    },
    NewModal: {
        setup: (_, { slots, expose }) => {
            expose({ show: mocks.show })
            return () => h('div', [slots.default?.(), slots.actions?.()])
        },
    },
}))

let wrapper: ReturnType<typeof mount> | undefined
const notice = {
    id: 'cloud-1',
    title: '云端公告',
    summary: '公告摘要',
    content: '## 活动安排\n\n新的内容',
    type: 'modal',
    priority: 'normal',
    starts_at: '2020-01-01T00:00:00Z',
    ends_at: null,
    published_at: '2020-01-01T00:00:00Z',
    action_url: null,
    action_label: null,
}
function respond(items = [notice]) {
    mocks.fetch.mockImplementation(async () => items)
}
beforeEach(() => {
    localStorage.clear()
    vi.useFakeTimers()
    vi.stubGlobal('requestIdleCallback', (callback: () => void) => {
        callback()
        return 1
    })
    respond()
})
afterEach(() => {
    wrapper?.unmount()
    vi.useRealTimers()
    vi.unstubAllGlobals()
    vi.clearAllMocks()
})

it('uses native HTTP, waits for login/startup gates and displays cloud Markdown', async () => {
    const browserFetch = vi.fn(() => {
        throw new Error('browser fetch is blocked by CSP')
    })
    vi.stubGlobal('fetch', browserFetch)
    wrapper = mount(RemoteAnnouncements, { props: { ready: false } })
    await flushPromises()
    expect(mocks.fetch).toHaveBeenCalledWith('announcements', '1.0.5', 'stable')
    expect(browserFetch).not.toHaveBeenCalled()
    expect(mocks.show).not.toHaveBeenCalled()
    await wrapper.setProps({ ready: true })
    await vi.advanceTimersByTimeAsync(350)
    expect(mocks.show).toHaveBeenCalledOnce()
    expect(wrapper.text()).toContain('云端公告')
    expect(wrapper.find('.markdown-body h2').text()).toBe('活动安排')
})

it('refreshes an open empty center, clears unverifiable content on failure and removes withdrawn notices', async () => {
    respond([])
    wrapper = mount(RemoteAnnouncements, { props: { ready: true } })
    await flushPromises()
    window.dispatchEvent(new Event(OPEN_REMOTE_ANNOUNCEMENT_CENTER_EVENT))
    await flushPromises()
    expect(wrapper.text()).toContain('No announcements')
    respond()
    await vi.advanceTimersByTimeAsync(60000)
    expect(wrapper.text()).toContain('云端公告')
    mocks.fetch.mockRejectedValue(new Error('offline'))
    window.dispatchEvent(new Event('online'))
    await flushPromises()
    expect(wrapper.text()).not.toContain('云端公告')
    expect(wrapper.text()).toContain('Could not refresh announcements')
    respond([])
    const refresh = wrapper
        .findAll('button')
        .find((button) => button.text() === 'Refresh announcements')!
    await refresh.trigger('click')
    await flushPromises()
    expect(wrapper.text()).not.toContain('云端公告')
    expect(wrapper.text()).toContain('No announcements')
    expect(wrapper.text()).not.toContain('Could not refresh announcements')
})

it('does not fetch for a local style preview or after unmount', async () => {
    wrapper = mount(RemoteAnnouncements, { props: { ready: true, previewOnly: true } })
    await flushPromises()
    window.dispatchEvent(new Event('online'))
    await vi.advanceTimersByTimeAsync(60000)
    expect(mocks.fetch).not.toHaveBeenCalled()
    wrapper.unmount()
    wrapper = mount(RemoteAnnouncements, { props: { ready: true } })
    await flushPromises()
    wrapper.unmount()
    wrapper = undefined
    const count = mocks.fetch.mock.calls.length
    window.dispatchEvent(new Event('online'))
    await vi.advanceTimersByTimeAsync(60000)
    expect(mocks.fetch).toHaveBeenCalledTimes(count)
})
