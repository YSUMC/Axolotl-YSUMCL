//! Sign-in through the society account site using Yggdrasil Connect.
//!
//! The site runs Blessing Skin with the Yggdrasil Connect extension, which
//! replaces the password-based part of the Yggdrasil API with OpenID Connect.
//! The flow a launcher is meant to use is:
//!
//! 1. the Yggdrasil API metadata announces the OpenID provider through
//!    `meta.feature.openid_configuration_url`;
//! 2. a device authorization grant (RFC 8628) requests
//!    `openid offline_access Yggdrasil.PlayerProfiles.Select Yggdrasil.Server.Join`;
//! 3. the player picks a character on the provider's authorization page, so the
//!    issued access token is bound to that character;
//! 4. the access token *is* the Minecraft access token: the session server
//!    validates it directly and authlib-injector passes it to the game, so there
//!    is no second exchange step;
//! 5. the character and the account are read from the user information endpoint.
//!
//! The classic Yggdrasil username and password login stays available as a
//! fallback for deployments without the extension; see `super::yggdrasil`.

use super::{
    Credentials, MinecraftAccountType, MinecraftProfile, YggdrasilAccount,
};
use super::{YggdrasilProfile, yggdrasil};
use crate::util::fetch::INSECURE_REQWEST_CLIENT;
use crate::{ErrorKind, ysumc};
use chrono::{Duration, Utc};
use reqwest::StatusCode;
use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use sqlx::Sqlite;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration as StdDuration, Instant};
use tokio::sync::Mutex;
use uuid::Uuid;

/// How long a started sign-in may sit around before the player has to begin
/// again. The provider hands out device codes with their own, shorter, lifetime;
/// this only bounds the launcher's own bookkeeping.
const DEVICE_LOGIN_LIFETIME: StdDuration = StdDuration::from_secs(900);

/// Scopes Yggdrasil Connect requires an OpenID provider to support. A provider
/// missing any of them does not implement the protocol, and signing in through
/// it would fail later in a way that is much harder to explain.
const REQUIRED_PROVIDER_SCOPES: [&str; 3] = [
    "openid",
    "Yggdrasil.PlayerProfiles.Select",
    "Yggdrasil.Server.Join",
];

static PENDING_DEVICE_LOGINS: LazyLock<
    Mutex<HashMap<Uuid, PendingDeviceLogin>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

static PROVIDER_METADATA: LazyLock<Mutex<HashMap<String, ProviderMetadata>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A device authorization grant the player still has to approve.
#[derive(Serialize, Debug, Clone)]
pub struct YsumcDeviceLoginFlow {
    /// Handle the launcher polls with.
    pub flow_id: Uuid,
    /// Short code the player confirms on the site.
    pub user_code: String,
    /// Page where the code has to be entered.
    pub verification_uri: String,
    /// Same page with the code already filled in.
    pub verification_uri_complete: String,
    /// Remaining lifetime of the code, in seconds.
    pub expires_in: u64,
    /// How often the launcher polls, in seconds.
    pub interval: u64,
}

/// State of a sign-in that is waiting for the player.
#[derive(Serialize, Debug)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum YsumcDeviceLoginPoll {
    /// The player has not approved the launcher yet.
    Pending { slow_down: bool },
    /// The account is signed in.
    Complete { credentials: Credentials },
}

struct PendingDeviceLogin {
    created: Instant,
    device_code: String,
    client_id: String,
    interval: StdDuration,
    last_poll: Option<Instant>,
    tokens: Option<OidcTokens>,
}

#[derive(Clone, Debug)]
struct OidcTokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Clone, Debug)]
struct ProviderMetadata {
    device_authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
    client_id: String,
}

#[derive(Deserialize)]
struct RawProviderMetadata {
    issuer: String,
    #[serde(default)]
    device_authorization_endpoint: Option<String>,
    token_endpoint: String,
    #[serde(default)]
    userinfo_endpoint: Option<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
    #[serde(default)]
    shared_client_id: Option<String>,
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// The user information endpoint returns a superset of the ID token claims,
/// including the character the access token is bound to.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserInfoResponse {
    #[serde(default)]
    selected_profile: Option<YggdrasilProfile>,
}

