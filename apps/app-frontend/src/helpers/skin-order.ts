import type { Skin } from './skins'

/** Blessing Skin texture IDs increase with upload time. Built-in skins keep their order. */
export function newestSiteSkinsFirst(skins: Skin[]): Skin[] {
    const uploaded = skins.filter((skin) => skin.source !== 'default')
    uploaded.sort((a, b) => Number(b.texture_key) - Number(a.texture_key))
    return [...uploaded, ...skins.filter((skin) => skin.source === 'default')]
}
