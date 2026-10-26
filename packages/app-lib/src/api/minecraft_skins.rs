//! # Minecraft Skins API
//!
//! ## Data Flow
//!
//! 1. Frontend calls `get_available_skins()` and `get_available_capes()`
//! 2. Backend gets the selected Minecraft account and a recent Mojang profile.
//!    If skins and capes load at the same time, they share the recent profile
//!    instead of sending the same request twice.
//! 3. The skin list is built from three places:
//!    - saved skin rows in the local app database
//!    - bundled Minecraft default skins
//!    - the active Mojang skin, if its texture is not represented by a saved
//!      skin or matching bundled default
//! 4. While building the list, any saved skin with Mojang's active texture is
//!    updated to Mojang's current model variant and cape, then returned as the
//!    equipped skin.
//! 5. Before changing a skin, the current non-default Mojang skin is preserved
//!    locally so switching away from an external skin does not lose it.
//! 6. After a Mojang change, the returned profile is saved in memory when
//!    possible. If that response cannot be read, or a later step fails, the
//!    backend asks Mojang for the profile again.
//!
//! ## Ownership
//!
//! Mojang decides which skin and cape are currently equipped for Microsoft
//! accounts. For offline accounts, the local database stores the selected skin
//! and the launcher applies it through an instance resource pack. The local
//! database also stores saved skin rows. A saved skin is the same saved skin when its
//! `texture_key` matches; changing its model variant or cape updates that saved
//! skin instead of creating another row.
//! When a refreshed Mojang profile reports the same texture as a saved skin but
//! a different cape or model variant, the saved skin is updated to match Mojang
//! and returned as the equipped skin.
//! A bundled default skin with no cape is redundant, so it is removed from the
//! saved-skin database and represented by the default skin list instead. A
//! bundled default skin with a cape is stored so the cape stays associated with
//! that default card, but it is still returned as a default skin rather than a
//! saved custom skin.
//!
//! `cape_id = Some(_)` means a skin should apply that specific cape.
//! `cape_id = None` means the skin should have no cape.
//!
//! ## Consistency
//!
//! A Mojang request and a SQLite write cannot be one all-or-nothing operation.
//! The backend handles this by reconciling refreshed Mojang profile data with
//! saved rows, saving skins that might be lost before changing Mojang, saving
//! uploaded skins with the texture key Mojang returns, and asking Mojang for the
//! latest profile again whenever the result is unclear.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock},
    time::Duration,
};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
pub use bytes::Bytes;
use futures::{StreamExt, TryStreamExt, stream};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tokio::sync::Mutex;
use url::Url;
use uuid::Uuid;

pub use crate::state::MinecraftSkinVariant;
use crate::{
    ErrorKind, State,
    state::{
        MinecraftCharacterExpressionState, MinecraftProfile,
        minecraft_skins::{
            CustomMinecraftSkin, CustomMinecraftSkinInsertPosition,
            OfflineMinecraftSkin, mojang_api,
        },
    },
};

use super::data::Credentials;

mod assets {
    mod default {
        mod default_skins;
        pub use default_skins::DEFAULT_SKINS;
    }
    pub use default::DEFAULT_SKINS;
}

mod offline;
pub(crate) use offline::{
    OFFLINE_SKIN_PACK_LEGACY_ID, OFFLINE_SKIN_PACK_MODERN_ID,
    OfflineSkinPackOptions, prepare_offline_skin_resource_pack,
};

mod png_util;
mod ysumc_site;

pub use ysumc_site::{cape_texture_id, texture_id as site_texture_id};

const SKIN_CHANGE_DEBOUNCE: Duration = Duration::from_secs(10);

static PENDING_SKIN_CHANGE: LazyLock<Mutex<PendingSkinChangeState>> =
    LazyLock::new(|| Mutex::new(PendingSkinChangeState::default()));
static SKIN_CHANGE_FLUSH_LOCK: LazyLock<Mutex<()>> =
    LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Default)]
struct PendingSkinChangeState {
    pending: HashMap<Uuid, PendingSkinChangeEntry>,
}

#[derive(Debug)]
struct PendingSkinChangeEntry {
    change: PendingSkinChange,
    generation: u64,
}

enum PendingEffectiveSkinChange {
    AddAndEquipCustom {
        texture_key: Arc<str>,
        texture_blob: Bytes,
        variant: MinecraftSkinVariant,
        cape_id: Option<Uuid>,
    },
    Equip {
        skin: Skin,
    },
    Unequip,
}

impl PendingEffectiveSkinChange {
    fn is_unequip(&self) -> bool {
        matches!(self, Self::Unequip)
    }

    fn cape_id(&self) -> Option<Uuid> {
        match self {
            Self::AddAndEquipCustom { cape_id, .. } => *cape_id,
            Self::Equip { skin } => skin.cape_id,
            Self::Unequip => None,
        }
    }

    fn skin(&self) -> Option<Skin> {
        match self {
            Self::AddAndEquipCustom {
                texture_key,
                texture_blob,
                variant,
                cape_id,
            } => Some(Skin {
                texture_key: Arc::clone(texture_key),
                name: None,
                section: None,
                variant: *variant,
                cape_id: *cape_id,
                texture: png_util::blob_to_data_url(texture_blob)
                    .or_else(|| {
                        png_util::blob_to_data_url(include_bytes!(
                            "minecraft_skins/assets/default/MissingNo.png"
                        ))
                    })
                    .unwrap(),
                source: SkinSource::Custom,
                is_equipped: true,
            }),
            Self::Equip { skin } => Some(skin.clone()),
            Self::Unequip => None,
        }
    }
}

#[derive(Debug)]
enum PendingSkinChange {
    AddAndEquipCustom {
        selected_credentials: Credentials,
        texture_blob: Bytes,
        variant: MinecraftSkinVariant,
        cape_id: Option<Uuid>,
        local_texture_key: Arc<str>,
    },
    Equip {
        selected_credentials: Credentials,
        skin: Skin,
    },
    Unequip {
        selected_credentials: Credentials,
    },
}

impl PendingSkinChange {
    fn profile_id(&self) -> Uuid {
        match self {
            Self::AddAndEquipCustom {
                selected_credentials,
                ..
            }
            | Self::Equip {
                selected_credentials,
                ..
            }
            | Self::Unequip {
                selected_credentials,
            } => selected_credentials.offline_profile.id,
        }
    }

    fn matches_skin(&self, skin: &Skin) -> bool {
        match self {
            Self::AddAndEquipCustom {
                local_texture_key, ..
            } => local_texture_key.as_ref() == skin.texture_key.as_ref(),
            Self::Equip {
                skin: pending_skin, ..
            } => pending_skin.texture_key == skin.texture_key,
            Self::Unequip { .. } => false,
        }
    }
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct Cape {
    /// An identifier for this cape, potentially unique to the owning player.
    pub id: Uuid,
    /// The name of the cape.
    pub name: Arc<str>,
    /// The URL of the cape PNG texture.
    pub texture: Arc<Url>,
    /// Whether the cape is currently equipped in the Minecraft profile of its corresponding
    /// player.
    pub is_equipped: bool,
}

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct Skin {
    /// A key used to recognize this skin texture.
    pub texture_key: Arc<str>,
    /// The name of the skin, if available.
    pub name: Option<Arc<str>>,
    /// The section this skin should be grouped under, if available.
    #[serde(default)]
    pub section: Option<Arc<str>>,
    /// The variant of the skin model.
    pub variant: MinecraftSkinVariant,
    /// The UUID of the cape that this skin uses, if any.
    ///
    /// If `None`, this skin uses no cape.
    pub cape_id: Option<Uuid>,
    /// The URL of the skin PNG texture. Can also be a data URL.
    pub texture: Arc<Url>,
    /// The source of the skin, which represents how the app knows about it.
    pub source: SkinSource,
    /// Whether the skin is currently equipped in the Minecraft profile of its corresponding
    /// player.
    pub is_equipped: bool,
}

#[derive(Clone, Deserialize, Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SkinSource {
    /// A default Minecraft skin, which may be assigned to players at random by default.
    Default,
    /// A skin that is not the default, but is not a custom skin managed by our app either.
    CustomExternal,
    /// A custom skin we have set up in our app.
    Custom,
}