#[derive(Deserialize)]
struct OAuthErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

enum DeviceTokenPoll {
    Pending { slow_down: bool },
    Complete(OidcTokens),
}

/// Starts a sign-in and returns the code the player confirms on the site.
#[tracing::instrument]
pub async fn begin_device_login() -> crate::Result<YsumcDeviceLoginFlow> {
    let provider = provider_metadata().await?;
    let response = request_device_code(&provider).await?;
    let flow_id = Uuid::new_v4();
    let interval = response.interval.unwrap_or(5).max(1);

    let mut pending = PENDING_DEVICE_LOGINS.lock().await;
    pending.retain(|_, login| login.created.elapsed() < DEVICE_LOGIN_LIFETIME);
    pending.insert(
        flow_id,
        PendingDeviceLogin {
            created: Instant::now(),
            device_code: response.device_code,
            client_id: provider.client_id.clone(),
            interval: StdDuration::from_secs(interval),
            last_poll: None,
            tokens: None,
        },
    );

    Ok(YsumcDeviceLoginFlow {
        flow_id,
        user_code: response.user_code,
        verification_uri: response.verification_uri,
        verification_uri_complete: response
            .verification_uri_complete
            .unwrap_or_default(),
        expires_in: response.expires_in,
        interval,
    })
}

/// Reports whether the player has approved the sign-in.
///
/// Safe to call repeatedly: once the provider has issued tokens, later calls
/// reuse them instead of asking again.
#[tracing::instrument(skip(exec))]
pub async fn poll_device_login(
    flow_id: Uuid,
    exec: impl sqlx::Executor<'_, Database = Sqlite> + Copy,
) -> crate::Result<YsumcDeviceLoginPoll> {
    let (device_code, client_id, tokens) = {
        let mut pending = PENDING_DEVICE_LOGINS.lock().await;
        pending
            .retain(|_, login| login.created.elapsed() < DEVICE_LOGIN_LIFETIME);
        let login =
            pending.get_mut(&flow_id).ok_or_else(device_login_expired)?;

        if login.tokens.is_none() {
            if let Some(last_poll) = login.last_poll
                && last_poll.elapsed() < login.interval
            {
                return Ok(YsumcDeviceLoginPoll::Pending { slow_down: false });
            }
            login.last_poll = Some(Instant::now());
        }

        (
            login.device_code.clone(),
            login.client_id.clone(),
            login.tokens.clone(),
        )
    };

    let tokens = match tokens {
        Some(tokens) => tokens,
        None => {
            let provider = provider_metadata().await?;
            match request_device_token(
                &provider.token_endpoint,
                &client_id,
                &device_code,
            )
            .await?
            {
                DeviceTokenPoll::Pending { slow_down } => {
                    if slow_down {
                        if let Some(login) =
                            PENDING_DEVICE_LOGINS.lock().await.get_mut(&flow_id)
                        {
                            login.interval += StdDuration::from_secs(5);
                        }
                    }
                    return Ok(YsumcDeviceLoginPoll::Pending { slow_down });
                }
                DeviceTokenPoll::Complete(tokens) => {
                    let mut pending = PENDING_DEVICE_LOGINS.lock().await;
                    if let Some(login) = pending.get_mut(&flow_id) {
                        login.tokens = Some(tokens.clone());
                    }
                    tokens
                }
            }
        }
    };

    let provider = provider_metadata().await?;
    let credentials = complete_login(&provider, &tokens, exec).await?;
    let _ = PENDING_DEVICE_LOGINS.lock().await.remove(&flow_id);

    Ok(YsumcDeviceLoginPoll::Complete { credentials })
}

