const trimTrailingSlash = (url: string) => url.replace(/\/$/, '')

export const AxolotlBrandConfig = Object.freeze({
    productName: 'YSUMC Launcher',
    shortProductName: 'YSUMC',
    website: 'https://ysumc.net/',
    repositoryUrl: 'https://github.com/Mystic-Stars/Axolotl',
    supportUrl: 'https://ysumc.net/',
    qqGroupNumber: '',
    qqChannelUrl: 'https://ysumc.net/',
    sponsorUrl: 'https://ysumc.net/',
    bundleIdentifier: 'net.ysumc.launcher',
    deepLinkScheme: 'ysumc',
    userAgent: (version: string, os: string) => `ysumc/launcher/${version} (${os})`,
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
    manifestUrl: 'https://skin.ysumc.net/api/launcher/manifest',
    announcementsUrl: 'https://skin.ysumc.net/api/launcher/announcements',
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
