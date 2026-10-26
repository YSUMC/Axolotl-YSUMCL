//! mc-interop v1 adapter. Only YSUMC discovers peers; the desktop reads one
//! authenticated home catalog plus its public friends endpoint. Credentials are
//! sent only to the pinned home-site API.
use super::*;
use futures::StreamExt;
use serde_json::Value;
use std::{io::Write, time::Duration};

const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Site {
    pub id: String,
    pub name: String,
    pub domain: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub site_id: String,
    pub domain: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub stale: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    site: Site,
    packs: Vec<Value>,
    #[serde(default)]
    peers: Vec<Value>,
    updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    id: String,
    name: String,
    summary: Option<String>,
    revision: Option<i64>,
    game_version: String,
    loader: String,
    loader_version: Option<String>,
    artifact: Option<Artifact>,
    server: ManagedServerSpec,
    source: Option<Source>,
}

#[derive(Deserialize)]
struct Artifact {
    format: String,
    url: String,
    size: u64,
    sha1: Option<String>,
    sha512: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    etag: Option<String>,
    body: String,
}

#[derive(Clone, Copy)]
enum CatalogKind {
    Site,
    Friends,
}

impl CatalogKind {
    fn parse(self, body: &str) -> crate::Result<ManagedManifest> {
        match self {
            Self::Site => parse(body),
            Self::Friends => parse_friends(body),
        }
    }
}

#[derive(Deserialize)]
struct FriendsDocument {
    schema: u32,
    generated_at: String,
    sites: Vec<Value>,
}

#[derive(Deserialize)]
struct FriendSite {
    site_id: String,
    name: String,
    description: Option<String>,
    domain: String,
    #[serde(default)]
    stale: bool,
    instances: Vec<Value>,
}

#[derive(Deserialize)]
struct FriendInstance {
    id: String,
    name: String,
    description: Option<String>,
    revision: Option<i64>,
    minecraft: ManagedMinecraftSpec,
    pack: Option<ManagedPackSpec>,
    server: ManagedServerSpec,
}

fn invalid(message: impl Into<String>) -> crate::Error {
    crate::ErrorKind::InputError(message.into()).into()
}

pub(super) fn valid_hashes(pack: &ManagedPackSpec) -> bool {
    fn valid(value: &Option<String>, len: usize) -> bool {
        value.as_ref().is_none_or(|hash| {
            hash.len() == len && hash.bytes().all(|b| b.is_ascii_hexdigit())
        })
    }
    (pack.sha1.is_some() || pack.sha512.is_some())
        && valid(&pack.sha1, 40)
        && valid(&pack.sha512, 128)
}

fn valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

pub(super) fn validate_server_spec(
    spec: &ManagedInstanceSpec,
) -> crate::Result<()> {
    let server = spec
        .server
        .as_ref()
        .ok_or_else(|| invalid("Missing server"))?;
    let address = server.address.trim_matches(['[', ']']);
    if !(valid_domain(address) || address.parse::<std::net::IpAddr>().is_ok())
        || server.port == Some(0)
    {
        return Err(invalid("Invalid server address or port"));
    }
    Ok(())
}

