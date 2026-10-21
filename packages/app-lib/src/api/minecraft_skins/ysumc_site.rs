//! Skin management against the society's Blessing Skin site.
//!
//! Microsoft accounts own their skin on Mojang, but a society account is signed in
//! with Yggdrasil Connect, where the skin lives on the society's skin site. That
//! site exposes the launcher-facing endpoints this module calls, authenticated with
//! the very token the launcher already holds; the site's own OpenID provider is the
//! authority on whether that token is still valid, so the launcher never has to
//! know how the site stores players.
//!
//! Endpoints (see `docs/launcher-protocol.md` §7):
//! - `GET  {site}/api/launcher/skins?player=<name>`
//! - `POST {site}/api/launcher/skins?player=<name>&type=<steve|alex>&name=<label>`
//!   with the PNG as the request body
//! - `PUT  {site}/api/launcher/skins/equip` with `{player, skin?, cape?}`
//! - `DELETE {site}/api/launcher/skins` with `{player, tid}`

use bytes::Bytes;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;
use url::Url;

use crate::ErrorKind;
use crate::state::{Credentials, MinecraftSkinVariant};
use crate::util::fetch::INSECURE_REQWEST_CLIENT;
use crate::ysumc;

use super::{Cape, Skin, SkinSource};

#[derive(Debug, Deserialize)]
struct SiteTextureView {
    tid: i64,
    name: String,
    #[serde(rename = "type")]
    type_: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SitePlayerView {
    name: String,
    #[serde(default)]
    skin: Option<SiteTextureView>,
    #[serde(default)]
    cape: Option<SiteTextureView>,
}

#[derive(Debug, Deserialize)]
struct SiteSkinsResponse {
    player: SitePlayerView,
    #[serde(default)]
    textures: Vec<SiteTextureView>,
}

#[derive(Debug, Deserialize)]
struct SiteUploadResponse {
    texture: SiteTextureView,
}

#[derive(Debug, Deserialize)]
struct SiteErrorResponse {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl SiteTextureView {
    fn variant(&self) -> MinecraftSkinVariant {
        if self.type_ == "alex" || self.model.as_deref() == Some("slim") {
            MinecraftSkinVariant::Slim
        } else {
            MinecraftSkinVariant::Classic
        }
    }

    fn is_cape(&self) -> bool {
        self.type_ == "cape"
    }

    fn texture_url(&self) -> Option<Arc<Url>> {
        self.url
            .as_deref()
            .and_then(|url| Url::parse(url).ok())
            .map(Arc::new)
    }

    fn into_skin(
        self,
        equipped: bool,
        cape_id: Option<uuid::Uuid>,
    ) -> Option<Skin> {
        let texture_key = Arc::from(self.tid.to_string());
        let name = Arc::from(self.name.as_str());
        let variant = self.variant();
        let texture = self.texture_url()?;

        Some(Skin {
            texture_key,
            name: Some(name),
            section: None,
            variant,
            cape_id,
            texture,
            source: SkinSource::Custom,
            is_equipped: equipped,
        })
    }

