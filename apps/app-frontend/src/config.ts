import { launcherBrand, launcherServices } from './branding'

const trimTrailingSlash = (url: string) => url.replace(/\/$/, '')

export const AxolotlBrandConfig = Object.freeze({
    productName: launcherBrand.productName,
    shortProductName: launcherBrand.shortName,
    website: launcherBrand.website,
    repositoryUrl: 'https://github.com/Mystic-Stars/Axolotl',
    supportUrl: launcherBrand.website,
    qqGroupNumber: '',
    qqChannelUrl: launcherBrand.website,
    sponsorUrl: launcherBrand.website,
    bundleIdentifier: launcherBrand.identifier,
    deepLinkScheme: launcherBrand.scheme,
    userAgent: (version: string, os: string) => `${launcherBrand.id}/launcher/${version} (${os})`,
    capabilities: Object.freeze({
        publicModrinthApi: true,
        privateModrinthServices: false,
        ghsTelemetry: false,
        ai: false,
    }),
})

export const YsumcConfig = Object.freeze({
    siteUrl: 'https://skin.ysumc.net',
    websiteUrl: 'https://ysumc.net',
    manifestUrl: launcherServices.manifestUrl,
    announcementsUrl: launcherServices.announcementsUrl,
    yggdrasilApiRoot: 'https://skin.ysumc.net/api/yggdrasil',
})

const siteUrl = trimTrailingSlash(import.meta.env.MODRINTH_URL || 'https://modrinth.com')
const officialLabrinthBaseUrl = trimTrailingSlash(
    import.meta.env.MODRINTH_API_BASE_URL || 'https://api.modrinth.com',
)
type DownloadSourceMode = 'auto' | 'official_only' | 'mirror_preferred' | 'official_preferred'

// The Modrinth API always uses the official source; Modrinth download mirror
// routing is handled by the Rust download layer.
export function setModrinthSourceMode(_sourceMode: DownloadSourceMode) {}

export function setModrinthMirrorEnabled(_enabled: boolean) {}

export function getOfficialLabrinthBaseUrl() {
    return officialLabrinthBaseUrl
}

export function getLabrinthBaseUrl() {
    return officialLabrinthBaseUrl
}

export const config = {
    siteUrl,
    labrinthBaseUrl: getLabrinthBaseUrl,
}