/// Represents either a URL or a blob for a Minecraft skin PNG texture.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum UrlOrBlob {
    Url(Url),
    Blob(Bytes),
}

fn ensure_skin_management_supported(
    credentials: &Credentials,
) -> crate::Result<()> {
    if credentials.is_yggdrasil() {
        // The society's own skin site hosts its players' skins and exposes the
        // launcher endpoints used below. Any other Yggdrasil provider is not
        // ours to write to.
        let supported = credentials.yggdrasil.as_ref().is_some_and(|account| {
            crate::ysumc::is_yggdrasil_api_root(&account.api_root)
        });
        if supported {
            return Ok(());
        }

        return Err(ErrorKind::InputError(
            "Skins for third-party Yggdrasil accounts must be managed through the account provider"
                .to_string(),
        )
        .as_error());
    }
    Ok(())
}

fn is_society_skin_site(credentials: &Credentials) -> bool {
    credentials.is_yggdrasil()
        && credentials.yggdrasil.as_ref().is_some_and(|account| {
            crate::ysumc::is_yggdrasil_api_root(&account.api_root)
        })
}

/// Gets the capes for the selected Minecraft profile.
/// Only one cape can be equipped.
#[tracing::instrument]
pub async fn get_available_capes() -> crate::Result<Vec<Cape>> {
    let state = State::get().await?;

    let selected_credentials = Credentials::get_default_credential(&state.pool)
        .await?
        .ok_or(ErrorKind::NoCredentialsError)?;

    // A society account's capes are textures in the skin site's closet, which is a
    // different library from the Mojang profile the code below reads.
    if is_society_skin_site(&selected_credentials) {
        return Ok(ysumc_site::fetch_skins(&selected_credentials).await?.capes);
    }

    let Some(profile) = selected_credentials.online_profile_fresh().await
    else {
        return Ok(Vec::new());
    };

    let pending_skin_change = pending_effective_skin_change(profile.id).await;
    let pending_cape_id = pending_skin_change
        .as_ref()
        .map(PendingEffectiveSkinChange::cape_id);

    Ok(profile
        .capes
        .iter()
        .map(|cape| Cape {
            id: cape.id,
            name: Arc::clone(&cape.name),
            texture: Arc::clone(&cape.url),
            is_equipped: pending_cape_id.map_or_else(
                || cape.state == MinecraftCharacterExpressionState::Active,
                |cape_id| cape_id == Some(cape.id),
            ),
        })
        .collect())
}

async fn society_cape_credentials(
    account_id: &str,
) -> crate::Result<Credentials> {
    let state = State::get().await?;
    let credentials = Credentials::get_default_credential(&state.pool)
        .await?
        .ok_or(ErrorKind::NoCredentialsError)?;
    if credentials.account_id.as_deref() != Some(account_id)
        || !is_society_skin_site(&credentials)
    {
        return Err(ErrorKind::InputError("The selected YSUMC account changed. Refresh the skin library and try again.".into()).into());
    }
    Ok(credentials)
}

