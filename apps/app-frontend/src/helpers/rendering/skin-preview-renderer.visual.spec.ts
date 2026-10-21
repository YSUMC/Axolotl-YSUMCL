import * as THREE from 'three'
import { afterEach, expect, it, vi } from 'vitest'

import type { Skin } from '../skins'
import { setActiveSkinPreviews, skinBlobUrlMap, skinPreviewFailures } from './batch-skin-renderer'
import { cancelSkinPreviews, generateSkinPreviews } from './skin-preview-renderer'

const mocks = vi.hoisted(() => ({ normalize: vi.fn(), retrieve: vi.fn() }))
vi.mock('../skins', () => ({
    get_normalized_skin_texture: mocks.normalize,
    determineModelType: async () => 'CLASSIC',
}))
vi.mock('../storage/skin-preview-storage', () => ({
    skinPreviewStorage: {
        batchRetrieve: mocks.retrieve,
        store: vi.fn(),
        cleanupInvalidKeys: vi.fn(),
    },
}))
vi.mock('../storage/head-storage', () => ({
    headStorage: {
        batchRetrieve: async () => ({}),
        retrieve: async () => 'blob:head',
        cleanupInvalidKeys: vi.fn(),
    },
}))
vi.mock('@modrinth/ui', () => ({
    loadSkinRendering: async () => ({
        setupSkinModel: async () => {
            const model = new THREE.Group()
            const head = new THREE.Object3D()
            head.name = 'Head'
            model.add(head)
            return { model }
        },
        applyCapeTexture: vi.fn(),
        createTransparentTexture: () => new THREE.Texture(),
        disposeCaches: vi.fn(),
    }),
}))

afterEach(() => {
    cancelSkinPreviews()
    setActiveSkinPreviews([])
    vi.clearAllMocks()
})

it('continues past a broken fourth texture and retries it without rerendering successful skins', async () => {
    const skins = Array.from(
        { length: 7 },
        (_, i) => ({ texture_key: String(i), variant: 'CLASSIC' }) as Skin,
    )
    mocks.retrieve.mockResolvedValue({})
    mocks.normalize.mockImplementation(async (skin: Skin) => {
        if (skin.texture_key === '3') throw new Error('Invalid skin dimensions')
        return 'data:image/png;base64,test'
    })
    await generateSkinPreviews(skins, [])
    expect(skinBlobUrlMap.size).toBe(6)
    expect(skinBlobUrlMap.has('6+CLASSIC+no-cape')).toBe(true)
    expect([...skinPreviewFailures]).toEqual(['3+CLASSIC+no-cape'])

    mocks.normalize.mockClear().mockResolvedValue('data:image/png;base64,repaired')
    await generateSkinPreviews(skins, [])
    expect(mocks.normalize).toHaveBeenCalledTimes(1)
    expect(skinBlobUrlMap.size).toBe(7)
    expect(skinPreviewFailures.size).toBe(0)
})

it('still renders when the preview cache cannot be read', async () => {
    mocks.retrieve.mockRejectedValue(new Error('IndexedDB unavailable'))
    mocks.normalize.mockResolvedValue('data:image/png;base64,test')
    await generateSkinPreviews([{ texture_key: '1', variant: 'CLASSIC' } as Skin], [])
    expect(skinBlobUrlMap.has('1+CLASSIC+no-cape')).toBe(true)
})
