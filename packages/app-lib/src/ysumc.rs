//! Endpoints of the YSUMC services this launcher build talks to.
//!
//! YSUMC's account provider and official instance catalog are pinned here.
//! Microsoft, offline and other Yggdrasil accounts remain independently available,
//! as do ordinary instances and resource downloads.
//!
//! See the workspace's YSUMC development document for the integration contract.

/// Public site of the society's Blessing Skin deployment.
pub const SITE_URL: &str = "https://skin.ysumc.net";

/// Public catalog of official packs.
pub const MANIFEST_URL: &str = "https://skin.ysumc.net/api/launcher/manifest";

/// Published home-site catalog. Friend instances are read separately.
pub const INTEROP_URL: &str =
    "https://skin.ysumc.net/.well-known/mc-interop-v1.json";

/// Direct friends aggregated by the home server; never crawl peers on the client.
pub const FRIENDS_URL: &str = "https://skin.ysumc.net/api/launcher/friends";

/// Authenticated proxy to the official CurseForge API; its key stays on the server.
pub const CURSEFORGE_PROXY_URL: &str =
    "https://skin.ysumc.net/api/launcher/curseforge";

/// Where a player creates the account the launcher accepts.
pub const REGISTER_URL: &str = "https://skin.ysumc.net/auth/register";

/// Where a player signs in with a browser.
pub const LOGIN_URL: &str = "https://skin.ysumc.net/auth/login";

/// Yggdrasil (authlib-injector compatible) API root of the skin site.
pub const YGGDRASIL_API_ROOT: &str = "https://skin.ysumc.net/api/yggdrasil";

/// Base URL of the Janus OpenID Connect provider behind the skin site.
pub const JANUS_BASE_URL: &str = "https://skin.ysumc.net/api/janus";

/// Human-readable name of the account site, used when the site does not report
/// one itself.
pub const SITE_LABEL: &str = "YSUMC";

/// The society's documentation, as shown in the launcher's Documentation page.
///
/// Opened as an ordinary website; it does not participate in account login.
pub const DOCS_URL: &str = "https://ysumc.net";

/// Client ID the site reserves for launchers (`shared_client_id` in the
/// OpenID provider metadata). The metadata is preferred when it announces one.
pub const OIDC_CLIENT_ID: &str = "";

/// Scopes requested when starting a device authorization grant.
///
/// `Yggdrasil.PlayerProfiles.Select` makes the authorization page ask the player
/// which character to play as and binds the issued access token to it;
/// `Yggdrasil.Server.Join` is what lets that token join a Minecraft server. The
/// Yggdrasil Connect specification requires the joined server to be paired with
/// `Select`, and forbids pairing `Select` with `PlayerProfiles.Read`.
pub const OIDC_SCOPE: &str = "openid offline_access Yggdrasil.PlayerProfiles.Select Yggdrasil.Server.Join";

/// OAuth grant type of the device authorization grant (RFC 8628).
pub const OIDC_DEVICE_CODE_GRANT_TYPE: &str =
    "urn:ietf:params:oauth:grant-type:device_code";

/// Whether an API root addresses the society's skin site.
///
/// Compared after normalizing away a trailing slash so stored accounts created
/// by earlier builds keep working.
pub fn is_yggdrasil_api_root(api_root: &str) -> bool {
    api_root.trim().trim_end_matches('/') == YGGDRASIL_API_ROOT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_ysumc_api_root() {
        assert!(is_yggdrasil_api_root(YGGDRASIL_API_ROOT));
        assert!(is_yggdrasil_api_root(&format!("{YGGDRASIL_API_ROOT}/")));
        assert!(is_yggdrasil_api_root(&format!(" {YGGDRASIL_API_ROOT} ")));
    }

    #[test]
    fn rejects_other_api_roots() {
        assert!(!is_yggdrasil_api_root(
            "https://littleskin.cn/api/yggdrasil"
        ));
        assert!(!is_yggdrasil_api_root(""));
        assert!(!is_yggdrasil_api_root("https://skin.ysumc.net"));
    }
}