/// Renews a session that was opened through the account site.
///
/// Called when the access token can no longer be used, which is what an OpenID
/// Connect refresh token is for. The provider may not return a new character, so
/// the stored one is kept unless the user information endpoint answers.
pub(super) async fn refresh_credentials_with_oidc(
    credentials: &mut Credentials,
    exec: impl sqlx::Executor<'_, Database = Sqlite> + Copy,
) -> crate::Result<()> {
    let provider = provider_metadata().await?;
    refresh_with_provider(credentials, &provider, exec).await
}

async fn refresh_with_provider(
    credentials: &mut Credentials,
    provider: &ProviderMetadata,
    exec: impl sqlx::Executor<'_, Database = Sqlite> + Copy,
) -> crate::Result<()> {
    let tokens =
        match request_refreshed_tokens(provider, &credentials.refresh_token)
            .await
        {
            Ok(tokens) => tokens,
            Err(error) => {
                if matches!(&*error.raw, ErrorKind::CredentialsExpired { .. }) {
                    sign_out_dead_session(credentials, exec).await;
                }
                return Err(error);
            }
        };

    let lifetime = token_lifetime(&tokens);
    credentials.access_token = tokens.access_token.clone();
    if let Some(refresh_token) = tokens.refresh_token.clone() {
        credentials.refresh_token = refresh_token;
    }
    credentials.expires = Utc::now() + lifetime;
    credentials.persist_refreshed(exec).await?;

    match fetch_selected_profile(
        &provider.userinfo_endpoint,
        &tokens.access_token,
    )
    .await
    {
        Ok(Some(profile)) => {
            credentials.offline_profile.id = profile.id;
            credentials.offline_profile.name = profile.name;
        }
        Ok(None) => {}
        Err(error) => tracing::warn!(
            "Keeping the stored character after a refresh; the user information endpoint did not answer: {error}"
        ),
    }

    credentials.persist_refreshed(exec).await?;

    Ok(())
}

/// Signs out a session the account site has stopped honouring.
///
/// The refresh token is what the site refused, so nothing the launcher can do
/// renews it; leaving the row behind would make every later action fail with the
/// same refusal and never ask the player to sign in again. The character's saved
/// skins are keyed by the character's UUID rather than by this row, so a new
/// sign-in for the same character finds them again.
async fn sign_out_dead_session(
    credentials: &Credentials,
    exec: impl sqlx::Executor<'_, Database = Sqlite> + Copy,
) {
    tracing::warn!(
        player = %credentials.offline_profile.name,
        "The account site refused to renew this sign-in; signing the account out so the player can sign in again"
    );

    if let Some(account_id) = credentials.account_id.as_deref()
        && let Err(error) = super::Credentials::remove(account_id, exec).await
    {
        tracing::warn!("Could not remove the expired account: {error}");
    }
}

async fn complete_login(
    provider: &ProviderMetadata,
    tokens: &OidcTokens,
    exec: impl sqlx::Executor<'_, Database = Sqlite> + Copy,
) -> crate::Result<Credentials> {
    let profile =
        fetch_selected_profile(&provider.userinfo_endpoint, &tokens.access_token)
            .await?
            .ok_or_else(|| {
                ErrorKind::OtherError(
                    "The account site did not report the character this sign-in is bound to. Start the sign-in again and pick a character on the authorization page."
                        .to_string(),
                )
                .as_error()
            })?;

    let server_name =
        yggdrasil::fetch_yggdrasil_metadata(ysumc::YGGDRASIL_API_ROOT)
            .await
            .map_or_else(
                |_| ysumc::SITE_LABEL.to_string(),
                |metadata| metadata.server_name,
            );

    let credentials = Credentials {
        account_id: Some(format!(
            "yggdrasil:{}:{}",
            ysumc::YGGDRASIL_API_ROOT,
            profile.id.as_hyphenated()
        )),
        offline_profile: MinecraftProfile {
            id: profile.id,
            name: profile.name,
            ..MinecraftProfile::default()
        },
        account_type: MinecraftAccountType::Yggdrasil,
        access_token: tokens.access_token.clone(),
        refresh_token: tokens.refresh_token.clone().unwrap_or_default(),
        expires: Utc::now() + token_lifetime(tokens),
        active: true,
        yggdrasil: Some(YggdrasilAccount {
            api_root: ysumc::YGGDRASIL_API_ROOT.to_string(),
            server_name,
            login: String::new(),
            client_token: String::new(),
        }),
    };
    credentials.upsert(exec).await?;

    Ok(credentials)
}

