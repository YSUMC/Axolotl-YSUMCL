import { mount } from '@vue/test-utils'
import { afterEach, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'

import CapeLibrary from './CapeLibrary.vue'

const api = vi.hoisted(() => ({ upload: vi.fn(), equip: vi.fn(), remove: vi.fn() }))
vi.mock('@/helpers/skins', () => ({
    upload_cape: api.upload,
    equip_cape: api.equip,
    remove_cape: api.remove,
    getCapePreviewUrl: (url: string) => url,
}))
vi.mock('@modrinth/ui', async () => {
    const { defineComponent, h } = await import('vue')
    return {
        defineMessages: (messages: unknown) => messages,
        useVIntl: () => ({
            formatMessage: (message: { defaultMessage: string }) => message.defaultMessage,
        }),
        Button: defineComponent({
            setup(_, { slots, attrs }) {
                return () => h('button', { type: 'button', ...attrs }, slots.default?.())
            },
        }),
        ConfirmModal: defineComponent({
            setup(_, { expose }) {
                expose({ show() {} })
                return () => null
            },
        }),
    }
})
const wrappers: ReturnType<typeof mount>[] = []
afterEach(() => {
    wrappers.splice(0).forEach((wrapper) => wrapper.unmount())
    vi.resetAllMocks()
})
function render(capes: unknown[] = []) {
    const wrapper = mount(CapeLibrary, {
        props: { accountId: 'ysumc:test', capes: capes as never[] },
    })
    wrappers.push(wrapper)
    return wrapper
}
async function pngFile(width = 64, height = 32) {
    const canvas = document.createElement('canvas')
    canvas.width = width
    canvas.height = height
    const blob = await new Promise<Blob>((resolve) =>
        canvas.toBlob((blob) => resolve(blob!), 'image/png'),
    )
    return new File([blob], 'My cape.png', { type: 'image/png' })
}
async function selectFile(wrapper: ReturnType<typeof render>, file: File) {
    const input = wrapper.get('input[type=file]')
    const transfer = new DataTransfer()
    transfer.items.add(file)
    ;(input.element as HTMLInputElement).files = transfer.files
    await input.trigger('change')
}
it('uploads a valid cape to the chosen account without silently equipping it', async () => {
    api.upload.mockResolvedValue({ id: 'new-cape' })
    const wrapper = render()
    await selectFile(wrapper, await pngFile())
    await vi.waitFor(() => expect(wrapper.find('form').exists()).toBe(true))
    await wrapper.get('form').trigger('submit')
    await vi.waitFor(() => expect(wrapper.emitted('changed')).toHaveLength(1))
    expect(api.upload).toHaveBeenCalledWith('ysumc:test', expect.any(Uint8Array), 'My cape')
    expect(api.equip).not.toHaveBeenCalled()
    expect(wrapper.find('form').exists()).toBe(false)
})
it('rejects skin-shaped images before sending an upload', async () => {
    const wrapper = render()
    await selectFile(wrapper, await pngFile(64, 64))
    await vi.waitFor(() => expect(wrapper.find('[role=alert]').exists()).toBe(true))
    expect(api.upload).not.toHaveBeenCalled()
})
it('equips the selected cape and surfaces server errors without reporting success', async () => {
    api.equip.mockRejectedValue(new Error('Site unavailable'))
    const wrapper = render([{ id: 'cape', name: 'Cape', texture: '', is_equipped: false }])
    await wrapper
        .findAll('button')
        .find((button) => button.text() === 'Wear')!
        .trigger('click')
    await vi.waitFor(() => expect(wrapper.get('[role=alert]').text()).toContain('Site unavailable'))
    expect(api.equip).toHaveBeenCalledWith('ysumc:test', 'cape')
    expect(wrapper.emitted('changed')).toBeUndefined()
    await nextTick()
})