fn convert(entry: Entry, index: usize) -> crate::Result<ManagedInstanceSpec> {
    validate_server_instance_id(&entry.id)?;
    if !matches!(
        entry.loader.as_str(),
        "vanilla" | "fabric" | "forge" | "neoforge" | "quilt"
    ) {
        return Err(invalid("Unsupported loader"));
    }
    if let Some(source) = &entry.source {
        if source.site_id.contains(':')
            || !valid_domain(&source.domain)
            || !entry.id.starts_with(&format!("{}:", source.site_id))
        {
            return Err(invalid("Invalid peer namespace"));
        }
    } else if entry.id.contains(':') {
        return Err(invalid("Namespaced entry has no source"));
    }
    let (pack, revision) = match entry.artifact {
        Some(artifact) => (
            ManagedPackSpec {
                kind: artifact.format,
                url: artifact.url,
                sha1: artifact.sha1,
                sha512: artifact.sha512,
                size: Some(artifact.size),
            },
            entry
                .revision
                .ok_or_else(|| invalid("Missing pack revision"))?,
        ),
        None => (
            ManagedPackSpec {
                kind: "server".into(),
                url: String::new(),
                sha1: None,
                sha512: None,
                size: None,
            },
            1,
        ),
    };
    let spec = ManagedInstanceSpec {
        id: entry.id,
        name: entry.name,
        source: entry.source,
        description: entry.summary,
        icon_url: None,
        sort: index as i32,
        revision,
        minecraft: ManagedMinecraftSpec {
            game_version: entry.game_version,
            loader: Some(entry.loader),
            loader_version: entry.loader_version,
        },
        pack,
        server: Some(entry.server),
        required: false,
    };
    validate_managed_spec(&spec)?;
    validate_server_spec(&spec)?;
    Ok(spec)
}

fn parse(body: &str) -> crate::Result<ManagedManifest> {
    if body.len() > MAX_DOCUMENT_BYTES {
        return Err(invalid("YSUMC catalog exceeds 1 MiB"));
    }
    let doc: Document = serde_json::from_str(body)?;
    if doc.site.id != "ysumc"
        || doc.site.domain != "skin.ysumc.net"
        || doc.site.name.trim().is_empty()
    {
        return Err(invalid("Unexpected YSUMC catalog identity"));
    }
    chrono::DateTime::parse_from_rfc3339(&doc.updated_at)
        .map_err(|_| invalid("Invalid catalog update time"))?;
    let mut ids = HashSet::new();
    let mut instances = Vec::new();
    let mut diagnostics = Vec::new();
    let peers: Vec<Source> = doc
        .peers
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect();
    for (index, value) in doc.packs.into_iter().enumerate() {
        let result = serde_json::from_value::<Entry>(value)
            .map_err(crate::Error::from)
            .and_then(|entry| convert(entry, index));
        match result {
            Ok(mut spec) if ids.insert(spec.id.clone()) => {
                if let Some(source) = spec.source.as_mut() {
                    if let Some(peer) = peers.iter().find(|peer| {
                        peer.site_id == source.site_id
                            && peer.domain == source.domain
                    }) {
                        source.name.clone_from(&peer.name);
                        source.description.clone_from(&peer.description);
                    }
                }
                instances.push(spec);
            }
            Ok(_) => diagnostics.push(format!("entry {index}: duplicate id")),
            Err(error) => diagnostics.push(format!("entry {index}: {error}")),
        }
    }
    for diagnostic in &diagnostics {
        tracing::warn!(%diagnostic, "Skipped invalid interop entry");
    }
    Ok(ManagedManifest {
        site: Some(doc.site),
        diagnostics,
        schema: SUPPORTED_MANIFEST_SCHEMA,
        generated_at: Some(doc.updated_at),
        launcher: None,
        cloud_map_url: None,
        instances,
    })
}