/// Lifetime of the freshly issued access token, with a conservative fallback for
/// providers that omit `expires_in`.
fn token_lifetime(tokens: &OidcTokens) -> Duration {
    // Never lengthen a short-lived token beyond the provider's expiry.
    tokens
        .expires_in
        .and_then(|seconds| i64::try_from(seconds).ok())
        .and_then(Duration::try_seconds)
        .unwrap_or_else(|| Duration::hours(1))
}

/// Reads the OpenID provider description the account site announces.
///
/// Cached per API root: it changes only when the site is reconfigured, and the
/// device flow needs its endpoints more than once.
async fn provider_metadata() -> crate::Result<ProviderMetadata> {
    if let Some(metadata) = PROVIDER_METADATA
        .lock()
        .await
        .get(ysumc::YGGDRASIL_API_ROOT)
        .cloned()
    {
        return Ok(metadata);
    }

    let api_metadata =
        yggdrasil::fetch_yggdrasil_metadata(ysumc::YGGDRASIL_API_ROOT).await?;
    let discovery_url = yggdrasil::openid_configuration_url(&api_metadata.raw)
        .ok_or_else(|| {
            ErrorKind::OtherError(
                "The account site does not announce an OpenID provider, so \
                 Yggdrasil Connect sign-in is unavailable. Sign in with the \
                 account name and password instead, or ask the site \
                 administrator to enable the Yggdrasil Connect feature."
                    .to_string(),
            )
            .as_error()
        })?;

    validate_provider_url(&discovery_url)?;
    let raw: RawProviderMetadata = get_json(&discovery_url).await?;
    if raw.issuer.trim_end_matches('/') != ysumc::JANUS_BASE_URL {
        return Err(ErrorKind::InputError(
            "Unexpected YSUMC OpenID issuer".into(),
        )
        .as_error());
    }

    let missing: Vec<&str> = REQUIRED_PROVIDER_SCOPES
        .iter()
        .copied()
        .filter(|scope| !raw.scopes_supported.iter().any(|it| it == scope))
        .collect();
    if !missing.is_empty() {
        return Err(ErrorKind::OtherError(format!(
            "The account site does not support Yggdrasil Connect (its OpenID provider is missing the {} scope(s)). Sign in with the account name and password instead.",
            missing.join(", ")
        ))
        .as_error());
    }

    let device_authorization_endpoint =
        raw.device_authorization_endpoint.ok_or_else(|| {
            ErrorKind::OtherError(
                "The account site does not offer the device authorization \
                 grant, which is how this launcher signs in without asking for \
                 your password. Sign in with the account name and password \
                 instead."
                    .to_string(),
            )
            .as_error()
        })?;
    let userinfo_endpoint = raw.userinfo_endpoint.ok_or_else(|| {
        ErrorKind::OtherError(
            "The account site does not offer the user information endpoint, so \
             the launcher cannot tell which character to start the game with."
                .to_string(),
        )
        .as_error()
    })?;

    let metadata = ProviderMetadata {
        device_authorization_endpoint,
        token_endpoint: raw.token_endpoint,
        userinfo_endpoint,
        client_id: raw
            .shared_client_id
            .unwrap_or_else(|| ysumc::OIDC_CLIENT_ID.to_string()),
    };

    for endpoint in [
        &metadata.device_authorization_endpoint,
        &metadata.token_endpoint,
        &metadata.userinfo_endpoint,
    ] {
        validate_provider_url(endpoint)?;
    }
    if metadata.client_id.trim().is_empty() {
        return Err(ErrorKind::InputError(
            "YSUMC OpenID metadata must publish shared_client_id".into(),
        )
        .as_error());
    }
    PROVIDER_METADATA
        .lock()
        .await
        .insert(ysumc::YGGDRASIL_API_ROOT.to_string(), metadata.clone());

    Ok(metadata)
}

