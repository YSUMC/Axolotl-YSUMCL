import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'

import { type Cape, get_skin_texture_data, type Skin } from '@/helpers/skins'

import EditSkinModal from './EditSkinModal.vue'

const api = vi.hoisted(() => ({
    invoke: vi.fn(),
    fetch: vi.fn(),
    handleError: vi.fn(),
    hide: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ invoke: api.invoke }))
vi.mock('@tauri-apps/plugin-http', () => ({ fetch: api.fetch }))
vi.mock('@modrinth/ui', async () => {
    const { defineComponent, h, ref } = await import('vue')
    return {
        defineMessages: (messages: unknown) => messages,
        commonMessages: { cancelButton: { defaultMessage: 'Cancel' } },
        useVIntl: () => ({
            formatMessage: (message: { defaultMessage: string }) => message.defaultMessage,
        }),
        injectNotificationManager: () => ({ handleError: api.handleError }),
        useScrollIndicator: () => ({
            showTopFade: ref(false),
            showBottomFade: ref(false),
            checkScrollState() {},
            forceCheck() {},
        }),
        NewModal: defineComponent({
            setup(_, { slots, expose }) {
                expose({ show() {}, hide: api.hide })
                return () => h('div', [slots.default?.(), slots.actions?.()])
            },
        }),
        Button: defineComponent({
            setup(_, { slots, attrs }) {
                return () => h('button', attrs, slots.default?.())
            },
        }),
        CapeLikeTextButton: defineComponent({
            setup(_, { slots, attrs }) {
                return () => h('button', attrs, slots.default?.())
            },
        }),
        CapeButton: defineComponent({
            props: ['name', 'texture', 'selected'],
            emits: ['select'],
            setup(props, { emit }) {
                return () =>
                    h('button', {
                        'aria-label': props.name,
                        'aria-pressed': props.selected,
                        'data-texture': props.texture,
                        onClick: () => emit('select'),
                    })
            },
        }),
        SkinPreviewRenderer: defineComponent({
            props: ['capeSrc'],
            setup(props) {
                return () => h('div', { 'data-preview-cape': props.capeSrc ?? '' })
            },
        }),
        RadioButtons: defineComponent({ setup: () => () => null }),
    }
})

const texture = 'https://skin.ysumc.net/textures/original-skin'
const png = Uint8Array.from(
    atob(
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aX1sAAAAASUVORK5CYII=',
    ),
    (char) => char.charCodeAt(0),
)
const skin: Skin = {
    texture_key: '651',
    texture,
    variant: 'SLIM',
    cape_id: 'old-cape',
    source: 'custom',
    is_equipped: true,
}
const cape: Cape = {
    id: 'new-cape',
    name: 'New cape',
    texture: 'https://skin.ysumc.net/textures/new-cape',
    is_equipped: false,
}
const wrappers: ReturnType<typeof mount>[] = []
beforeEach(() => {
    api.fetch.mockImplementation(async () => new Response(png))
    api.invoke.mockImplementation(async (command, args) => {
        if (command.endsWith('|normalize_skin_texture')) return png
        if (command.endsWith('|save_custom_skin')) {
            return { ...args.skin, cape_id: args.cape?.id }
        }
    })
})
afterEach(() => {
    wrappers.splice(0).forEach((wrapper) => wrapper.unmount())
    vi.restoreAllMocks()
    vi.resetAllMocks()
})

async function render() {
    const wrapper = mount(EditSkinModal, {
        props: { capes: [{ ...cape, id: 'old-cape', name: 'Old cape' }, cape] },
        global: { directives: { tooltip() {} } },
    })
    wrappers.push(wrapper)
    await wrapper.vm.show(new MouseEvent('click'), skin)
    return wrapper
}