fn validate_cape_upload(texture_blob: &[u8], name: &str) -> crate::Result<()> {
    let (width, height) = png_util::dimensions(texture_blob)?;
    if texture_blob.len() > 1024 * 1024
        || height == 0
        || width > 1024
        || height > 512
        || width != height * 2
    {
        return Err(ErrorKind::InputError("Use a PNG cape with a 2:1 aspect ratio, at most 1024 × 512 pixels and 1 MiB.".into()).into());
    }
    if name.trim().is_empty() || name.chars().count() > 80 {
        return Err(ErrorKind::InputError(
            "Enter a cape name of 1–80 characters.".into(),
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod cape_upload_tests {
    use super::*;

    #[test]
    fn cape_upload_rejects_invalid_dimensions_names_and_payloads() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(64u32.to_be_bytes());
        png.extend(32u32.to_be_bytes());
        assert!(validate_cape_upload(&png, "Cape").is_ok());
        assert!(validate_cape_upload(&png, "  ").is_err());
        assert!(validate_cape_upload(&png, &"x".repeat(81)).is_err());
        assert!(validate_cape_upload(b"not a png", "Cape").is_err());
        png[20..24].copy_from_slice(&64u32.to_be_bytes());
        assert!(validate_cape_upload(&png, "Skin").is_err());
        png[20..24].copy_from_slice(&32u32.to_be_bytes());
        png.resize(1024 * 1024 + 1, 0);
        assert!(validate_cape_upload(&png, "Too big").is_err());
    }
}

/// Upload into the YSUMC closet without changing the player's skin or cape.
pub async fn upload_cape(
    account_id: String,
    texture_blob: Bytes,
    name: String,
) -> crate::Result<Cape> {
    validate_cape_upload(&texture_blob, &name)?;
    let credentials = society_cape_credentials(&account_id).await?;
    ysumc_site::upload_cape(&credentials, &texture_blob, name.trim()).await
}

/// Change only the cape slot; `None` removes the currently worn cape.
pub async fn equip_cape(
    account_id: String,
    cape_id: Option<Uuid>,
) -> crate::Result<()> {
    let credentials = society_cape_credentials(&account_id).await?;
    let tid = selected_site_cape(cape_id)?;
    ysumc_site::equip_texture(&credentials, None, Some(tid)).await
}

pub async fn remove_cape(
    account_id: String,
    cape_id: Uuid,
) -> crate::Result<()> {
    let credentials = society_cape_credentials(&account_id).await?;
    let tid = selected_site_cape(Some(cape_id))?
        .ok_or(ErrorKind::InvalidSkinTexture)?;
    ysumc_site::delete_texture(&credentials, tid).await
}

/// Gets the skins for the selected Minecraft profile.
/// Returns saved custom skins, bundled default skins, and the active Mojang skin if its texture
/// is not represented by a saved skin or matching bundled default.
///
/// Saved skins are identified by texture key. If Mojang reports that a saved skin is active with
/// a different model variant or cape, the saved row is updated and returned as equipped.
/// Exactly one returned skin is marked as equipped.
#[tracing::instrument]
pub async fn get_available_skins() -> crate::Result<Vec<Skin>> {
    let state = State::get().await?;

    let selected_credentials = Credentials::get_default_credential(&state.pool)
        .await?
        .ok_or(ErrorKind::NoCredentialsError)?;
    let log_started = std::time::Instant::now();
    tracing::info!(
        target: "theseus::api::minecraft_skins",
        player = %selected_credentials.offline_profile.name,
        is_society = is_society_skin_site(&selected_credentials),
        "listing available skins"
    );

    let online_profile = selected_credentials.online_profile_fresh().await;
    tracing::info!(
        target: "theseus::api::minecraft_skins",
        elapsed_ms = log_started.elapsed().as_millis() as u64,
        has_profile = online_profile.is_some(),
        "read the account's online profile"
    );
    let profile_id = online_profile
        .as_ref()
        .map_or(selected_credentials.offline_profile.id, |profile| {
            profile.id
        });
    let offline_skin = if selected_credentials.is_offline() {
        OfflineMinecraftSkin::get(profile_id, &state.pool).await?
    } else {
        None
    };

    let current_skin = online_profile
        .as_ref()
        .map(|profile| profile.current_skin())
        .transpose()?;
    let current_cape_id = online_profile
        .as_ref()
        .and_then(|profile| profile.current_cape())
        .map(|cape| cape.id);
    let pending_skin_change = pending_effective_skin_change(profile_id).await;
    let pending_unequip = pending_skin_change
        .as_ref()
        .is_some_and(PendingEffectiveSkinChange::is_unequip);
    let pending_skin = pending_skin_change
        .as_ref()
        .and_then(PendingEffectiveSkinChange::skin);

    let fallback_default_skin = get_fallback_default_skin()?;
    let current_skin_texture_key = offline_skin.as_ref().map_or_else(
        || {
            pending_skin.as_ref().map_or_else(
                || {
                    if pending_unequip {
                        Arc::clone(&fallback_default_skin.texture_key)
                    } else if let Some(current_skin) = current_skin {
                        current_skin.texture_key()
                    } else {
                        Arc::clone(&fallback_default_skin.texture_key)
                    }
                },
                |skin| skin.texture_key.clone(),
            )
        },
        |skin| Arc::from(skin.texture_key.as_str()),
    );
    let current_skin_variant = offline_skin.as_ref().map_or_else(
        || {
            pending_skin.as_ref().map_or_else(
                || {
                    if pending_unequip {
                        fallback_default_skin.variant
                    } else if let Some(current_skin) = current_skin {
                        current_skin.variant
                    } else {
                        fallback_default_skin.variant
                    }
                },
                |skin| skin.variant,
            )
        },
        |skin| skin.variant,
    );
    let current_cape_id = if offline_skin.is_some() {
        None
    } else {
        pending_skin.as_ref().map_or(
            if pending_unequip {
                None
            } else if current_skin.is_some() {
                current_cape_id
            } else {
                None
            },
            |skin| skin.cape_id,
        )
    };
    let mut found_equipped_skin = false;
    let mut available_skins = Vec::new();
    let mut custom_skins = Vec::new();
    let mut saved_default_skins = Vec::new();

    for mut custom_skin in CustomMinecraftSkin::get_all(profile_id, &state.pool)
        .await?
        .collect::<Vec<_>>()
        .await
    {
        let is_saved_default_skin =
            is_bundled_skin(&custom_skin.texture_key, custom_skin.variant);
        let current_skin_sync =
            if pending_skin.is_some() || current_skin.is_none() {
                SavedSkinSync {
                    is_current_skin: custom_skin.texture_key
                        == current_skin_texture_key.as_ref()
                        && custom_skin.variant == current_skin_variant
                        && custom_skin.cape_id == current_cape_id,
                    settings_changed: false,
                }
            } else {
                sync_saved_skin_with_current_profile(
                    &mut custom_skin,
                    &current_skin_texture_key,
                    current_skin_variant,
                    current_cape_id,
                )
            };

        let synced_texture_blob = if current_skin_sync.settings_changed {
            let texture_blob = custom_skin.texture_blob(&state.pool).await?;

            if is_saved_default_skin && custom_skin.cape_id.is_none() {
                custom_skin.remove(profile_id, &state.pool).await?;
            } else {
                CustomMinecraftSkin::add(
                    profile_id,
                    &custom_skin.texture_key,
                    &texture_blob,
                    custom_skin.variant,
                    custom_skin.cape_id,
                    CustomMinecraftSkinInsertPosition::Bottom,
                    &state.pool,
                )
                .await?;
            }

            Some(texture_blob)
        } else {
            None
        };

        if is_saved_default_skin {
            if custom_skin.cape_id.is_some() {
                saved_default_skins.push(custom_skin);
            }
            continue;
        }

        let is_equipped =
            !found_equipped_skin && current_skin_sync.is_current_skin;

        found_equipped_skin |= is_equipped;

        let texture_blob = match synced_texture_blob {
            Some(texture_blob) => texture_blob,
            None => custom_skin.texture_blob(&state.pool).await?,
        };

        custom_skins.push(Skin {
            name: None,
            section: None,
            variant: custom_skin.variant,
            cape_id: custom_skin.cape_id,
            texture: png_util::blob_to_data_url(texture_blob)
                .or_else(|| {
                    png_util::blob_to_data_url(include_bytes!(
                        "minecraft_skins/assets/default/MissingNo.png"
                    ))
                })
                .unwrap(),
            source: SkinSource::Custom,
            is_equipped,
            texture_key: custom_skin.texture_key.into(),
        });
    }

    available_skins.extend(custom_skins);

    for default_skin in assets::DEFAULT_SKINS.iter() {
        let is_equipped = !found_equipped_skin
            && default_skin.texture_key == current_skin_texture_key
            && default_skin.variant == current_skin_variant;
        let saved_cape_id = saved_default_skins
            .iter()
            .find(|skin| {
                skin.texture_key.as_str() == default_skin.texture_key.as_ref()
                    && skin.variant == default_skin.variant
            })
            .and_then(|skin| skin.cape_id);

        found_equipped_skin |= is_equipped;

        available_skins.push(Skin {
            texture_key: Arc::clone(&default_skin.texture_key),
            name: default_skin.name.as_ref().cloned(),
            section: default_skin.section.as_ref().cloned(),
            variant: default_skin.variant,
            cape_id: if is_equipped {
                current_cape_id
            } else {
                saved_cape_id
            },
            texture: Arc::clone(&default_skin.texture),
            source: SkinSource::Default,
            is_equipped,
        });
    }

    // Keep the active skin visible even if the app has never saved it. A society
    // account gets this from the site list below instead.
    if !found_equipped_skin && !is_society_skin_site(&selected_credentials) {
        if let Some(mut skin) = pending_skin {
            skin.is_equipped = true;
            available_skins.push(skin);
        } else if let Some(current_skin) = current_skin {
            available_skins.push(Skin {
                texture_key: current_skin_texture_key,
                name: current_skin.name.as_deref().map(Arc::from),
                section: None,
                variant: current_skin_variant,
                cape_id: current_cape_id,
                texture: Arc::clone(&current_skin.url),
                source: SkinSource::CustomExternal,
                is_equipped: true,
            });
        }
    }

    // The society's skin site is the source of truth for a society account: the
    // textures in its closet are the player's skins, and they can be equipped
    // straight from here.
    if is_society_skin_site(&selected_credentials) {
        tracing::info!(
            target: "theseus::api::minecraft_skins",
            elapsed_ms = log_started.elapsed().as_millis() as u64,
            local_skins = available_skins.len(),
            "asking the skin site for the character's textures"
        );
        match ysumc_site::fetch_skins(&selected_credentials).await {
            Ok(site) => {
                let mut site_skins = site.skins;
                for skin in site_skins.iter_mut() {
                    skin.is_equipped =
                        Some(ysumc_site::texture_id(skin).unwrap_or_default())
                            == site.equipped_skin_tid;
                    if skin.is_equipped {
                        skin.cape_id =
                            site.equipped_cape_tid.map(ysumc_site::cape_uuid);
                    }
                }

                // Take a skin only once. The site list wins because it is the same
                // library the player sees on the website; the entries the Mojang
                // path produced above are either rows an older launcher mirrored
                // locally or the profile fallback, and counting them twice is what
                // made the launcher show more skins than the site has.
                let site_textures = site_skins
                    .iter()
                    .map(site_skin_texture)
                    .collect::<HashSet<_>>();
                let is_on_site = |skin: &Skin| {
                    texture_hash(&skin.texture)
                        .is_some_and(|hash| site_textures.contains(&hash))
                };

                let site_has_equipped =
                    site_skins.iter().any(|skin| skin.is_equipped);

                for mut skin in available_skins {
                    if is_on_site(&skin) {
                        continue;
                    }
                    // Exactly one entry may carry the equipped flag, and the site
                    // already claimed it.
                    if site_has_equipped && skin.is_equipped {
                        continue;
                    }
                    skin.is_equipped = false;
                    site_skins.push(skin);
                }

                tracing::info!(
                    target: "theseus::api::minecraft_skins",
                    elapsed_ms = log_started.elapsed().as_millis() as u64,
                    skins = site_skins.len(),
                    capes = site.capes.len(),
                    "returning the skin site's closet"
                );

                return Ok(site_skins);
            }
            Err(error) => {
                tracing::warn!(
                    target: "theseus::api::minecraft_skins",
                    elapsed_ms = log_started.elapsed().as_millis() as u64,
                    "Unable to list the skin site textures, showing local skins only: {error}"
                );
            }
        }
    }

    tracing::info!(
        target: "theseus::api::minecraft_skins",
        elapsed_ms = log_started.elapsed().as_millis() as u64,
        skins = available_skins.len(),
        "returning locally known skins"
    );

    Ok(available_skins)
}

/// The content hash a skin site serves its textures under.
fn site_skin_texture(skin: &Skin) -> String {
    texture_hash(&skin.texture).unwrap_or_default()
}

/// Blessing Skin serves a texture as `/textures/{sha256}`, which is the same hash
/// the launcher names a mirrored texture row after.
fn texture_hash(texture: &Url) -> Option<String> {
    if texture.scheme() == "data" {
        let (_, encoded) = texture.as_str().split_once("base64,")?;
        let blob = BASE64_STANDARD.decode(encoded).ok()?;
        return Some(format!("{:x}", sha2::Sha256::digest(&blob)));
    }

    texture
        .path_segments()?
        .next_back()
        .map(|segment| segment.to_ascii_lowercase())
}

/// Adds or updates a skin in the app database and equips it for the current profile.
/// Bundled default skins are only persisted when they have an associated cape.
#[tracing::instrument(skip(texture_blob))]
pub async fn add_and_equip_custom_skin(
    texture_blob: Bytes,
    variant: MinecraftSkinVariant,
    cape: Option<Cape>,
) -> crate::Result<Skin> {
    let (skin_width, skin_height) = png_util::dimensions(&texture_blob)?;
    if skin_width != 64 || ![32, 64].contains(&skin_height) {
        return Err(ErrorKind::InvalidSkinTexture)?;
    }

    let state = State::get().await?;
    let selected_credentials = Credentials::get_default_credential(&state.pool)
        .await?
        .ok_or(ErrorKind::NoCredentialsError)?;
    ensure_skin_management_supported(&selected_credentials)?;

    // A society account lives entirely on the skin site: the texture goes into the
    // site's closet and is equipped there, and the site's closet is what lists the
    // player's skins afterwards, so there is nothing to mirror locally.
    if is_society_skin_site(&selected_credentials) {
        let cape_tid = selected_site_cape(cape.as_ref().map(|cape| cape.id))?;
        let uploaded = ysumc_site::upload_skin(
            &selected_credentials,
            &texture_blob,
            variant,
            &local_skin_name(&texture_blob),
        )
        .await?;
        ysumc_site::equip_texture(
            &selected_credentials,
            Some(uploaded.tid),
            Some(cape_tid),
        )
        .await?;

        return Ok(uploaded
            .into_skin(true, cape.as_ref().map(|cape| cape.id))
            .ok_or_else(|| {
                ErrorKind::OtherError(
                    "The skin site did not return a usable texture URL"
                        .to_string(),
                )
                .as_error()
            })?);
    }

    let cape_id = if selected_credentials.is_offline() {
        None
    } else {
        cape.map(|cape| cape.id)
    };
    let local_texture_key = local_skin_texture_key(&texture_blob);

    CustomMinecraftSkin::add(
        selected_credentials.offline_profile.id,
        &local_texture_key,
        &texture_blob,
        variant,
        cape_id,
        CustomMinecraftSkinInsertPosition::Top,
        &state.pool,
    )
    .await?;

    if selected_credentials.is_offline() {
        OfflineMinecraftSkin::set(
            selected_credentials.offline_profile.id,
            &local_texture_key,
            variant,
            &state.pool,
        )
        .await?;
    } else {
        set_pending_skin_change(PendingSkinChange::AddAndEquipCustom {
            selected_credentials,
            texture_blob: Bytes::clone(&texture_blob),
            variant,
            cape_id,
            local_texture_key: Arc::clone(&local_texture_key),
        })
        .await;
    }

    Ok(Skin {
        texture_key: local_texture_key,
        name: None,
        section: None,
        variant,
        cape_id,
        texture: png_util::blob_to_data_url(texture_blob)
            .or_else(|| {
                png_util::blob_to_data_url(include_bytes!(
                    "minecraft_skins/assets/default/MissingNo.png"
                ))
            })
            .unwrap(),
        source: SkinSource::Custom,
        is_equipped: true,
    })
}

async fn add_and_equip_custom_skin_now(
    selected_credentials: &Credentials,
    texture_blob: Bytes,
    variant: MinecraftSkinVariant,
    cape_id: Option<Uuid>,
    local_texture_key: &str,
) -> crate::Result<()> {
    let state = State::get().await?;

    let previous_profile = selected_credentials
        .online_profile_fresh()
        .await
        .ok_or_else(|| ErrorKind::OnlineMinecraftProfileUnavailable {
            user_name: selected_credentials.offline_profile.name.clone(),
        })?;

    preserve_current_profile_skin(&state, &previous_profile).await?;

    // Mojang only gives us the new texture key after accepting the uploaded skin.
    // Use the profile from that response when possible, and fetch it only if that
    // response cannot be read.
    let profile = mojang_api::MinecraftSkinOperation::equip(
        selected_credentials,
        stream::iter([Ok::<_, String>(Bytes::clone(&texture_blob))]),
        variant,
    )
    .await?;

    let profile = match profile {
        Some(profile) => profile,
        None => selected_credentials
            .refresh_online_profile()
            .await
            .ok_or_else(|| ErrorKind::OnlineMinecraftProfileUnavailable {
                user_name: selected_credentials.offline_profile.name.clone(),
            })?,
    };

    let equipped_skin = profile.current_skin()?;
    let equipped_skin_texture_key = equipped_skin.texture_key();
    let equipped_skin_variant = equipped_skin.variant;
    let insert_position = if local_texture_key
        != equipped_skin_texture_key.as_ref()
    {
        CustomMinecraftSkin::get_by_texture(
            profile.id,
            local_texture_key,
            &state.pool,
        )
        .await?
        .map_or(CustomMinecraftSkinInsertPosition::Top, |skin| {
            CustomMinecraftSkinInsertPosition::At(skin.display_order)
        })
    } else {
        CustomMinecraftSkinInsertPosition::Top
    };

    let persistence_result = if cape_id.is_none()
        && is_bundled_skin(&equipped_skin_texture_key, equipped_skin_variant)
    {
        CustomMinecraftSkin {
            texture_key: equipped_skin_texture_key.to_string(),
            variant: equipped_skin_variant,
            cape_id: None,
            display_order: 0,
        }
        .remove(profile.id, &state.pool)
        .await
    } else {
        CustomMinecraftSkin::add(
            profile.id,
            &equipped_skin_texture_key,
            &texture_blob,
            variant,
            cape_id,
            insert_position,
            &state.pool,
        )
        .await
    };

    if let Err(error) = persistence_result {
        refresh_profile_cache(selected_credentials).await;
        return Err(error);
    }

    if local_texture_key != equipped_skin_texture_key.as_ref() {
        CustomMinecraftSkin {
            texture_key: local_texture_key.to_string(),
            variant,
            cape_id,
            display_order: 0,
        }
        .remove(profile.id, &state.pool)
        .await?;
    }

    if let Err(error) = sync_cape(selected_credentials, &profile, cape_id).await
    {
        refresh_profile_cache(selected_credentials).await;
        return Err(error);
    }

    Ok(())
}

/// Equips the given skin for the currently selected Minecraft profile, then applies its cape.
/// If the skin is already equipped, it will be re-equipped.
///
/// This does not check whether a custom skin exists in the app database.
#[tracing::instrument]
pub async fn equip_skin(skin: Skin) -> crate::Result<()> {
    let state = State::get().await?;
    let selected_credentials = Credentials::get_default_credential(&state.pool)
        .await?
        .ok_or(ErrorKind::NoCredentialsError)?;
    ensure_skin_management_supported(&selected_credentials)?;

    if is_society_skin_site(&selected_credentials) {
        equip_on_skin_site(&selected_credentials, &skin).await?;

        return Ok(());
    }

    if selected_credentials.is_offline() {
        equip_offline_skin_now(&state, &selected_credentials, &skin).await?;
    } else {
        set_pending_skin_change(PendingSkinChange::Equip {
            selected_credentials,
            skin,
        })
        .await;
    }

    Ok(())
}

/// Equips a site texture, uploading bundled/local textures to the closet first.
async fn equip_on_skin_site(
    credentials: &Credentials,
    skin: &Skin,
) -> crate::Result<()> {
    let cape_tid = selected_site_cape(skin.cape_id)?;
    let skin_tid = if let Some(tid) = ysumc_site::texture_id(skin) {
        tid
    } else {
        let texture = png_util::url_to_data_stream(&skin.texture)
            .await?
            .try_fold(Vec::new(), |mut bytes, chunk| async move {
                bytes.extend_from_slice(&chunk);
                Ok(bytes)
            })
            .await?;
        let (width, height) = png_util::dimensions(&texture)?;
        if width != 64 || ![32, 64].contains(&height) {
            return Err(ErrorKind::InvalidSkinTexture.into());
        }
        ysumc_site::upload_skin(
            credentials,
            &Bytes::from(texture),
            skin.variant,
            skin.name.as_deref().unwrap_or("Minecraft skin"),
        )
        .await?
        .tid
    };

    ysumc_site::equip_texture(credentials, Some(skin_tid), Some(cape_tid)).await
}

fn selected_site_cape(id: Option<uuid::Uuid>) -> crate::Result<Option<i64>> {
    id.map(|id| {
        ysumc_site::cape_tid_from_uuid(id).ok_or_else(|| {
            ErrorKind::InputError(
                "Please choose a cape from your YSUMC closet".into(),
            )
            .as_error()
        })
    })
    .transpose()
}

async fn equip_offline_skin_now(
    state: &State,
    selected_credentials: &Credentials,
    skin: &Skin,
) -> crate::Result<()> {
    let texture_blob = png_util::url_to_data_stream(&skin.texture)
        .await?
        .try_fold(Vec::new(), |mut texture, chunk| async move {
            texture.extend_from_slice(&chunk);
            Ok(texture)
        })
        .await?;
    let (skin_width, skin_height) = png_util::dimensions(&texture_blob)?;
    if skin_width != 64 || ![32, 64].contains(&skin_height) {
        return Err(ErrorKind::InvalidSkinTexture)?;
    }

    CustomMinecraftSkin::add(
        selected_credentials.offline_profile.id,
        &skin.texture_key,
        &texture_blob,
        skin.variant,
        None,
        CustomMinecraftSkinInsertPosition::Bottom,
        &state.pool,
    )
    .await?;
    OfflineMinecraftSkin::set(
        selected_credentials.offline_profile.id,
        &skin.texture_key,
        skin.variant,
        &state.pool,
    )
    .await
}

async fn equip_skin_now(
    selected_credentials: &Credentials,
    skin: &Skin,
) -> crate::Result<()> {
    let state = State::get().await?;

    let profile = selected_credentials
        .online_profile_fresh()
        .await
        .ok_or_else(|| ErrorKind::OnlineMinecraftProfileUnavailable {
            user_name: selected_credentials.offline_profile.name.clone(),
        })?;

    preserve_current_profile_skin(&state, &profile).await?;

    let texture_blob = png_util::url_to_data_stream(&skin.texture)
        .await?
        .try_fold(Vec::new(), |mut texture, chunk| async move {
            texture.extend_from_slice(&chunk);
            Ok(texture)
        })
        .await?;

    let profile = mojang_api::MinecraftSkinOperation::equip(
        selected_credentials,
        stream::iter([Ok::<_, String>(Bytes::from(texture_blob.clone()))]),
        skin.variant,
    )
    .await?;

    let profile = match profile {
        Some(profile) => profile,
        None => selected_credentials
            .refresh_online_profile()
            .await
            .ok_or_else(|| ErrorKind::OnlineMinecraftProfileUnavailable {
                user_name: selected_credentials.offline_profile.name.clone(),
            })?,
    };

    if let Err(error) =
        persist_equipped_skin(&state, &profile, skin, &texture_blob).await
    {
        refresh_profile_cache(selected_credentials).await;
        return Err(error);
    }

    if let Err(error) =
        sync_cape(selected_credentials, &profile, skin.cape_id).await
    {
        refresh_profile_cache(selected_credentials).await;
        return Err(error);
    }

    Ok(())
}

async fn persist_equipped_skin(
    state: &State,
    profile: &MinecraftProfile,
    skin: &Skin,
    texture_blob: &[u8],
) -> crate::Result<()> {
    let equipped_skin = profile.current_skin()?;
    let equipped_skin_texture_key = equipped_skin.texture_key();
    let equipped_skin_variant = equipped_skin.variant;
    let texture_key_changed =
        skin.texture_key.as_ref() != equipped_skin_texture_key.as_ref();
    let insert_position = if texture_key_changed {
        CustomMinecraftSkin::get_by_texture(
            profile.id,
            &skin.texture_key,
            &state.pool,
        )
        .await?
        .map_or(
            CustomMinecraftSkinInsertPosition::Bottom,
            |saved_skin| {
                CustomMinecraftSkinInsertPosition::At(saved_skin.display_order)
            },
        )
    } else {
        CustomMinecraftSkinInsertPosition::Bottom
    };

    if skin.cape_id.is_none()
        && is_bundled_skin(&equipped_skin_texture_key, equipped_skin_variant)
    {
        CustomMinecraftSkin {
            texture_key: equipped_skin_texture_key.to_string(),
            variant: equipped_skin_variant,
            cape_id: None,
            display_order: 0,
        }
        .remove(profile.id, &state.pool)
        .await?;
    } else if texture_key_changed || !matches!(&skin.source, SkinSource::Custom)
    {
        CustomMinecraftSkin::add(
            profile.id,
            &equipped_skin_texture_key,
            texture_blob,
            equipped_skin_variant,
            skin.cape_id,
            insert_position,
            &state.pool,
        )
        .await?;
    }

    if texture_key_changed {
        CustomMinecraftSkin {
            texture_key: skin.texture_key.to_string(),
            variant: skin.variant,
            cape_id: skin.cape_id,
            display_order: 0,
        }
        .remove(profile.id, &state.pool)
        .await?;
    }

    Ok(())
}

/// Removes a custom skin from the app database.
///
/// The player will continue to be equipped with the same skin and cape as before, even if
/// the currently selected skin is the one being removed. This gives frontend code more options
/// to decide between unequipping strategies: falling back to other custom skin, to a default
/// skin, letting the user choose another skin, etc.
#[tracing::instrument]
pub async fn remove_custom_skin(skin: Skin) -> crate::Result<()> {
    let state = State::get().await?;

    let selected_credentials = Credentials::get_default_credential(&state.pool)
        .await?
        .ok_or(ErrorKind::NoCredentialsError)?;
    ensure_skin_management_supported(&selected_credentials)?;

    // A society account's skins belong to the site: drop the texture from its
    // closet there, and let the launcher's local library follow along. A texture
    // the site no longer has (already removed on the website, say) is not an
    // error worth blocking the local cleanup.
    if let Some(site_tid) = is_society_skin_site(&selected_credentials)
        .then(|| ysumc_site::texture_id(&skin))
        .flatten()
    {
        ysumc_site::delete_texture(&selected_credentials, site_tid).await?;
    }

    if selected_credentials.is_offline() {
        OfflineMinecraftSkin::clear_if_texture(
            selected_credentials.offline_profile.id,
            &skin.texture_key,
            &state.pool,
        )
        .await?;
    }

    CustomMinecraftSkin {
        texture_key: skin.texture_key.to_string(),
        variant: skin.variant,
        cape_id: skin.cape_id,
        display_order: 0,
    }
    .remove(selected_credentials.offline_profile.id, &state.pool)
    .await?;

    cancel_pending_skin_change_for_skin(
        selected_credentials.offline_profile.id,
        &skin,
    )
    .await;

    Ok(())
}

/// Persists the custom saved skin order for the currently selected Minecraft profile.
#[tracing::instrument]
pub async fn set_custom_skin_order(
    texture_keys: Vec<String>,
) -> crate::Result<()> {
    let state = State::get().await?;

    let selected_credentials = Credentials::get_default_credential(&state.pool)
        .await?
        .ok_or(ErrorKind::NoCredentialsError)?;
    ensure_skin_management_supported(&selected_credentials)?;

    CustomMinecraftSkin::set_order(
        selected_credentials.offline_profile.id,
        &texture_keys,
        &state.pool,
    )
    .await
}

/// Adds or updates a saved skin locally without applying it to Mojang.
///
/// This is used by the skin editor. If the edited skin is currently equipped, the caller should
/// queue a separate equip operation after saving the local row.
#[tracing::instrument(skip(texture_blob))]
pub async fn save_custom_skin(
    mut skin: Skin,
    texture_blob: Bytes,
    variant: MinecraftSkinVariant,
    cape: Option<Cape>,
    replace_texture: bool,
) -> crate::Result<Skin> {
    let (skin_width, skin_height) = png_util::dimensions(&texture_blob)?;
    if skin_width != 64 || ![32, 64].contains(&skin_height) {
        return Err(ErrorKind::InvalidSkinTexture)?;
    }

    let state = State::get().await?;

    let selected_credentials = Credentials::get_default_credential(&state.pool)
        .await?
        .ok_or(ErrorKind::NoCredentialsError)?;
    ensure_skin_management_supported(&selected_credentials)?;

    // The site's closet is the only skin library a society account has, so an edit
    // is an upload: the site stores a new texture, the character keeps wearing it
    // when it already was, and the local database stays out of the way.
    if is_society_skin_site(&selected_credentials) {
        let cape_tid = selected_site_cape(cape.as_ref().map(|cape| cape.id))?;
        let uploaded = ysumc_site::upload_skin(
            &selected_credentials,
            &texture_blob,
            variant,
            &local_skin_name(&texture_blob),
        )
        .await?;
        if skin.is_equipped {
            ysumc_site::equip_texture(
                &selected_credentials,
                Some(uploaded.tid),
                Some(cape_tid),
            )
            .await?;
        }

        return uploaded
            .into_skin(skin.is_equipped, cape.as_ref().map(|cape| cape.id))
            .ok_or_else(|| {
                ErrorKind::OtherError(
                    "The skin site did not return a usable texture URL"
                        .to_string(),
                )
                .as_error()
            });
    }

    let old_texture_key = Arc::clone(&skin.texture_key);
    let texture_key = if replace_texture {
        local_skin_texture_key(&texture_blob)
    } else {
        Arc::clone(&skin.texture_key)
    };
    let cape_id = if selected_credentials.is_offline() {
        None
    } else {
        cape.map(|cape| cape.id)
    };
    let insert_position = if replace_texture && old_texture_key != texture_key {
        CustomMinecraftSkin::get_by_texture(
            selected_credentials.offline_profile.id,
            &old_texture_key,
            &state.pool,
        )
        .await?
        .map_or(CustomMinecraftSkinInsertPosition::Bottom, |skin| {
            CustomMinecraftSkinInsertPosition::At(skin.display_order)
        })
    } else {
        CustomMinecraftSkinInsertPosition::Bottom
    };

    if cape_id.is_none() && is_bundled_skin(&texture_key, variant) {
        CustomMinecraftSkin {
            texture_key: texture_key.to_string(),
            variant,
            cape_id: None,
            display_order: 0,
        }
        .remove(selected_credentials.offline_profile.id, &state.pool)
        .await?;
    } else {
        CustomMinecraftSkin::add(
            selected_credentials.offline_profile.id,
            &texture_key,
            &texture_blob,
            variant,
            cape_id,
            insert_position,
            &state.pool,
        )
        .await?;
    }

    if replace_texture && old_texture_key != texture_key {
        CustomMinecraftSkin {
            texture_key: old_texture_key.to_string(),
            variant: skin.variant,
            cape_id: skin.cape_id,
            display_order: 0,
        }
        .remove(selected_credentials.offline_profile.id, &state.pool)
        .await?;
    }

    skin.texture_key = texture_key;
    skin.variant = variant;
    skin.cape_id = cape_id;
    skin.texture = png_util::blob_to_data_url(texture_blob)
        .or_else(|| {
            png_util::blob_to_data_url(include_bytes!(
                "minecraft_skins/assets/default/MissingNo.png"
            ))
        })
        .unwrap();

    Ok(skin)
}

/// Unequips the currently equipped skin for the currently selected Minecraft profile, resetting
/// it to one of the default skins and unequipping any cape.
#[tracing::instrument]
pub async fn unequip_skin() -> crate::Result<()> {
    let state = State::get().await?;
    let selected_credentials = Credentials::get_default_credential(&state.pool)
        .await?
        .ok_or(ErrorKind::NoCredentialsError)?;
    ensure_skin_management_supported(&selected_credentials)?;
    if is_society_skin_site(&selected_credentials) {
        return ysumc_site::equip_texture(
            &selected_credentials,
            Some(0),
            Some(None),
        )
        .await;
    }
    if selected_credentials.is_offline() {
        OfflineMinecraftSkin::clear(
            selected_credentials.offline_profile.id,
            &state.pool,
        )
        .await?;
    } else {
        set_pending_skin_change(PendingSkinChange::Unequip {
            selected_credentials,
        })
        .await;
    }

    Ok(())
}

async fn unequip_skin_now(
    selected_credentials: &Credentials,
) -> crate::Result<()> {
    let state = State::get().await?;

    let profile = selected_credentials
        .online_profile_fresh()
        .await
        .ok_or_else(|| ErrorKind::OnlineMinecraftProfileUnavailable {
            user_name: selected_credentials.offline_profile.name.clone(),
        })?;

    preserve_current_profile_skin(&state, &profile).await?;

    mojang_api::MinecraftSkinOperation::unequip_any(selected_credentials)
        .await?;

    if let Err(error) = sync_cape(selected_credentials, &profile, None).await {
        refresh_profile_cache(selected_credentials).await;
        return Err(error);
    }

    Ok(())
}

/// Normalizes the texture of a Minecraft skin to the modern 64x64 format, handling
/// legacy 64x32 skins as the vanilla game client does. This function prioritizes
/// PNG encoding speed over compression density, so the resulting textures are better
/// suited for display purposes, not persistent storage or transmission.
///
/// The normalized texture is returned as PNG bytes.
#[tracing::instrument]
pub async fn normalize_skin_texture(
    texture: &UrlOrBlob,
) -> crate::Result<Bytes> {
    png_util::normalize_skin_texture(texture).await
}

/// Normalizes a skin for previews without applying Minecraft's inner-layer
/// opacity conversion.
#[tracing::instrument]
pub async fn normalize_skin_texture_for_preview(
    texture: &UrlOrBlob,
) -> crate::Result<Bytes> {
    png_util::normalize_skin_texture_for_preview(texture).await
}

/// Sends any pending skin change immediately.
///
/// This is used before launching Minecraft and before closing the app so the debounced
/// skin selection is still applied for those boundary cases.
#[tracing::instrument]
pub async fn flush_pending_skin_change() -> crate::Result<()> {
    flush_pending_skin_change_inner(None).await
}

/// Sends any pending skin change for a specific Minecraft account immediately.
#[tracing::instrument]
pub async fn flush_pending_skin_change_for_profile(
    profile_id: Uuid,
) -> crate::Result<()> {
    flush_pending_skin_change_inner(Some(PendingSkinChangeFilter::Profile(
        profile_id,
    )))
    .await
}

async fn set_pending_skin_change(change: PendingSkinChange) {
    let profile_id = change.profile_id();
    let generation = {
        let mut state = PENDING_SKIN_CHANGE.lock().await;
        let generation = state
            .pending
            .get(&profile_id)
            .map_or(1, |entry| entry.generation.wrapping_add(1));

        state
            .pending
            .insert(profile_id, PendingSkinChangeEntry { change, generation });

        generation
    };

    schedule_pending_skin_change_flush(profile_id, generation);
}

fn schedule_pending_skin_change_flush(profile_id: Uuid, generation: u64) {
    tokio::spawn(async move {
        tokio::time::sleep(SKIN_CHANGE_DEBOUNCE).await;

        if let Err(error) = flush_pending_skin_change_inner(Some(
            PendingSkinChangeFilter::Generation {
                profile_id,
                generation,
            },
        ))
        .await
        {
            let _ = crate::event::emit::emit_warning(&format!(
                "Failed to apply pending Minecraft skin change: {error}"
            ))
            .await;
        }
    });
}

async fn pending_effective_skin_change(
    profile_id: Uuid,
) -> Option<PendingEffectiveSkinChange> {
    let state = PENDING_SKIN_CHANGE.lock().await;

    state
        .pending
        .get(&profile_id)
        .map(|entry| match &entry.change {
            PendingSkinChange::AddAndEquipCustom {
                texture_blob,
                variant,
                cape_id,
                local_texture_key,
                ..
            } => PendingEffectiveSkinChange::AddAndEquipCustom {
                texture_key: Arc::clone(local_texture_key),
                texture_blob: Bytes::clone(texture_blob),
                variant: *variant,
                cape_id: *cape_id,
            },
            PendingSkinChange::Equip { skin, .. } => {
                PendingEffectiveSkinChange::Equip { skin: skin.clone() }
            }
            PendingSkinChange::Unequip { .. } => {
                PendingEffectiveSkinChange::Unequip
            }
        })
}

async fn cancel_pending_skin_change_for_skin(profile_id: Uuid, skin: &Skin) {
    let mut state = PENDING_SKIN_CHANGE.lock().await;
    let should_cancel = state
        .pending
        .get(&profile_id)
        .is_some_and(|entry| entry.change.matches_skin(skin));

    if should_cancel {
        state.pending.remove(&profile_id);
    }
}

#[derive(Clone, Copy, Debug)]
enum PendingSkinChangeFilter {
    Generation { profile_id: Uuid, generation: u64 },
    Profile(Uuid),
}

async fn flush_pending_skin_change_inner(
    filter: Option<PendingSkinChangeFilter>,
) -> crate::Result<()> {
    let _guard = SKIN_CHANGE_FLUSH_LOCK.lock().await;

    loop {
        let entry = {
            let mut state = PENDING_SKIN_CHANGE.lock().await;

            match filter {
                Some(PendingSkinChangeFilter::Generation {
                    profile_id,
                    generation,
                }) => {
                    let Some(entry) = state.pending.get(&profile_id) else {
                        return Ok(());
                    };

                    if entry.generation != generation {
                        return Ok(());
                    }

                    state.pending.remove(&profile_id)
                }
                Some(PendingSkinChangeFilter::Profile(profile_id)) => {
                    state.pending.remove(&profile_id)
                }
                None => {
                    let profile_id = state.pending.keys().next().copied();
                    profile_id.and_then(|profile_id| {
                        state.pending.remove(&profile_id)
                    })
                }
            }
        };

        let Some(entry) = entry else {
            return Ok(());
        };

        if let Err(error) = execute_pending_skin_change(&entry.change).await {
            let profile_id = entry.change.profile_id();
            let generation = entry.generation;
            let mut state = PENDING_SKIN_CHANGE.lock().await;
            state.pending.entry(profile_id).or_insert(entry);
            schedule_pending_skin_change_flush(profile_id, generation);

            return Err(error);
        }

        if filter.is_some() {
            return Ok(());
        }
    }
}

async fn execute_pending_skin_change(
    change: &PendingSkinChange,
) -> crate::Result<()> {
    match change {
        PendingSkinChange::AddAndEquipCustom {
            selected_credentials,
            texture_blob,
            variant,
            cape_id,
            local_texture_key,
        } => {
            add_and_equip_custom_skin_now(
                selected_credentials,
                Bytes::clone(texture_blob),
                *variant,
                *cape_id,
                local_texture_key,
            )
            .await
        }
        PendingSkinChange::Equip {
            selected_credentials,
            skin,
        } => equip_skin_now(selected_credentials, skin).await,
        PendingSkinChange::Unequip {
            selected_credentials,
        } => unequip_skin_now(selected_credentials).await,
    }
}

/// Reads and validates a skin texture file from the given path.
/// Returns the file content as bytes if it's a valid skin texture (PNG with 64x64 or 64x32 dimensions).
#[tracing::instrument]
pub async fn get_dragged_skin_data(
    path: &std::path::Path,
) -> crate::Result<Bytes> {
    if let Some(extension) = path.extension() {
        if extension.to_string_lossy().to_lowercase() != "png" {
            return Err(ErrorKind::InvalidSkinTexture.into());
        }
    } else {
        return Err(ErrorKind::InvalidSkinTexture.into());
    }

    tracing::debug!("Reading file: {:?}", path);

    if !path.exists() {
        tracing::error!("File does not exist: {:?}", path);
        return Err(ErrorKind::InvalidSkinTexture.into());
    }

    let data = match tokio::fs::read(path).await {
        Ok(data) => {
            tracing::debug!(
                "File read successfully, size: {} bytes",
                data.len()
            );
            data
        }
        Err(err) => {
            tracing::error!("Failed to read file: {}", err);
            return Err(err.into());
        }
    };

    let url_or_blob = UrlOrBlob::Blob(data.clone().into());

    match normalize_skin_texture(&url_or_blob).await {
        Ok(_) => Ok(data.into()),
        Err(err) => {
            tracing::error!("Failed to normalize skin texture: {}", err);
            Err(ErrorKind::InvalidSkinTexture.into())
        }
    }
}

async fn preserve_current_profile_skin(
    state: &State,
    profile: &MinecraftProfile,
) -> crate::Result<()> {
    let current_skin = profile.current_skin()?;
    let current_skin_texture_key = current_skin.texture_key();
    let current_cape_id = profile.current_cape().map(|cape| cape.id);

    if is_bundled_skin_texture(&current_skin_texture_key) {
        return Ok(());
    }

    if let Some(saved_skin) = CustomMinecraftSkin::get_by_texture(
        profile.id,
        &current_skin_texture_key,
        &state.pool,
    )
    .await?
    {
        if saved_skin.variant == current_skin.variant
            && saved_skin.cape_id == current_cape_id
        {
            return Ok(());
        }

        let texture = saved_skin.texture_blob(&state.pool).await?;
        CustomMinecraftSkin::add(
            profile.id,
            &current_skin_texture_key,
            &texture,
            current_skin.variant,
            current_cape_id,
            CustomMinecraftSkinInsertPosition::Bottom,
            &state.pool,
        )
        .await?;

        return Ok(());
    }

    let texture = png_util::url_to_data_stream(&current_skin.url)
        .await?
        .try_fold(Vec::new(), |mut texture, chunk| async move {
            texture.extend_from_slice(&chunk);
            Ok(texture)
        })
        .await?;

    CustomMinecraftSkin::add(
        profile.id,
        &current_skin_texture_key,
        &texture,
        current_skin.variant,
        current_cape_id,
        CustomMinecraftSkinInsertPosition::Bottom,
        &state.pool,
    )
    .await?;

    Ok(())
}

async fn refresh_profile_cache(selected_credentials: &Credentials) {
    let _ = selected_credentials.refresh_online_profile().await;
}

fn is_bundled_skin_texture(texture_key: &str) -> bool {
    assets::DEFAULT_SKINS
        .iter()
        .any(|default_skin| default_skin.texture_key.as_ref() == texture_key)
}

fn is_bundled_skin(texture_key: &str, variant: MinecraftSkinVariant) -> bool {
    assets::DEFAULT_SKINS.iter().any(|default_skin| {
        default_skin.texture_key.as_ref() == texture_key
            && default_skin.variant == variant
    })
}

fn get_fallback_default_skin() -> crate::Result<&'static Skin> {
    assets::DEFAULT_SKINS
        .iter()
        .find(|skin| {
            skin.name.as_deref() == Some("Steve")
                && skin.variant == MinecraftSkinVariant::Classic
        })
        .or_else(|| {
            assets::DEFAULT_SKINS
                .iter()
                .find(|skin| skin.name.as_deref() == Some("Steve"))
        })
        .or_else(|| assets::DEFAULT_SKINS.first())
        .ok_or_else(|| {
            ErrorKind::OtherError("No bundled default skins found".into())
                .as_error()
        })
}

fn local_skin_texture_key(texture_blob: &[u8]) -> Arc<str> {
    Arc::from(format!("local-{:x}", sha2::Sha256::digest(texture_blob)))
}

/// A stable, readable label for a texture the player just uploaded. The site shows
/// it in the closet, and the launcher has no other name to offer.
fn local_skin_name(texture_blob: &[u8]) -> String {
    let digest = format!("{:x}", sha2::Sha256::digest(texture_blob));
    format!("skin-{}", &digest[..8])
}

#[derive(Debug, PartialEq, Eq)]
struct SavedSkinSync {
    is_current_skin: bool,
    settings_changed: bool,
}

fn sync_saved_skin_with_current_profile(
    saved_skin: &mut CustomMinecraftSkin,
    current_skin_texture_key: &str,
    current_skin_variant: MinecraftSkinVariant,
    current_cape_id: Option<Uuid>,
) -> SavedSkinSync {
    if saved_skin.texture_key != current_skin_texture_key {
        return SavedSkinSync {
            is_current_skin: false,
            settings_changed: false,
        };
    }

    let settings_changed = saved_skin.variant != current_skin_variant
        || saved_skin.cape_id != current_cape_id;

    if settings_changed {
        saved_skin.variant = current_skin_variant;
        saved_skin.cape_id = current_cape_id;
    }

    SavedSkinSync {
        is_current_skin: true,
        settings_changed,
    }
}

/// Sets the equipped cape to the skin's associated cape, or no cape.
async fn sync_cape(
    selected_credentials: &Credentials,
    profile: &MinecraftProfile,
    target_cape_id: Option<Uuid>,
) -> crate::Result<()> {
    let current_cape_id = profile.current_cape().map(|cape| cape.id);

    if current_cape_id != target_cape_id {
        match target_cape_id {
            Some(cape_id) => {
                mojang_api::MinecraftCapeOperation::equip(
                    selected_credentials,
                    cape_id,
                )
                .await?
            }
            None => {
                mojang_api::MinecraftCapeOperation::unequip_any(
                    selected_credentials,
                )
                .await?
            }
        }
    }

    Ok(())
}