/// The character the access token is bound to.
///
/// The user information endpoint is the spec's own way of validating an access
/// token, so a successful answer here also proves the token still works.
async fn fetch_selected_profile(
    userinfo_endpoint: &str,
    access_token: &str,
) -> crate::Result<Option<YggdrasilProfile>> {
    let response = INSECURE_REQWEST_CLIENT
        .get(userinfo_endpoint)
        .header(ACCEPT, "application/json")
        .header(reqwest::header::ACCEPT_LANGUAGE, "zh-CN")
        .bearer_auth(access_token)
        .timeout(StdDuration::from_secs(30))
        .send()
        .await?;

    if matches!(response.status().as_u16(), 401 | 403) {
        return Err(ErrorKind::CredentialsExpired {
            detail: "The account site rejected this access token.".into(),
        }
        .as_error());
    }
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }

    let userinfo: UserInfoResponse = response.json().await?;
    Ok(userinfo.selected_profile)
}

pub(super) async fn validate_credentials(
    credentials: &Credentials,
) -> crate::Result<bool> {
    validate_with_provider(credentials, &provider_metadata().await?).await
}

async fn validate_with_provider(
    credentials: &Credentials,
    provider: &ProviderMetadata,
) -> crate::Result<bool> {
    // Connect deployments may disable the entire classic authserver, including
    // validate. Userinfo checks expiry, revocation and the selected character.
    match fetch_selected_profile(
        &provider.userinfo_endpoint,
        &credentials.access_token,
    )
    .await
    {
        Ok(Some(profile)) => Ok(profile.id == credentials.offline_profile.id
            && profile.name == credentials.offline_profile.name),
        Ok(None) => Ok(false),
        Err(error)
            if matches!(&*error.raw, ErrorKind::CredentialsExpired { .. }) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

async fn request_device_code(
    provider: &ProviderMetadata,
) -> crate::Result<DeviceCodeResponse> {
    let response = INSECURE_REQWEST_CLIENT
        .post(&provider.device_authorization_endpoint)
        .header(ACCEPT, "application/json")
        .form(&[
            ("client_id", provider.client_id.as_str()),
            ("scope", ysumc::OIDC_SCOPE),
        ])
        .timeout(StdDuration::from_secs(30))
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(oauth_error(status, &body));
    }

    serde_json::from_str(&body).map_err(|error| {
        ErrorKind::OtherError(format!(
            "The account site returned an unexpected sign-in response: {error}"
        ))
        .as_error()
    })
}

async fn request_device_token(
    token_endpoint: &str,
    client_id: &str,
    device_code: &str,
) -> crate::Result<DeviceTokenPoll> {
    let response = INSECURE_REQWEST_CLIENT
        .post(token_endpoint)
        .header(ACCEPT, "application/json")
        .form(&[
            ("grant_type", ysumc::OIDC_DEVICE_CODE_GRANT_TYPE),
            ("client_id", client_id),
            ("device_code", device_code),
        ])
        .timeout(StdDuration::from_secs(30))
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    if let Ok(error) = serde_json::from_str::<OAuthErrorResponse>(&body) {
        return match error.error.as_str() {
            "authorization_pending" => {
                Ok(DeviceTokenPoll::Pending { slow_down: false })
            }
            "slow_down" => Ok(DeviceTokenPoll::Pending { slow_down: true }),
            "expired_token" => Err(ErrorKind::InputError(
                "The YSUMC sign-in request expired; start it again."
                    .to_string(),
            )
            .as_error()),
            _ => Err(oauth_error(status, &body)),
        };
    }

    if !status.is_success() {
        return Err(oauth_error(status, &body));
    }

    Ok(DeviceTokenPoll::Complete(parse_tokens(&body)?))
}

async fn request_refreshed_tokens(
    provider: &ProviderMetadata,
    refresh_token: &str,
) -> crate::Result<OidcTokens> {
    let response = INSECURE_REQWEST_CLIENT
        .post(&provider.token_endpoint)
        .header(ACCEPT, "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", provider.client_id.as_str()),
            ("refresh_token", refresh_token),
        ])
        .timeout(StdDuration::from_secs(30))
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        if matches!(status.as_u16(), 400 | 401 | 403)
            && let Some(detail) = refused_session_detail(&body)
        {
            return Err(ErrorKind::CredentialsExpired { detail }.as_error());
        }
        return Err(oauth_error(status, &body));
    }

    parse_tokens(&body)
}