it('saves a cape change even when WebView fetch rejects the site texture', async () => {
    const browserFetch = vi
        .spyOn(globalThis, 'fetch')
        .mockRejectedValue(new TypeError('Failed to fetch'))
    const wrapper = await render()
    await wrapper.get('[aria-label="New cape"]').trigger('click')
    expect(wrapper.get('[data-preview-cape]').attributes('data-preview-cape')).toBe(
        `${cape.texture}?ysumc-preview=2`,
    )
    expect(wrapper.get('[aria-label="New cape"]').attributes('data-texture')).toBe(
        `${cape.texture}?ysumc-preview=2`,
    )
    await wrapper
        .findAll('button')
        .find((button) => button.text() === 'Save skin')!
        .trigger('click')
    await flushPromises()

    expect(browserFetch).not.toHaveBeenCalled()
    expect(api.fetch).toHaveBeenCalledWith(texture, { signal: expect.any(AbortSignal) })
    expect(api.invoke).toHaveBeenCalledWith('plugin:minecraft-skins|save_custom_skin', {
        skin,
        textureBlob: png,
        variant: 'SLIM',
        cape,
        replaceTexture: false,
    })
    expect(api.invoke).toHaveBeenCalledWith('plugin:minecraft-skins|equip_skin', {
        skin: { ...skin, cape_id: cape.id },
    })
    expect(wrapper.emitted('saved')).toHaveLength(1)
    expect(api.handleError).not.toHaveBeenCalled()
    expect(api.hide).toHaveBeenCalledOnce()
})

it('saves removing a cape without replacing the original skin PNG', async () => {
    const wrapper = await render()
    await wrapper
        .findAll('button')
        .find((button) => button.text() === 'None')!
        .trigger('click')
    await wrapper
        .findAll('button')
        .find((button) => button.text() === 'Save skin')!
        .trigger('click')
    await flushPromises()
    expect(api.invoke).toHaveBeenCalledWith('plugin:minecraft-skins|save_custom_skin', {
        skin,
        textureBlob: png,
        variant: 'SLIM',
        cape: undefined,
        replaceTexture: false,
    })
    expect(wrapper.get('[data-preview-cape]').attributes('data-preview-cape')).toBe('')
    expect(wrapper.emitted('saved')).toHaveLength(1)
})

it('keeps edits available for retry when the texture server fails', async () => {
    api.fetch.mockResolvedValueOnce(new Response('unavailable', { status: 503 }))
    const wrapper = await render()
    await wrapper.get('[aria-label="New cape"]').trigger('click')
    const save = wrapper.findAll('button').find((button) => button.text() === 'Save skin')!
    await save.trigger('click')
    await flushPromises()
    expect(api.handleError).toHaveBeenCalledWith(
        expect.objectContaining({ message: expect.stringContaining('503') }),
    )
    expect(wrapper.emitted('saved')).toBeUndefined()
    expect(api.hide).not.toHaveBeenCalled()
    expect(api.invoke.mock.calls.some(([command]) => command.endsWith('|save_custom_skin'))).toBe(
        false,
    )
    expect(wrapper.get('[aria-label="New cape"]').attributes('aria-pressed')).toBe('true')
    expect(save.attributes('disabled')).toBeUndefined()
    await save.trigger('click')
    await flushPromises()
    expect(wrapper.emitted('saved')).toHaveLength(1)
})

it('reads uploaded PNG data locally without a native HTTP request', async () => {
    const data = `data:image/png;base64,${btoa(String.fromCharCode(...png))}`
    expect(await get_skin_texture_data(data)).toEqual(png)
    expect(api.fetch).not.toHaveBeenCalled()
})

it('does not route other texture providers through the site-only native permission', async () => {
    const browserFetch = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(png))
    const mojangTexture = 'https://textures.minecraft.net/texture/example'
    expect(await get_skin_texture_data(mojangTexture)).toEqual(png)
    expect(browserFetch).toHaveBeenCalledWith(mojangTexture, { signal: expect.any(AbortSignal) })
    expect(api.fetch).not.toHaveBeenCalled()
})