    fn into_cape(self, equipped: bool) -> Option<Cape> {
        Some(Cape {
            id: cape_id(self.tid),
            name: Arc::from(self.name.as_str()),
            texture: self.texture_url()?,
            is_equipped: equipped,
        })
    }
}

/// The site identifies textures with its own numeric id; the launcher models a cape
/// by UUID, so derive a stable one from that id. Only the first half is used so the
/// round trip through [`cape_texture_id`] is exact.
fn cape_id(tid: i64) -> uuid::Uuid {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&tid.to_be_bytes());
    uuid::Uuid::from_bytes(bytes)
}

fn tid_of(texture_key: &str) -> Option<i64> {
    texture_key.parse::<i64>().ok().filter(|tid| *tid > 0)
}

pub struct SiteSkins {
    pub player_name: String,
    pub equipped_skin_tid: Option<i64>,
    pub equipped_cape_tid: Option<i64>,
    pub skins: Vec<Skin>,
    pub capes: Vec<Cape>,
}

impl Clone for SiteSkins {
    fn clone(&self) -> Self {
        Self {
            player_name: self.player_name.clone(),
            equipped_skin_tid: self.equipped_skin_tid,
            equipped_cape_tid: self.equipped_cape_tid,
            skins: self.skins.clone(),
            capes: self.capes.clone(),
        }
    }
}

/// How long a listed closet is reused before the site is asked again.
///
/// The site throttles its launcher endpoints per account (`throttle:30,1`), and
/// several parts of the launcher list a character's textures while a page mounts.
/// Hammering the endpoint spends that whole budget in seconds and then every
/// request is refused for a minute, which is exactly the "my skins disappeared"
/// report. Skins and capes change far less often than that, so a short reuse
/// window costs nothing while keeping the budget for real writes.
const CACHE_TTL: Duration = Duration::from_secs(20);

/// The launcher never waits on the skin site forever. A page that lists a closet
/// has to render even when the site stops answering, so every listing is bounded
/// and a timeout is reported like any other site failure.
const LIST_TIMEOUT: Duration = Duration::from_secs(20);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct ClosetCache {
    generation: u64,
    entry: Option<([u8; 32], Instant, SiteSkins)>,
}

static CACHE: Mutex<ClosetCache> = Mutex::new(ClosetCache {
    generation: 0,
    entry: None,
});
// A cancelled request drops this guard automatically; later readers can retry.
static LIST_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

fn cache_key(account: &str, player: &str, token: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    for part in [account, player, token] {
        hash.update(part.len().to_be_bytes());
        hash.update(part.as_bytes());
    }
    hash.finalize().into()
}

fn cached_skins(key: &[u8; 32]) -> Option<SiteSkins> {
    let guard = CACHE.lock().ok()?;
    let (stored_key, fetched_at, skins) = guard.entry.as_ref()?;

    (stored_key == key && fetched_at.elapsed() < CACHE_TTL)
        .then(|| skins.clone())
}

fn store_skins(key: [u8; 32], generation: u64, skins: &SiteSkins) {
    if let Ok(mut guard) = CACHE.lock() {
        // Do not restore a stale response that raced an equip/upload/delete.
        if guard.generation == generation {
            guard.entry = Some((key, Instant::now(), skins.clone()));
        }
    }
}

fn endpoint(path: &str) -> String {
    format!("{}{}", ysumc::SITE_URL.trim_end_matches('/'), path)
}

fn access_token(credentials: &Credentials) -> crate::Result<&str> {
    if credentials.access_token.trim().is_empty() {
        return Err(ErrorKind::NoCredentialsError.into());
    }
    Ok(credentials.access_token.as_str())
}

async fn site_error(response: reqwest::Response) -> crate::Error {
    let status = response.status();
    let message = response
        .json::<SiteErrorResponse>()
        .await
        .ok()
        .and_then(|body| body.message.or(body.error))
        .unwrap_or_else(|| format!("the skin site answered HTTP {status}"));
    ErrorKind::OtherError(format!("Skin site: {message}")).as_error()
}

/// Bounds a write so a stalled site cannot leave a skin action pending forever.
async fn bounded(
    request: impl std::future::Future<Output = reqwest::Result<reqwest::Response>>,
) -> crate::Result<reqwest::Response> {
    match tokio::time::timeout(WRITE_TIMEOUT, request).await {
        Ok(response) => Ok(response?),
        Err(_) => {
            tracing::warn!(
                target: "theseus::api::minecraft_skins",
                timeout_seconds = WRITE_TIMEOUT.as_secs(),
                "the skin site did not answer a write in time"
            );
            Err(ErrorKind::OtherError(format!(
                "The skin site did not answer within {} seconds",
                WRITE_TIMEOUT.as_secs()
            ))
            .as_error())
        }
    }
}

/// Lists the character's textures and what is currently equipped.
///
/// The answer is reused for [`CACHE_TTL`]; `invalidate_skins` drops it after a
/// write so a change is visible immediately. Concurrent callers share one
/// request, and the request is bounded by [`LIST_TIMEOUT`] so a site that stops
/// answering cannot leave a page loading forever.
pub async fn fetch_skins(
    credentials: &Credentials,
) -> crate::Result<SiteSkins> {
    let player_name = &credentials.offline_profile.name;
    let token = access_token(credentials)?;
    let key = cache_key(
        credentials.account_id.as_deref().unwrap_or(""),
        player_name,
        token,
    );
    if let Some(cached) = cached_skins(&key) {
        return Ok(cached);
    }
    let _listing = LIST_LOCK.lock().await;
    if let Some(cached) = cached_skins(&key) {
        return Ok(cached);
    }
    let generation = CACHE
        .lock()
        .map(|cache| cache.generation)
        .unwrap_or_default();
    let skins = list_skins(player_name, token).await?;
    store_skins(key, generation, &skins);
    Ok(skins)
}

async fn list_skins(
    player_name: &str,
    token: &str,
) -> crate::Result<SiteSkins> {
    let started = Instant::now();
    tracing::info!(
        target: "theseus::api::minecraft_skins",
        player = %player_name,
        "asking the skin site for the character's textures"
    );

    let request = INSECURE_REQWEST_CLIENT
        .get(endpoint("/api/launcher/skins"))
        .timeout(LIST_TIMEOUT)
        .query(&[("player", player_name)])
        .bearer_auth(token)
        .send();

    let response = match tokio::time::timeout(LIST_TIMEOUT, request).await {
        Ok(response) => response?,
        Err(_) => {
            tracing::warn!(
                target: "theseus::api::minecraft_skins",
                player = %player_name,
                timeout_seconds = LIST_TIMEOUT.as_secs(),
                "the skin site did not answer the listing in time"
            );
            return Err(ErrorKind::OtherError(format!(
                "The skin site did not answer within {} seconds",
                LIST_TIMEOUT.as_secs()
            ))
            .as_error());
        }
    };
    if !response.status().is_success() {
        let error = site_error(response).await;
        tracing::warn!(
            target: "theseus::api::minecraft_skins",
            player = %player_name,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "the skin site refused the listing: {error}"
        );
        return Err(error);
    }
    let body: SiteSkinsResponse = response.json().await.map_err(|error| {
        ErrorKind::OtherError(format!(
            "Skin site returned invalid JSON: {error}"
        ))
        .as_error()
    })?;

    let cape_tid = body.player.cape.as_ref().map(|cape| cape.tid);
    let skin_tid = body.player.skin.as_ref().map(|skin| skin.tid);
    let capes = body
        .textures
        .iter()
        .filter(|texture| texture.is_cape())
        .filter_map(|texture| {
            SiteTextureView {
                tid: texture.tid,
                name: texture.name.clone(),
                type_: texture.type_.clone(),
                model: texture.model.clone(),
                url: texture.url.clone(),
            }
            .into_cape(Some(texture.tid) == cape_tid)
        })
        .collect::<Vec<_>>();
    let mut found_equipped = false;
    let skins = body
        .textures
        .into_iter()
        .filter(|texture| !texture.is_cape())
        .filter_map(|texture| {
            let equipped = !found_equipped && Some(texture.tid) == skin_tid;
            found_equipped |= equipped;
            texture.into_skin(
                equipped,
                if equipped {
                    cape_tid.map(cape_id)
                } else {
                    None
                },
            )
        })
        .collect::<Vec<_>>();

    let skins = SiteSkins {
        player_name: body.player.name,
        equipped_skin_tid: skin_tid,
        equipped_cape_tid: cape_tid,
        skins,
        capes,
    };

    tracing::info!(
        target: "theseus::api::minecraft_skins",
        player = %skins.player_name,
        skins = skins.skins.len(),
        capes = skins.capes.len(),
        equipped_skin = ?skins.equipped_skin_tid,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "listed the character's textures from the skin site"
    );

    Ok(skins)
}

/// Drops the reuse window so the next [`fetch_skins`] sees a write that just
/// happened.
pub fn invalidate_skins() {
    if let Ok(mut guard) = CACHE.lock() {
        guard.generation = guard.generation.wrapping_add(1);
        guard.entry = None;
    }
}

/// Uploads a skin and reports the texture the site created (or the equal texture it
/// already had).
pub async fn upload_skin(
    credentials: &Credentials,
    texture_blob: &Bytes,
    variant: MinecraftSkinVariant,
    name: &str,
) -> crate::Result<UploadedSkin> {
    let token = access_token(credentials)?.to_string();
    let texture_type = match variant {
        MinecraftSkinVariant::Slim => "alex",
        MinecraftSkinVariant::Classic | MinecraftSkinVariant::Unknown => {
            "steve"
        }
    };
    let response = bounded(
        INSECURE_REQWEST_CLIENT
            .post(endpoint("/api/launcher/skins"))
            .timeout(WRITE_TIMEOUT)
            .query(&[
                ("player", credentials.offline_profile.name.as_str()),
                ("type", texture_type),
                ("name", name),
            ])
            .bearer_auth(token)
            .header("Content-Type", "image/png")
            .body(texture_blob.to_vec())
            .send(),
    )
    .await?;
    if !response.status().is_success() {
        return Err(site_error(response).await);
    }
    let body: SiteUploadResponse = response.json().await.map_err(|error| {
        ErrorKind::OtherError(format!(
            "Skin site returned invalid JSON: {error}"
        ))
        .as_error()
    })?;
    // a new texture belongs in the next listing
    invalidate_skins();

    Ok(UploadedSkin {
        tid: body.texture.tid,
        name: body.texture.name.clone(),
        texture: body.texture.texture_url(),
        variant: body.texture.variant(),
    })
}

/// Takes a texture out of the character's closet on the site, unequipping it there
/// first when the character is wearing it.
pub async fn delete_texture(
    credentials: &Credentials,
    tid: i64,
) -> crate::Result<()> {
    let token = access_token(credentials)?;
    let body = serde_json::json!({
        "player": credentials.offline_profile.name,
        "tid": tid,
    });
    let response = bounded(
        INSECURE_REQWEST_CLIENT
            .delete(endpoint("/api/launcher/skins"))
            .timeout(WRITE_TIMEOUT)
            .bearer_auth(token)
            .json(&body)
            .send(),
    )
    .await?;
    if !response.status().is_success() {
        return Err(site_error(response).await);
    }
    invalidate_skins();

    Ok(())
}

/// What the site reports about a texture it just accepted.
pub struct UploadedSkin {
    pub tid: i64,
    pub name: String,
    pub texture: Option<Arc<Url>>,
    pub variant: MinecraftSkinVariant,
}

impl UploadedSkin {
    pub fn into_skin(
        self,
        equipped: bool,
        cape_id: Option<uuid::Uuid>,
    ) -> Option<Skin> {
        Some(Skin {
            texture_key: Arc::from(self.tid.to_string()),
            name: Some(Arc::from(self.name.as_str())),
            section: None,
            variant: self.variant,
            cape_id,
            texture: self.texture?,
            source: SkinSource::Custom,
            is_equipped: equipped,
        })
    }
}

/// Equips an uploaded texture on the character the token belongs to.
///
/// Each slot is optional: `None` leaves it exactly as it is, while `Some(None)`
/// unequips it. The caller therefore always decides the cape, because the site's
/// closet is the only place a society account's cape lives.
pub async fn equip_texture(
    credentials: &Credentials,
    skin_tid: Option<i64>,
    cape_tid: Option<Option<i64>>,
) -> crate::Result<()> {
    let token = access_token(credentials)?;
    let mut body = serde_json::Map::new();
    body.insert(
        "player".to_string(),
        serde_json::Value::String(credentials.offline_profile.name.clone()),
    );
    if let Some(skin_tid) = skin_tid {
        body.insert("skin".to_string(), serde_json::Value::from(skin_tid));
    }
    if let Some(cape_tid) = cape_tid {
        // `0` is how the site's own equipment endpoint unequips a slot.
        body.insert(
            "cape".to_string(),
            serde_json::Value::from(cape_tid.unwrap_or_default()),
        );
    }
    if body.len() <= 1 {
        return Err(ErrorKind::InputError(
            "No skin or cape was given to equip".to_string(),
        )
        .into());
    }

    let response = bounded(
        INSECURE_REQWEST_CLIENT
            .put(endpoint("/api/launcher/skins/equip"))
            .timeout(WRITE_TIMEOUT)
            .bearer_auth(token)
            .json(&serde_json::Value::Object(body))
            .send(),
    )
    .await?;
    if !response.status().is_success() {
        return Err(site_error(response).await);
    }
    invalidate_skins();

    Ok(())
}

/// Takes a texture id out of a skin the site handed back.
pub fn texture_id(skin: &Skin) -> Option<i64> {
    tid_of(&skin.texture_key)
}

pub fn cape_texture_id(cape: &Cape) -> Option<i64> {
    cape_tid_from_uuid(cape.id)
}

pub fn cape_tid_from_uuid(id: uuid::Uuid) -> Option<i64> {
    if id.as_bytes()[8..] != [0; 8] {
        return None;
    }
    let tid = i64::from_be_bytes(id.as_bytes()[..8].try_into().ok()?);
    (tid > 0).then_some(tid)
}

/// The inverse of [`cape_tid_from_uuid`].
pub fn cape_uuid(tid: i64) -> uuid::Uuid {
    cape_id(tid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_identity_changes_when_account_player_or_token_changes() {
        let original = cache_key("account-a", "Steve", "token-a");
        assert_ne!(original, cache_key("account-b", "Steve", "token-a"));
        assert_ne!(original, cache_key("account-a", "Alex", "token-a"));
        assert_ne!(original, cache_key("account-a", "Steve", "token-b"));
        assert_ne!(cache_key("ab", "c", "d"), cache_key("a", "bc", "d"));
    }

    #[test]
    fn rejects_foreign_and_invalid_cape_ids() {
        assert_eq!(cape_tid_from_uuid(uuid::Uuid::nil()), None);
        assert_eq!(cape_tid_from_uuid(cape_id(-1)), None);
        let mut foreign = *cape_id(42).as_bytes();
        foreign[15] = 1;
        assert_eq!(cape_tid_from_uuid(uuid::Uuid::from_bytes(foreign)), None);
    }

    #[test]
    fn cape_ids_round_trip_through_the_site_texture_id() {
        for tid in [1_i64, 2, 42, 9_999, i64::from(u32::MAX)] {
            let cape = Cape {
                id: cape_id(tid),
                name: Arc::from("Cape"),
                texture: Arc::new(
                    Url::parse("https://skin.ysumc.net/textures/x").unwrap(),
                ),
                is_equipped: true,
            };
            assert_eq!(cape_texture_id(&cape), Some(tid));
        }
    }

    #[test]
    fn texture_keys_only_carry_site_texture_ids() {
        assert_eq!(tid_of("12"), Some(12));
        assert_eq!(tid_of("local-abc"), None);
        assert_eq!(tid_of("0"), None);
    }
}