fn parse_tokens(body: &str) -> crate::Result<OidcTokens> {
    let token: TokenResponse = serde_json::from_str(body).map_err(|error| {
        ErrorKind::OtherError(format!(
            "The account site returned an unexpected token response: {error}"
        ))
        .as_error()
    })?;

    Ok(OidcTokens {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_in: token.expires_in,
    })
}

async fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> crate::Result<T> {
    let response = INSECURE_REQWEST_CLIENT
        .get(url)
        .header(ACCEPT, "application/json")
        .timeout(StdDuration::from_secs(30))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    Ok(response.json().await?)
}

/// The OAuth error code and the human-readable detail the provider sent, if the
/// body is an OAuth error at all.
fn oauth_error_parts(body: &str) -> Option<(String, String)> {
    let error = serde_json::from_str::<OAuthErrorResponse>(body).ok()?;
    let detail = match error.error_description {
        Some(description) if !description.trim().is_empty() => {
            format!("{}: {description}", error.error)
        }
        _ => error.error.clone(),
    };

    Some((error.error, detail))
}

/// Whether a token endpoint refusal means the stored session is gone for good.
///
/// Only the two codes that name the grant itself qualify: the launcher signs the
/// account out on them, so a rate limit, a gateway error or a maintenance page
/// must never be mistaken for an expired session — that would ask every player
/// to sign in again because the site had a bad minute.
fn refused_session_detail(body: &str) -> Option<String> {
    let (code, detail) = oauth_error_parts(body)?;

    matches!(code.as_str(), "invalid_grant" | "invalid_token").then_some(detail)
}

/// Turns an OAuth failure into an error that says what to do about it.
fn oauth_error(status: StatusCode, body: &str) -> crate::Error {
    let (code, detail) = oauth_error_parts(body).unwrap_or_else(|| {
        (
            String::new(),
            if body.trim().is_empty() {
                format!("HTTP {status}")
            } else {
                format!("HTTP {status}: {body}")
            },
        )
    });

    let hint = match code.as_str() {
        "invalid_scope" => {
            " The account site has to allow the launcher to request \
             `openid`, `Yggdrasil.PlayerProfiles.Select` and \
             `Yggdrasil.Server.Join`."
        }
        "invalid_client" => {
            " The launcher's client ID has to be allowed to use the \
             device authorization grant on the account site."
        }
        "access_denied" => {
            " The sign-in was refused on the authorization page."
        }
        _ => "",
    };

    ErrorKind::OtherError(format!(
        "The YSUMC account site rejected the sign-in ({detail}).{hint}"
    ))
    .as_error()
}

async fn response_error(response: reqwest::Response) -> crate::Error {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<OAuthErrorResponse>(&body)
        .ok()
        .and_then(|error| {
            error
                .error_description
                .or(Some(error.error))
                .filter(|message| !message.trim().is_empty())
        })
        .unwrap_or_else(|| {
            if body.trim().is_empty() {
                format!("The account site returned HTTP {status}")
            } else {
                format!("The account site returned HTTP {status}: {body}")
            }
        });
    ErrorKind::OtherError(message).as_error()
}

fn device_login_expired() -> crate::Error {
    ErrorKind::InputError(
        "The YSUMC sign-in request expired; start it again.".to_string(),
    )
    .as_error()
}

#[cfg(test)]
mod tests {
    use super::super::refresh_tests::{mock_http, saved_account};
    use super::*;
    use serde_json::json;