fn parse_friends(body: &str) -> crate::Result<ManagedManifest> {
    if body.len() > MAX_DOCUMENT_BYTES {
        return Err(invalid("YSUMC friends catalog exceeds 1 MiB"));
    }
    let doc: FriendsDocument = serde_json::from_str(body)?;
    if doc.schema != SUPPORTED_MANIFEST_SCHEMA {
        return Err(invalid("Unsupported friends schema"));
    }
    chrono::DateTime::parse_from_rfc3339(&doc.generated_at)
        .map_err(|_| invalid("Invalid friends update time"))?;
    let mut catalog = ManagedManifest {
        site: None,
        diagnostics: Vec::new(),
        schema: doc.schema,
        generated_at: Some(doc.generated_at),
        launcher: None,
        cloud_map_url: None,
        instances: Vec::new(),
    };
    let mut ids = HashSet::new();
    for (site_index, value) in doc.sites.into_iter().enumerate() {
        let site: FriendSite = match serde_json::from_value(value) {
            Ok(site) => site,
            Err(error) => {
                catalog
                    .diagnostics
                    .push(format!("friend {site_index}: {error}"));
                continue;
            }
        };
        if validate_server_instance_id(&site.site_id).is_err()
            || site.site_id.contains(':')
            || site.site_id == "ysumc"
            || !valid_domain(&site.domain)
            || site.domain.eq_ignore_ascii_case("skin.ysumc.net")
            || site.name.trim().is_empty()
        {
            catalog
                .diagnostics
                .push(format!("friend {site_index}: invalid identity"));
            continue;
        }
        let source = Source {
            site_id: site.site_id,
            domain: site.domain,
            name: site.name,
            description: site.description.unwrap_or_default(),
            stale: site.stale,
        };
        for (index, value) in site.instances.into_iter().enumerate() {
            let converted = (|| {
                let item: FriendInstance = serde_json::from_value(value)?;
                let artifact = match item.pack {
                    Some(pack) => Some(Artifact {
                        format: pack.kind,
                        url: pack.url,
                        size: pack
                            .size
                            .ok_or_else(|| invalid("Missing pack size"))?,
                        sha1: pack.sha1,
                        sha512: pack.sha512,
                    }),
                    None => None,
                };
                convert(
                    Entry {
                        id: item.id,
                        name: item.name,
                        summary: item.description,
                        revision: item.revision,
                        game_version: item.minecraft.game_version,
                        loader: item
                            .minecraft
                            .loader
                            .unwrap_or_else(|| "vanilla".into()),
                        loader_version: item.minecraft.loader_version,
                        artifact,
                        server: item.server,
                        source: Some(source.clone()),
                    },
                    catalog.instances.len(),
                )
            })();
            match converted {
                Ok(spec) if ids.insert(spec.id.clone()) => {
                    catalog.instances.push(spec)
                }
                Ok(_) => catalog.diagnostics.push(format!(
                    "friend {site_index} entry {index}: duplicate id"
                )),
                Err(error) => catalog.diagnostics.push(format!(
                    "friend {site_index} entry {index}: {error}"
                )),
            }
        }
    }
    Ok(catalog)
}

// A valid empty friends response intentionally removes peers. During a rolling
// server upgrade this also replaces, rather than duplicates, legacy source rows.
fn merge_friends(catalog: &mut ManagedManifest, friends: ManagedManifest) {
    catalog.instances.retain(|spec| spec.source.is_none());
    catalog.instances.extend(friends.instances);
    catalog.diagnostics.extend(friends.diagnostics);
}

