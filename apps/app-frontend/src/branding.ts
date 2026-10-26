import catalog from '../../../brands/catalog.json'

const brandId = import.meta.env.VITE_LAUNCHER_BRAND || 'ysumc'
if (brandId !== 'ysumc' && brandId !== 'mutmc') {
    throw new Error(`Unknown launcher brand: ${brandId}`)
}

export const launcherBrand = Object.freeze(catalog[brandId])
export const isMutmc = launcherBrand.id === 'mutmc'
export const launcherServices = Object.freeze({
    manifestUrl: `${launcherBrand.apiBase}/manifest`,
    announcementsUrl: `${launcherBrand.apiBase}/announcements`,
    releasesUrl: `${launcherBrand.apiBase}/update/releases`,
})
