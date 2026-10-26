//! Account-scoped requests to the pinned YSUMC launcher API.
use crate::State;
use crate::state::Credentials;
use futures::StreamExt;
use reqwest::{Method, Url};
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Document {
    Identity,
    Announcements,
    Releases,
}

fn invalid(message: &str) -> crate::Error {
    crate::ErrorKind::InputError(message.into()).into()
}

pub fn is_api_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("skin.ysumc.net")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.path().starts_with("/api/launcher/")
            && url.fragment().is_none()
    })
}

pub async fn account_scope() -> crate::Result<Option<String>> {
    Ok(credentials().await?.map(|account| account.account_id()))
}

async fn credentials() -> crate::Result<Option<Credentials>> {
    if crate::branding::active().id != "ysumc" {
        return Ok(None);
    }
    let state = State::get().await?;
    Ok(Credentials::get_active_without_refresh(&state.pool)
        .await?
        .filter(|account| {
            account.is_yggdrasil()
                && account.yggdrasil.as_ref().is_some_and(|provider| {
                    crate::ysumc::is_yggdrasil_api_root(&provider.api_root)
                })
        }))
}

pub async fn authorization(url: &str) -> crate::Result<Option<String>> {
    if !is_api_url(url) {
        return Ok(None);
    }
    let Some(mut account) = credentials().await? else {
        return Ok(None);
    };
    let state = State::get().await?;
    account.refresh_inner(&state.pool, false).await?;
    Ok(Some(format!("Bearer {}", account.access_token)))
}

async fn request(
    url: &str,
    method: Method,
) -> crate::Result<reqwest::Response> {
    if !is_api_url(url) {
        return Err(invalid("Unexpected launcher API URL"));
    }
    let state = State::get().await?;
    let mut account = credentials().await?;
    let scope = account.as_ref().map(|account| account.account_id());
    // Ignore the optional insecure transport setting for authenticated traffic.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()?;
    for retry in 0..2 {
        let mut request = client
            .request(method.clone(), url)
            .header("Accept", "application/json");
        if let Some(account) = account.as_mut() {
            account.refresh_inner(&state.pool, retry > 0).await?;
            request = request.bearer_auth(&account.access_token);
        }
        let response = request.send().await?;
        if scope != account_scope().await? {
            return Err(invalid("Launcher account changed; refresh content"));
        }
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            && account.is_some()
            && retry == 0
        {
            continue;
        }
        return Ok(response);
    }
    Err(invalid("Launcher authentication failed"))
}

pub async fn fetch_text(url: &str) -> crate::Result<String> {
    let response = request(url, Method::GET).await?;
    if !response.status().is_success() {
        return Err(crate::ErrorKind::HttpError {
            status: response.status().as_u16(),
            method: "GET".into(),
            url: url.into(),
        }
        .into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
            return Err(invalid("Launcher document is too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes)
        .map_err(|_| invalid("Invalid launcher document encoding"))
}

pub async fn document(
    kind: Document,
    version: Option<String>,
    channel: Option<String>,
) -> crate::Result<serde_json::Value> {
    let path = match kind {
        Document::Identity => "identity",
        Document::Announcements => "announcements",
        Document::Releases => "update/releases",
    };
    let mut url =
        Url::parse(&format!("{}/api/launcher/{path}", crate::ysumc::SITE_URL))?;
    for (key, value) in [("version", version), ("channel", channel)] {
        if let Some(value) = value {
            if value.len() > 100 {
                return Err(invalid("Invalid launcher request parameter"));
            }
            url.query_pairs_mut().append_pair(key, &value);
        }
    }
    Ok(serde_json::from_str(&fetch_text(url.as_str()).await?)?)
}

/// Authorize the private pack route before handing a public CDN URL to the
/// downloader. Never forward the account token to that redirect destination.
pub(crate) async fn pack_download(
    url: &str,
) -> crate::Result<(String, Option<(String, String)>)> {
    if !is_api_url(url)
        || !Url::parse(url)?.path().starts_with("/api/launcher/packs/")
    {
        return Ok((url.into(), None));
    }
    let response = request(url, Method::HEAD).await?;
    if response.status().is_redirection() {
        let target = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| invalid("Missing pack redirect"))?;
        let target = Url::parse(url)?.join(target)?;
        if target.scheme() != "https"
            || !target.username().is_empty()
            || target.password().is_some()
        {
            return Err(invalid("Unsafe pack redirect"));
        }
        return Ok((target.to_string(), None));
    }
    if !response.status().is_success() {
        return Err(invalid("This account cannot download this pack"));
    }
    Ok((
        url.into(),
        authorization(url)
            .await?
            .map(|value| ("Authorization".into(), value)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_only_sent_to_the_pinned_https_api() {
        assert!(is_api_url("https://skin.ysumc.net/api/launcher/identity"));
        for url in [
            "http://skin.ysumc.net/api/launcher/identity",
            "https://skin.ysumc.net.evil.test/api/launcher/identity",
            "https://evil.test/api/launcher/identity",
            "https://user@skin.ysumc.net/api/launcher/identity",
            "https://skin.ysumc.net:444/api/launcher/identity",
            "https://skin.ysumc.net/releases/file.exe",
            "https://skin.ysumc.net/api/launcher/../../external",
        ] {
            assert!(!is_api_url(url), "{url}");
        }
    }
}