// A 304 is only usable together with a successfully parsed persistent snapshot.
async fn request(
    client: &reqwest::Client,
    endpoint: &str,
    cached: Option<&Snapshot>,
    kind: CatalogKind,
) -> crate::Result<Snapshot> {
    let mut request = client
        .get(endpoint)
        .timeout(Duration::from_secs(15))
        .header(reqwest::header::ACCEPT, "application/json")
        .header("X-YSUMC-Catalog", "split-v1");
    if let Some(etag) = cached.and_then(|s| s.etag.as_ref()) {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let response = request.send().await?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        let cached =
            cached.ok_or_else(|| invalid("304 without a cached catalog"))?;
        kind.parse(&cached.body)?;
        return Ok(Snapshot {
            body: cached.body.clone(),
            etag: cached.etag.clone(),
        });
    }
    if !response.status().is_success() {
        return Err(crate::ErrorKind::HttpError {
            status: response.status().as_u16(),
            method: "GET".into(),
            url: endpoint.into(),
        }
        .into());
    }
    if response
        .content_length()
        .is_some_and(|len| len > MAX_DOCUMENT_BYTES as u64)
    {
        return Err(invalid("YSUMC catalog exceeds 1 MiB"));
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len() + chunk.len() > MAX_DOCUMENT_BYTES {
            return Err(invalid("YSUMC catalog exceeds 1 MiB"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = String::from_utf8(bytes)
        .map_err(|_| invalid("Catalog is not UTF-8"))?;
    kind.parse(&body)?;
    Ok(Snapshot { etag, body })
}

// The own manifest publishes official mrpacks independently of public listing.
// Interop and friends supply server-only entries and other artifact formats.
fn merge_home_manifest(catalog: &mut ManagedManifest, home: &ManagedManifest) {
    catalog.launcher = home.launcher.clone();
    catalog.cloud_map_url = home.cloud_map_url.clone();
    let mut own: Vec<_> = home
        .instances
        .iter()
        .filter(|spec| spec.source.is_none() && !spec.id.contains(':'))
        .cloned()
        .collect();
    own.sort_by(|a, b| (a.sort, &a.name).cmp(&(b.sort, &b.name)));
    catalog.instances.retain(|spec| {
        spec.source.is_some()
            || (!own.iter().any(|local| local.id == spec.id)
                && (spec.pack.kind != "mrpack" || spec.pack.sha1.is_none()))
    });
    own.append(&mut catalog.instances);
    catalog.instances = own;
}

fn reconcile_server_revisions(
    catalog: &mut ManagedManifest,
    previous: Option<&ManagedManifest>,
) {
    for spec in &mut catalog.instances {
        if spec.pack.kind != "server" {
            continue;
        }
        if let Some(old) = previous
            .and_then(|old| old.instances.iter().find(|old| old.id == spec.id))
        {
            spec.revision = old.revision
                + i64::from(
                    old.pack.kind != "server"
                        || old.minecraft != spec.minecraft,
                );
        }
    }
}

fn audience_catalog(
    home_body: &str,
    public_body: &str,
) -> crate::Result<ManagedManifest> {
    let home: Value = serde_json::from_str(home_body)?;
    match home.get("catalog") {
        // An explicit empty or malformed projection must never fall back to
        // public entries cached before access was restricted.
        Some(document) => parse(&serde_json::to_string(document)?),
        None => parse(public_body), // Older plugin compatibility.
    }
}

async fn cached_snapshot(
    pool: &sqlx::SqlitePool,
    endpoint: &str,
    kind: CatalogKind,
) -> crate::Result<Option<Snapshot>> {
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT body FROM managed_manifest_cache WHERE manifest_url = ?",
    )
    .bind(endpoint)
    .fetch_optional(pool)
    .await?;
    Ok(raw
        .as_deref()
        .and_then(|s| serde_json::from_str::<Snapshot>(s).ok())
        .filter(|s| kind.parse(&s.body).is_ok()))
}

async fn request_with_retries(
    endpoint: &str,
    cached: Option<&Snapshot>,
    kind: CatalogKind,
) -> crate::Result<Snapshot> {
    let client = &crate::util::fetch::NO_REDIRECT_REQWEST_CLIENT;
    let mut result = request(client, endpoint, cached, kind).await;
    for delay in [250, 750, 1500] {
        if result.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(delay)).await;
        result = request(client, endpoint, cached, kind).await;
    }
    result
}

pub(super) async fn fetch_catalog() -> crate::Result<String> {
    let state = State::get().await?;
    let cached = cached_snapshot(
        &state.pool,
        crate::ysumc::INTEROP_URL,
        CatalogKind::Site,
    )
    .await?;
    let cached_friends = cached_snapshot(
        &state.pool,
        crate::ysumc::FRIENDS_URL,
        CatalogKind::Friends,
    )
    .await?;
    let previous: Option<String> = sqlx::query_scalar(
        "SELECT body FROM managed_manifest_cache WHERE manifest_url = ?",
    )
    .bind(crate::ysumc::MANIFEST_URL)
    .fetch_optional(&state.pool)
    .await?;
    let previous = previous
        .as_deref()
        .and_then(|s| parse_manifest(crate::ysumc::MANIFEST_URL, s).ok());
    // If either endpoint fails, the caller uses its complete offline snapshot.
    // Never retire friends merely because one of the split requests failed.
    let (snapshot, friends_snapshot) = tokio::try_join!(
        request_with_retries(
            crate::ysumc::INTEROP_URL,
            cached.as_ref(),
            CatalogKind::Site
        ),
        request_with_retries(
            crate::ysumc::FRIENDS_URL,
            cached_friends.as_ref(),
            CatalogKind::Friends
        ),
    )?;
    let home_body = fetch_legacy_body().await?;
    let mut catalog = audience_catalog(&home_body, &snapshot.body)?;
    merge_friends(&mut catalog, parse_friends(&friends_snapshot.body)?);
    let home = parse_manifest(crate::ysumc::MANIFEST_URL, &home_body)?;
    // A failed home fetch must use the outer offline snapshot path, never mark
    // a stale publication or revision as current and allow a new installation.
    merge_home_manifest(&mut catalog, &home);
    reconcile_server_revisions(&mut catalog, previous.as_ref());
    let body = serde_json::to_string(&catalog)?;
    let mut transaction = state.pool.begin().await?;
    store_manifest_body(
        &mut transaction,
        crate::ysumc::INTEROP_URL,
        &serde_json::to_string(&snapshot)?,
        Utc::now().timestamp(),
    )
    .await?;
    store_manifest_body(
        &mut transaction,
        crate::ysumc::FRIENDS_URL,
        &serde_json::to_string(&friends_snapshot)?,
        Utc::now().timestamp(),
    )
    .await?;
    transaction.commit().await?;
    Ok(body)
}

// A server-only entry becomes a minimal local instance using the existing
// transactional installer, without downloading a nonexistent artifact.
fn server_archive(spec: &ManagedInstanceSpec) -> crate::Result<Vec<u8>> {
    let mut dependencies = serde_json::Map::new();
    dependencies.insert(
        "minecraft".into(),
        Value::String(spec.minecraft.game_version.clone()),
    );
    if let Some(loader) =
        spec.minecraft.loader.as_deref().filter(|l| *l != "vanilla")
    {
        let key = match loader {
            "fabric" => "fabric-loader",
            "quilt" => "quilt-loader",
            other => other,
        };
        dependencies.insert(
            key.into(),
            Value::String(
                spec.minecraft
                    .loader_version
                    .clone()
                    .unwrap_or_else(|| "latest".into()),
            ),
        );
    }
    let index = serde_json::json!({"formatVersion":1, "game":"minecraft", "versionId":spec.revision.to_string(),
        "name":spec.name, "files":[], "dependencies":dependencies});
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(
        "modrinth.index.json",
        zip::write::SimpleFileOptions::default(),
    )
    .map_err(|e| invalid(e.to_string()))?;
    zip.write_all(&serde_json::to_vec(&index)?)?;
    Ok(zip
        .finish()
        .map_err(|e| invalid(e.to_string()))?
        .into_inner())
}

pub(super) async fn server_location(
    spec: &ManagedInstanceSpec,
) -> crate::Result<crate::api::pack::install_from::CreatePackLocation> {
    let bytes = server_archive(spec)?;
    let hash = sha1_smol::Sha1::from(&bytes).hexdigest();
    let state = State::get().await?;
    let directory = state.directories.caches_dir().join("ysumc-packs");
    tokio::fs::create_dir_all(&directory).await?;
    let path = directory.join(format!("{hash}.mrpack"));
    tokio::fs::write(&path, bytes).await?;
    Ok(crate::api::pack::install_from::CreatePackLocation::FromFile { path })
}

#[cfg(test)]
mod tests;