    fn mock_provider(root: &str) -> ProviderMetadata {
        ProviderMetadata {
            token_endpoint: format!("{root}/token"),
            userinfo_endpoint: format!("{root}/userinfo"),
            device_authorization_endpoint: format!("{root}/device"),
            client_id: "launcher-test".into(),
        }
    }

    #[tokio::test]
    async fn expired_access_token_rotates_without_interactive_login() {
        let (pool, mut account) =
            saved_account(ysumc::YGGDRASIL_API_ROOT, true).await;
        let mut server = mock_http(vec![
            Some((200, json!({"access_token":"renewed-access", "refresh_token":"renewed-refresh", "expires_in":3600}))),
            Some((200, json!({"selectedProfile":{"id":account.offline_profile.id.simple().to_string(),"name":"Renamed"}}))),
        ]).await;
        let provider = mock_provider(&server.root);
        refresh_with_provider(&mut account, &provider, &pool)
            .await
            .unwrap();
        let request = server.requests.recv().await.unwrap();
        assert!(request.starts_with("POST /token "));
        let form = url::form_urlencoded::parse(
            request.split("\r\n\r\n").nth(1).unwrap().as_bytes(),
        )
        .into_owned()
        .collect::<HashMap<_, _>>();
        assert_eq!(form["grant_type"], "refresh_token");
        assert_eq!(form["refresh_token"], "old-refresh");
        assert_eq!(form["client_id"], "launcher-test");
        let userinfo =
            server.requests.recv().await.unwrap().to_ascii_lowercase();
        assert!(userinfo.contains("authorization: bearer renewed-access"));
        let stored = Credentials::get_active_without_refresh(&pool)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.access_token, "renewed-access");
        assert_eq!(stored.refresh_token, "renewed-refresh");
        assert_eq!(stored.offline_profile.name, "Renamed");
        assert!(!stored.needs_refresh());
    }

    #[tokio::test]
    async fn rotated_tokens_survive_cancellation_during_profile_lookup() {
        let (pool, mut account) =
            saved_account(ysumc::YGGDRASIL_API_ROOT, true).await;
        let mut server = mock_http(vec![
            Some((200, json!({"access_token":"renewed-access", "refresh_token":"renewed-refresh", "expires_in":3600}))),
            None,
        ]).await;
        let provider = mock_provider(&server.root);
        // A user switching accounts while this request is in flight must remain
        // on their new selection after the old account's token is persisted.
        sqlx::query("UPDATE minecraft_users SET active = FALSE")
            .execute(&pool)
            .await
            .unwrap();
        {
            let refresh = refresh_with_provider(&mut account, &provider, &pool);
            tokio::pin!(refresh);
            tokio::select! {
                result = &mut refresh => panic!("profile lookup should remain pending: {result:?}"),
                ready = tokio::time::timeout(StdDuration::from_secs(5), async {
                    assert!(server.requests.recv().await.unwrap().starts_with("POST /token "));
                    assert!(server.requests.recv().await.unwrap().starts_with("GET /userinfo "));
                }) => ready.unwrap(),
            }
        }
        let stored = Credentials::get_all_without_refresh(&pool)
            .await
            .unwrap()
            .remove(0);
        assert_eq!(stored.refresh_token, "renewed-refresh");
        assert_eq!(stored.access_token, "renewed-access");
        assert!(!stored.active);
    }

    #[tokio::test]
    async fn transient_server_errors_do_not_remove_saved_login() {
        for status in [429, 500, 502] {
            let (pool, mut account) =
                saved_account(ysumc::YGGDRASIL_API_ROOT, true).await;
            let server = mock_http(vec![Some((
                status,
                json!({"error":"invalid_token"}),
            ))])
            .await;
            let error = refresh_with_provider(
                &mut account,
                &mock_provider(&server.root),
                &pool,
            )
            .await
            .unwrap_err();
            assert!(!matches!(
                &*error.raw,
                ErrorKind::CredentialsExpired { .. }
            ));
            let stored = Credentials::get_active_without_refresh(&pool)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(stored.refresh_token, "old-refresh");
        }
    }

    #[tokio::test]
    async fn revoked_refresh_token_requires_new_authorization() {
        let (pool, mut account) =
            saved_account(ysumc::YGGDRASIL_API_ROOT, true).await;
        let server =
            mock_http(vec![Some((400, json!({"error":"invalid_grant"})))])
                .await;
        let error = refresh_with_provider(
            &mut account,
            &mock_provider(&server.root),
            &pool,
        )
        .await
        .unwrap_err();
        assert!(matches!(&*error.raw, ErrorKind::CredentialsExpired { .. }));
        assert!(
            Credentials::get_active_without_refresh(&pool)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn omitted_refresh_token_and_failed_profile_lookup_keep_session_usable()
     {
        let (pool, mut account) =
            saved_account(ysumc::YGGDRASIL_API_ROOT, true).await;
        let server = mock_http(vec![
            Some((
                200,
                json!({"access_token":"renewed-access","expires_in":3600}),
            )),
            Some((503, json!({"error":"temporarily_unavailable"}))),
        ])
        .await;
        refresh_with_provider(
            &mut account,
            &mock_provider(&server.root),
            &pool,
        )
        .await
        .unwrap();
        let stored = Credentials::get_active_without_refresh(&pool)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.access_token, "renewed-access");
        assert_eq!(stored.refresh_token, "old-refresh");
        assert_eq!(stored.offline_profile.name, "BeforeRefresh");
    }

    #[test]
    fn respects_short_token_lifetime() {
        assert_eq!(
            token_lifetime(&OidcTokens {
                access_token: "test".into(),
                refresh_token: None,
                expires_in: Some(10)
            }),
            Duration::seconds(10)
        );
    }

    #[tokio::test]
    async fn connect_validation_uses_userinfo_and_distinguishes_expiry_from_outage()
     {
        let (_pool, account) =
            saved_account(ysumc::YGGDRASIL_API_ROOT, true).await;
        for (status, expected) in [
            (200, Some(true)),
            (401, Some(false)),
            (403, Some(false)),
            (503, None),
        ] {
            let mut server = mock_http(vec![Some((status, json!({"selectedProfile": {
                "id": account.offline_profile.id.simple().to_string(), "name": account.offline_profile.name,
            }})))]).await;
            let result =
                validate_with_provider(&account, &mock_provider(&server.root))
                    .await;
            assert_eq!(result.ok(), expected);
            let request =
                server.requests.recv().await.unwrap().to_ascii_lowercase();
            assert!(request.starts_with("get /userinfo "));
            assert!(request.contains("authorization: bearer old-access"));
        }
    }

    #[test]
    fn a_refused_grant_is_the_only_refusal_that_ends_the_session() {
        let invalid_grant = concat!(
            r#"{"error":"invalid_grant","#,
            r#""error_description":"grant request is invalid"}"#
        );
        assert_eq!(
            refused_session_detail(invalid_grant),
            Some("invalid_grant: grant request is invalid".to_string())
        );

        let invalid_token = r#"{"error":"invalid_token"}"#;
        assert!(refused_session_detail(invalid_token).is_some());
    }

    #[test]
    fn a_site_hiccup_keeps_the_stored_session() {
        let hiccups = [
            r#"{"error":"invalid_client"}"#,
            r#"{"error":"invalid_scope"}"#,
            r#"{"error":"access_denied"}"#,
            "<html>502 Bad Gateway</html>",
            "",
            "{}",
        ];

        for body in hiccups {
            assert_eq!(refused_session_detail(body), None, "body: {body}");
        }
    }
}

fn validate_provider_url(value: &str) -> crate::Result<()> {
    let url = url::Url::parse(value).map_err(|_| {
        ErrorKind::InputError("Invalid YSUMC provider URL".into())
    })?;
    if url.scheme() != "https"
        || url.host_str() != Some("skin.ysumc.net")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ErrorKind::InputError(
            "YSUMC provider endpoints must use https://skin.ysumc.net".into(),
        )
        .as_error());
    }
    Ok(())
}
