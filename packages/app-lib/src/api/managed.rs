//! Server-managed game instances.
//!
//! The society backend publishes a manifest that declares which instances a player
//! may launch, at which `revision`, and from which pack file. This module keeps
//! the local record of those instances, downloads their packs, and gates
//! launching on the recorded revision. The manifest service is optional at
//! runtime: a failed sync keeps the previous snapshot and the previous records.
//!
//! See `docs/launcher-protocol.md` for the contract these types mirror.

use crate::State;
use crate::state::InstanceInstallStage;
use crate::util::fetch::INSECURE_REQWEST_CLIENT;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Highest manifest schema this build understands. A manifest that declares a
/// higher schema is rejected instead of being partially interpreted.
const SUPPORTED_MANIFEST_SCHEMA: u32 = 1;

/// Only pack format the existing pack install pipeline can consume.
const MRPACK_KIND: &str = "mrpack";

/// Longest managed instance id accepted, matching the manifest contract.
const MAX_INSTANCE_ID_LEN: usize = 64;

/// Port used when the manifest provides a `server` object without a port.
const DEFAULT_SERVER_PORT: u16 = 25565;

/// One manifest document as served by the society backend.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManagedManifest {
    #[serde(default)]
    pub cloud_map_url: Option<String>,
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub launcher: Option<ManagedLauncherPolicy>,
    #[serde(default)]
    pub instances: Vec<ManagedInstanceSpec>,
}

/// Launcher update policy carried by a manifest.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManagedLauncherPolicy {
    #[serde(default)]
    pub min_version: Option<String>,
    #[serde(default)]
    pub latest_version: Option<String>,
    #[serde(default)]
    pub force_update: bool,
}

/// One managed instance declared by a manifest.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManagedInstanceSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub sort: i32,
    pub revision: i64,
    pub minecraft: ManagedMinecraftSpec,
    pub pack: ManagedPackSpec,
    #[serde(default)]
    pub server: Option<ManagedServerSpec>,
    #[serde(default = "default_required")]
    pub required: bool,
}

/// Game and loader versions a managed instance must be installed with.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManagedMinecraftSpec {
    pub game_version: String,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub loader_version: Option<String>,
}

/// Where the content of a managed instance comes from.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManagedPackSpec {
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

/// Server a managed instance connects to on launch.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManagedServerSpec {
    pub address: String,
    #[serde(default)]
    pub opened_on: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

fn default_required() -> bool {
    true
}

/// Outcome of reconciling a manifest with the local records.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManagedSyncReport {
    #[serde(default)]
    pub cloud_map_url: Option<String>,
    pub schema: u32,
    pub offline: bool,
    pub synced_at: i64,
    #[serde(default)]
    pub launcher: Option<ManagedLauncherPolicy>,
    pub instances: Vec<ManagedInstanceAction>,
}

/// What the launcher should do with one managed instance.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManagedInstanceAction {
    pub spec: ManagedInstanceSpec,
    #[serde(default)]
    pub instance_id: Option<String>,
    #[serde(default)]
    pub applied_revision: Option<i64>,
    pub action: ManagedInstanceActionKind,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ManagedInstanceActionKind {
    Create,
    Update,
    Current,
    Retired,
}

/// Local record of one managed instance.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ManagedInstanceRecord {
    pub server_instance_id: String,
    pub instance_id: String,
    pub revision: i64,
    pub target_revision: i64,
    pub name: String,
    pub description: String,
    pub icon_url: String,
    pub sort: i32,
    pub required: bool,
    pub retired: bool,
    pub server_address: String,
    pub server_port: u16,
    pub updated: i64,
}

#[derive(sqlx::FromRow)]
struct ManagedInstanceRow {
    server_instance_id: String,
    instance_id: String,
    revision: i64,
    target_revision: i64,
    name: String,
    description: String,
    icon_url: String,
    sort: i64,
    required: i64,
    retired: i64,
    server_address: String,
    server_port: i64,
    updated: i64,
}

#[derive(sqlx::FromRow)]
struct ManagedGateRow {
    server_instance_id: String,
    revision: i64,
    target_revision: i64,
    retired: i64,
}

impl From<ManagedInstanceRow> for ManagedInstanceRecord {
    fn from(row: ManagedInstanceRow) -> Self {
        Self {
            server_instance_id: row.server_instance_id,
            instance_id: row.instance_id,
            revision: row.revision,
            target_revision: row.target_revision,
            name: row.name,
            description: row.description,
            icon_url: row.icon_url,
            sort: clamp_i32(row.sort),
            required: row.required != 0,
            retired: row.retired != 0,
            server_address: row.server_address,
            server_port: clamp_u16(row.server_port),
            updated: row.updated,
        }
    }
}

/// Fetches and parses the manifest.
///
/// A manifest that declares a schema newer than the one this build implements
/// is rejected, so a stale launcher cannot silently install the wrong content.
#[tracing::instrument]
pub async fn fetch_manifest(
    manifest_url: &str,
) -> crate::Result<ManagedManifest> {
    let body = fetch_manifest_body(manifest_url).await?;
    parse_manifest(manifest_url, &body)
}

/// Fetches the manifest, reconciles it with the local managed instance
/// records, and reports what should happen to every managed instance.
///
/// This does not download packs or create instances: the caller uses the
/// reported `instance_id` with the existing instance and pack install commands.
#[tracing::instrument]
pub async fn sync_manifest(
    manifest_url: &str,
) -> crate::Result<ManagedSyncReport> {
    // Serialize catalog reconciliation so older requests cannot retire newer rows.
    static SYNC_LOCK: tokio::sync::Mutex<()> =
        tokio::sync::Mutex::const_new(());
    let _sync = SYNC_LOCK.lock().await;
    validate_manifest_endpoint(manifest_url)?;
    let state = State::get().await?;
    let known = list_rows(&state).await?;
    let remote = fetch_manifest_body(manifest_url).await.and_then(|body| {
        let parsed = parse_manifest(manifest_url, &body)?;
        for spec in &parsed.instances {
            if let Some(row) =
                known.iter().find(|row| row.server_instance_id == spec.id)
            {
                validate_revision_progress(
                    spec.revision,
                    row.revision.max(row.target_revision),
                )?;
            }
        }
        Ok((body, parsed))
    });
    let (body, parsed, synced_at, offline) = match remote {
        Ok((body, parsed)) => (body, parsed, Utc::now().timestamp(), false),
        Err(error) => {
            let cached: Option<(String, i64)> = sqlx::query_as(
                "SELECT body, updated FROM managed_manifest_cache WHERE manifest_url = ?",
            )
            .bind(manifest_url)
            .fetch_optional(&state.pool)
            .await?;
            let Some((body, updated)) = cached else {
                return Err(error);
            };
            let parsed = parse_manifest(manifest_url, &body)?;
            tracing::warn!(%error, "Using the last valid official catalog");
            (body, parsed, updated, true)
        }
    };

    let ManagedManifest {
        schema,
        cloud_map_url,
        launcher,
        instances,
        ..
    } = parsed;

    let updated = Utc::now().timestamp();
    let mut actions = Vec::with_capacity(instances.len());

    for spec in &instances {
        if spec.pack.kind != MRPACK_KIND {
            tracing::warn!(
                server_instance_id = %spec.id,
                pack_kind = %spec.pack.kind,
                "Managed instance uses a pack format this launcher cannot install"
            );
        }

        let existing =
            known.iter().find(|row| row.server_instance_id == spec.id);
        let applied = existing.as_ref().map(|row| row.revision);
        let instance_id = existing
            .as_ref()
            .map(|row| row.instance_id.clone())
            .filter(|value| !value.is_empty());
        let target = match applied {
            Some(applied) if applied > spec.revision => {
                tracing::warn!(
                    server_instance_id = %spec.id,
                    applied,
                    requested = spec.revision,
                    "Refusing a managed instance revision downgrade"
                );
                applied
            }
            _ => spec.revision,
        }
        .max(existing.as_ref().map_or(0, |row| row.target_revision));

        // A record alone is not proof that the instance is usable: the local
        // instance may have been deleted, or an earlier install may never have
        // finished. Both have to end in an install, and only the local instance
        // can tell the difference, so it is consulted here rather than trusted
        // from the record.
        let linked_stage = match instance_id.as_deref() {
            Some(local_id) => crate::state::get_instance(local_id, &state.pool)
                .await?
                .map(|metadata| metadata.instance.install_stage),
            None => None,
        };

        let action = match (instance_id.as_deref(), applied) {
            _ if existing.as_ref().is_some_and(|row| row.retired != 0)
                && offline =>
            {
                ManagedInstanceActionKind::Retired
            }
            (None, _) => ManagedInstanceActionKind::Create,
            (Some(_), Some(_)) if linked_stage.is_none() => {
                ManagedInstanceActionKind::Create
            }
            (Some(_), Some(applied))
                if applied == target
                    && linked_stage
                        == Some(InstanceInstallStage::Installed) =>
            {
                ManagedInstanceActionKind::Current
            }
            (Some(_), _) => ManagedInstanceActionKind::Update,
        };
        actions.push(ManagedInstanceAction {
            spec: spec.clone(),
            instance_id,
            applied_revision: applied,
            action,
        });
    }

    let declared: HashSet<&str> =
        instances.iter().map(|spec| spec.id.as_str()).collect();
    // Commit the catalog and its snapshot together; no pool reads are held
    // behind this write transaction.
    let mut transaction = state.pool.begin().await?;
    if !offline {
        for spec in &instances {
            store_spec(&mut transaction, spec, spec.revision, updated).await?;
        }
    }
    for row in known {
        if offline || declared.contains(row.server_instance_id.as_str()) {
            continue;
        }

        // Retiring server content must never delete a player's worlds or files.
        sqlx::query("UPDATE managed_instances SET retired = TRUE WHERE server_instance_id = ?")
            .bind(&row.server_instance_id).execute(&mut *transaction).await?;
    }
    if !offline {
        store_manifest_body(&mut transaction, manifest_url, &body, synced_at)
            .await?;
    }
    transaction.commit().await?;

    actions.sort_by(|left, right| {
        left.spec
            .sort
            .cmp(&right.spec.sort)
            .then_with(|| left.spec.name.cmp(&right.spec.name))
    });

    Ok(ManagedSyncReport {
        schema,
        cloud_map_url,
        offline,
        synced_at,
        launcher,
        instances: actions,
    })
}

/// Local records of every known managed instance, ordered for display.
#[tracing::instrument]
pub async fn list_managed_instances()
-> crate::Result<Vec<ManagedInstanceRecord>> {
    let state = State::get().await?;
    Ok(list_rows(&state)
        .await?
        .into_iter()
        .map(ManagedInstanceRecord::from)
        .collect())
}

/// The native side selects the trusted pack. The complete spec is persisted in
/// the install job, including retries after a launcher restart.
pub async fn install_official(
    server_instance_id: &str,
) -> crate::Result<crate::install::InstallJobSnapshot> {
    // Serialize the check and enqueue, so two clicks cannot create two instances.
    static INSTALL_LOCK: tokio::sync::Mutex<()> =
        tokio::sync::Mutex::const_new(());
    let _install_guard = INSTALL_LOCK.lock().await;
    let state = State::get().await?;
    for job in crate::install::store::list(false, &state).await? {
        if job
            .state
            .request
            .official_spec()
            .is_some_and(|spec| spec.id == server_instance_id)
        {
            return Err(crate::ErrorKind::InputError(
                "此官方包已有安装任务，请在下载页面继续或取消该任务。".into(),
            )
            .into());
        }
    }
    let report = sync_manifest(crate::ysumc::MANIFEST_URL).await?;
    if report.offline {
        return Err(crate::ErrorKind::InputError(
            "请联网后安装或更新官方整合包".into(),
        )
        .into());
    }
    let action = report
        .instances
        .into_iter()
        .find(|item| item.spec.id == server_instance_id)
        .ok_or_else(|| {
            crate::ErrorKind::InputError("未找到官方整合包".into())
        })?;
    if !matches!(
        action.action,
        ManagedInstanceActionKind::Create | ManagedInstanceActionKind::Update
    ) {
        return Err(crate::ErrorKind::InputError(
            "此官方整合包当前不可安装或更新".into(),
        )
        .into());
    }
    let spec = action.spec;
    let location =
        crate::api::pack::install_from::CreatePackLocation::FromUrl {
            url: spec.pack.url.clone(),
            sha1: spec.pack.sha1.clone().unwrap(),
            size: spec.pack.size.unwrap(),
            display: crate::api::pack::install_from::RemotePackDisplay {
                name: spec.name.clone(),
                game_version: spec.minecraft.game_version.clone(),
                loader: spec.minecraft.loader.clone(),
                loader_version: spec.minecraft.loader_version.clone(),
                icon_url: spec.icon_url.clone(),
            },
        };
    let edit = Some(crate::install::InstallPostInstallEdit {
        name: Some(spec.name.clone()),
        official: Some(spec),
        ..Default::default()
    });
    if let Some(instance_id) = action
        .instance_id
        .filter(|_| action.action == ManagedInstanceActionKind::Update)
    {
        crate::install::install_pack_to_existing_instance(
            instance_id,
            location,
            edit,
        )
        .await
    } else {
        crate::install::create_modpack_instance(location, edit).await
    }
}

pub async fn ensure_content_editable(instance_id: &str) -> crate::Result<()> {
    let state = State::get().await?;
    let managed: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM managed_instances WHERE instance_id = ?",
    )
    .bind(instance_id)
    .fetch_one(&state.pool)
    .await?;
    if managed.0 > 0 {
        return Err(crate::ErrorKind::InputError(
            "官方整合包的模组由服务器统一管理，不能添加、修改或禁用。".into(),
        )
        .into());
    }
    Ok(())
}

pub(crate) async fn verify_installed_mods(
    instance_id: &str,
) -> crate::Result<()> {
    let state = State::get().await?;
    let installed: Option<(String,)> = sqlx::query_as(
        "SELECT spec_json FROM managed_installations WHERE instance_id = ?",
    )
    .bind(instance_id)
    .fetch_optional(&state.pool)
    .await?;
    let spec = if let Some((body,)) = installed {
        serde_json::from_str::<ManagedInstanceSpec>(&body)?
    } else {
        let managed: Option<(String, i64)> = sqlx::query_as("SELECT server_instance_id, revision FROM managed_instances WHERE instance_id = ?")
            .bind(instance_id).fetch_optional(&state.pool).await?;
        let Some((server_id, revision)) = managed else {
            return Ok(());
        };
        let cached: Option<(String,)> = sqlx::query_as(
            "SELECT body FROM managed_manifest_cache WHERE manifest_url = ?",
        )
        .bind(crate::ysumc::MANIFEST_URL)
        .fetch_optional(&state.pool)
        .await?;
        let cached = cached.ok_or_else(|| {
            crate::ErrorKind::InputError("请先刷新官方整合包清单".into())
        })?;
        parse_manifest(crate::ysumc::MANIFEST_URL, &cached.0)?
            .instances
            .into_iter()
            .find(|spec| spec.id == server_id && spec.revision == revision)
            .ok_or_else(|| {
                crate::ErrorKind::InputError(
                    "请先更新官方整合包，再启动游戏".into(),
                )
            })?
    };
    validate_managed_spec(&spec)?;
    crate::api::managed_mods::verify_and_repair(instance_id, &spec).await?;
    sqlx::query("INSERT INTO managed_installations(instance_id, spec_json) VALUES (?, ?) ON CONFLICT(instance_id) DO UPDATE SET spec_json = excluded.spec_json")
        .bind(instance_id).bind(serde_json::to_string(&spec)?).execute(&state.pool).await?;
    Ok(())
}

/// The society instance that owns a local instance, and whether the server requires
/// it.
///
/// The flag identifies server-recommended packs. Both kinds are installed only
/// when the player requests them.
pub struct ManagedOwner {
    pub server_instance_id: String,
    pub required: bool,
}

/// Looks up the society instance owning a local instance, if any.
#[tracing::instrument]
pub async fn managed_owner(
    instance_id: &str,
) -> crate::Result<Option<ManagedOwner>> {
    let state = State::get().await?;

    let row = sqlx::query_as::<_, (String, i64)>(
        "SELECT server_instance_id, required FROM managed_instances
         WHERE instance_id = ? LIMIT 1",
    )
    .bind(instance_id)
    .fetch_optional(&state.pool)
    .await?;

    Ok(row.map(|(server_instance_id, required)| ManagedOwner {
        server_instance_id,
        required: required != 0,
    }))
}

/// Records that the local instance now holds the given applied revision.
///
/// The required `target_revision` is owned by [`sync_manifest`], so a manifest
/// that moved on while the install was running still forces another update.
#[tracing::instrument]
pub async fn mark_instance_installed(
    server_instance_id: &str,
    instance_id: &str,
    revision: i64,
) -> crate::Result<()> {
    let state = State::get().await?;
    let result = sqlx::query(
        "UPDATE managed_instances
         SET instance_id = ?, revision = ?, updated = ?
         WHERE server_instance_id = ?",
    )
    .bind(instance_id)
    .bind(revision)
    .bind(Utc::now().timestamp())
    .bind(server_instance_id)
    .execute(&state.pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(crate::ErrorKind::InputError(format!(
            "Unknown managed instance {server_instance_id}; sync the instance manifest first"
        ))
        .as_error());
    }

    Ok(())
}

/// Records which local instance represents a managed instance.
///
/// The caller invokes this after the pack install job for `revision` has
/// settled successfully, so the applied revision is stored together with the
/// instance id. `target_revision` is left to [`sync_manifest`]: a manifest that
/// moved on while the install was running still forces another update.
#[tracing::instrument]
pub async fn register_instance(
    server_instance_id: &str,
    instance_id: &str,
    revision: i64,
) -> crate::Result<()> {
    let state = State::get().await?;
    sqlx::query(
        "INSERT INTO managed_instances (
            server_instance_id, instance_id, revision, target_revision, name,
            updated
         )
         VALUES (?, ?, ?, ?, '', ?)
         ON CONFLICT(server_instance_id) DO UPDATE SET
           instance_id = excluded.instance_id,
           revision = excluded.revision,
           updated = excluded.updated",
    )
    .bind(server_instance_id)
    .bind(instance_id)
    .bind(revision)
    .bind(revision)
    .bind(Utc::now().timestamp())
    .execute(&state.pool)
    .await?;

    Ok(())
}

/// Launch gate: rejects a managed instance that is retired, not installed, or
/// not installed at the revision the manifest currently requires.
///
/// Only local state is read, so the gate never blocks a launch on the network.
/// Instances that are not managed pass through unchanged.
#[tracing::instrument]
pub async fn ensure_instance_runnable(instance_id: &str) -> crate::Result<()> {
    let state = State::get().await?;
    let rows = sqlx::query_as::<_, ManagedGateRow>(
        "SELECT server_instance_id, revision, target_revision, retired
         FROM managed_instances
         WHERE instance_id = ?",
    )
    .bind(instance_id)
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    let cached: Option<(String,)> = sqlx::query_as(
        "SELECT body FROM managed_manifest_cache WHERE manifest_url = ?",
    )
    .bind(crate::ysumc::MANIFEST_URL)
    .fetch_optional(&state.pool)
    .await?;
    if let Some((body,)) = cached {
        if let Some(policy) =
            serde_json::from_str::<ManagedManifest>(&body)?.launcher
        {
            validate_launcher_policy(&policy, env!("CARGO_PKG_VERSION"))?;
        }
    }
    for row in &rows {
        if row.retired != 0 {
            return Err(crate::ErrorKind::LauncherError(format!(
                "Managed instance {} is no longer offered by the server and cannot be launched",
                row.server_instance_id
            ))
            .as_error());
        }
        if row.revision != row.target_revision {
            return Err(crate::ErrorKind::LauncherError(format!(
                "Managed instance {} must be updated from revision {} to revision {} before it can be launched",
                row.server_instance_id, row.revision, row.target_revision
            ))
            .as_error());
        }
    }

    let stage = crate::state::get_instance(instance_id, &state.pool)
        .await?
        .map(|metadata| metadata.instance.install_stage);
    if stage != Some(InstanceInstallStage::Installed) {
        return Err(crate::ErrorKind::InstanceNotReady {
            instance_id: instance_id.to_string(),
            stage: stage
                .map(|stage| stage.as_str().to_string())
                .unwrap_or_else(|| "not_installed".to_string()),
        }
        .as_error());
    }

    Ok(())
}

/// Rejects an identifier that could escape the managed cache directory or that
/// no server should ever emit.
fn validate_server_instance_id(value: &str) -> crate::Result<()> {
    let safe = !value.is_empty()
        && value.len() <= MAX_INSTANCE_ID_LEN
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
        });
    if !safe {
        return Err(crate::ErrorKind::InputError(format!(
            "Managed instance id \"{value}\" is not a usable identifier"
        ))
        .as_error());
    }
    Ok(())
}

fn validate_revision_progress(incoming: i64, known: i64) -> crate::Result<()> {
    if incoming < known {
        return Err(crate::ErrorKind::InputError(
            "The official catalog contains an older revision; keeping the last valid snapshot".into()
        ).as_error());
    }
    Ok(())
}

fn validate_managed_spec(spec: &ManagedInstanceSpec) -> crate::Result<()> {
    validate_server_instance_id(&spec.id)?;
    if spec.revision < 1 {
        return Err(crate::ErrorKind::InputError(format!(
            "Managed instance {} must declare a positive revision",
            spec.id
        ))
        .as_error());
    }
    if spec.name.trim().is_empty()
        || spec.minecraft.game_version.trim().is_empty()
    {
        return Err(crate::ErrorKind::InputError(format!(
            "Managed instance {} does not declare a Minecraft game version",
            spec.id
        ))
        .as_error());
    }
    let digest = spec.pack.sha1.as_deref().unwrap_or_default();
    let secure_url = url::Url::parse(&spec.pack.url).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
    });
    if digest.len() != 40
        || !digest.bytes().all(|c| c.is_ascii_hexdigit())
        || spec.pack.size.unwrap_or(0) == 0
        || !secure_url
    {
        return Err(crate::ErrorKind::InputError(
            "Official packs require HTTPS, a SHA-1 hash and a positive size"
                .into(),
        )
        .as_error());
    }
    if spec.pack.kind != MRPACK_KIND
        || spec.server.as_ref().is_some_and(|server| {
            server.address.trim().is_empty() || server.port == Some(0)
        })
    {
        return Err(crate::ErrorKind::InputError(format!(
            "Managed instance {} has an unsupported pack format or invalid server address",
            spec.id
        ))
        .as_error());
    }
    Ok(())
}

fn validate_manifest_schema(schema: u32) -> crate::Result<()> {
    if schema > SUPPORTED_MANIFEST_SCHEMA {
        return Err(crate::ErrorKind::InputError(format!(
            "The instance manifest requires schema {schema}, but this launcher only supports schema {SUPPORTED_MANIFEST_SCHEMA}; update the launcher"
        ))
        .as_error());
    }
    if schema == 0 {
        return Err(crate::ErrorKind::InputError(
            "The instance manifest does not declare a schema version"
                .to_string(),
        )
        .as_error());
    }
    Ok(())
}

async fn fetch_manifest_body(manifest_url: &str) -> crate::Result<String> {
    validate_manifest_endpoint(manifest_url)?;
    let response = INSECURE_REQWEST_CLIENT
        .get(manifest_url)
        .timeout(std::time::Duration::from_secs(20))
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(crate::ErrorKind::HttpError {
            status: status.as_u16(),
            method: "GET".to_string(),
            url: manifest_url.to_string(),
        }
        .as_error());
    }
    Ok(response.text().await?)
}

fn validate_manifest_endpoint(manifest_url: &str) -> crate::Result<()> {
    if manifest_url != crate::ysumc::MANIFEST_URL {
        return Err(crate::ErrorKind::InputError(
            "Unexpected YSUMC manifest endpoint".into(),
        )
        .as_error());
    }
    Ok(())
}

fn parse_manifest(
    manifest_url: &str,
    body: &str,
) -> crate::Result<ManagedManifest> {
    let manifest: ManagedManifest =
        serde_json::from_str(body).map_err(|error| {
            crate::ErrorKind::InputError(format!(
                "Manifest from {manifest_url} is not valid JSON: {error}"
            ))
            .as_error()
        })?;
    validate_manifest_schema(manifest.schema)?;
    let mut ids = HashSet::new();
    for spec in &manifest.instances {
        validate_managed_spec(spec)?;
        if !ids.insert(&spec.id) {
            return Err(crate::ErrorKind::InputError(
                "Duplicate official instance id".into(),
            )
            .as_error());
        }
    }
    if let Some(policy) = &manifest.launcher {
        for version in [&policy.min_version, &policy.latest_version]
            .into_iter()
            .flatten()
        {
            semver::Version::parse(version).map_err(|error| {
                crate::ErrorKind::InputError(format!(
                    "Invalid launcher version policy: {error}"
                ))
            })?;
        }
    }
    Ok(manifest)
}

async fn list_rows(state: &State) -> crate::Result<Vec<ManagedInstanceRow>> {
    Ok(sqlx::query_as::<_, ManagedInstanceRow>(
        "SELECT server_instance_id, instance_id, revision, target_revision,
                name, description, icon_url, sort, required, retired,
                server_address, server_port, updated
         FROM managed_instances
         ORDER BY sort ASC, name ASC",
    )
    .fetch_all(&state.pool)
    .await?)
}

async fn store_spec(
    connection: &mut sqlx::SqliteConnection,
    spec: &ManagedInstanceSpec,
    target_revision: i64,
    updated: i64,
) -> crate::Result<()> {
    let description = spec.description.as_deref().unwrap_or("");
    let icon_url = spec.icon_url.as_deref().unwrap_or("");
    let server_address = spec
        .server
        .as_ref()
        .map(|server| server.address.as_str())
        .unwrap_or("");
    let server_port = spec
        .server
        .as_ref()
        .map(|server| server.port.unwrap_or(DEFAULT_SERVER_PORT))
        .unwrap_or(0);

    sqlx::query(
        "INSERT OR IGNORE INTO managed_instances (
            server_instance_id, instance_id, revision, target_revision, name,
            description, icon_url, sort, required, retired, server_address,
            server_port, updated
         )
         VALUES (?, '', 0, ?, ?, ?, ?, ?, ?, FALSE, ?, ?, ?)",
    )
    .bind(&spec.id)
    .bind(target_revision)
    .bind(&spec.name)
    .bind(description)
    .bind(icon_url)
    .bind(i64::from(spec.sort))
    .bind(i64::from(spec.required))
    .bind(server_address)
    .bind(i64::from(server_port))
    .bind(updated)
    .execute(&mut *connection)
    .await?;

    sqlx::query(
        "UPDATE managed_instances
         SET target_revision = ?, name = ?, description = ?, icon_url = ?,
             sort = ?, required = ?, retired = FALSE, server_address = ?,
             server_port = ?, updated = ?
         WHERE server_instance_id = ?",
    )
    .bind(target_revision)
    .bind(&spec.name)
    .bind(description)
    .bind(icon_url)
    .bind(i64::from(spec.sort))
    .bind(i64::from(spec.required))
    .bind(server_address)
    .bind(i64::from(server_port))
    .bind(updated)
    .bind(&spec.id)
    .execute(&mut *connection)
    .await?;

    Ok(())
}

async fn store_manifest_body(
    connection: &mut sqlx::SqliteConnection,
    manifest_url: &str,
    body: &str,
    updated: i64,
) -> crate::Result<()> {
    sqlx::query(
        "INSERT INTO managed_manifest_cache (manifest_url, body, updated)
         VALUES (?, ?, ?)
         ON CONFLICT(manifest_url) DO UPDATE SET
           body = excluded.body,
           updated = excluded.updated",
    )
    .bind(manifest_url)
    .bind(body)
    .bind(updated)
    .execute(connection)
    .await?;
    Ok(())
}

fn clamp_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn clamp_u16(value: i64) -> u16 {
    value.clamp(0, i64::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> serde_json::Value {
        serde_json::json!({
            "schema": 1,
            "instances": [{
                "id": "survival", "name": "YSUMC Survival", "revision": 3,
                "minecraft": { "game_version": "1.21.1" },
                "pack": { "kind": "mrpack", "url": "https://skin.ysumc.net/packs/survival.mrpack",
                    "sha1": "a".repeat(40), "size": 100 }
            }]
        })
    }

    #[test]
    fn refuses_corrupt_or_unsupported_catalogs_before_reconciliation() {
        let valid = catalog();
        assert!(
            parse_manifest(crate::ysumc::MANIFEST_URL, &valid.to_string())
                .is_ok()
        );
        for (field, value) in [
            ("url", serde_json::json!("https://")),
            (
                "url",
                serde_json::json!("https://user:secret@skin.ysumc.net/pack"),
            ),
            ("kind", serde_json::json!("zip")),
            ("sha1", serde_json::json!("invalid")),
            ("size", serde_json::json!(0)),
        ] {
            let mut corrupt = valid.clone();
            corrupt["instances"][0]["pack"][field] = value;
            assert!(
                parse_manifest(
                    crate::ysumc::MANIFEST_URL,
                    &corrupt.to_string()
                )
                .is_err()
            );
        }
        let mut duplicate = valid.clone();
        duplicate["instances"]
            .as_array_mut()
            .unwrap()
            .push(valid["instances"][0].clone());
        assert!(
            parse_manifest(crate::ysumc::MANIFEST_URL, &duplicate.to_string())
                .is_err()
        );
        assert!(validate_revision_progress(2, 3).is_err());
        assert!(validate_revision_progress(3, 3).is_ok());
        assert!(validate_revision_progress(4, 3).is_ok());
    }

    #[tokio::test]
    async fn catalog_and_snapshot_roll_back_together_without_losing_installed_identity()
     {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql("CREATE TABLE settings(theme TEXT); INSERT INTO settings VALUES ('dark');")
            .execute(&pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/20261020120000_ysumc-managed-instances.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        let body = catalog().to_string();
        let manifest =
            parse_manifest(crate::ysumc::MANIFEST_URL, &body).unwrap();
        let mut transaction = pool.begin().await.unwrap();
        store_spec(&mut transaction, &manifest.instances[0], 3, 10)
            .await
            .unwrap();
        store_manifest_body(
            &mut transaction,
            crate::ysumc::MANIFEST_URL,
            &body,
            10,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        sqlx::query("UPDATE managed_instances SET instance_id = 'local-game', revision = 3")
            .execute(&pool).await.unwrap();

        let mut changed = manifest.instances[0].clone();
        changed.revision = 4;
        let mut transaction = pool.begin().await.unwrap();
        store_spec(&mut transaction, &changed, 4, 20).await.unwrap();
        store_manifest_body(
            &mut transaction,
            crate::ysumc::MANIFEST_URL,
            "changed",
            20,
        )
        .await
        .unwrap();
        transaction.rollback().await.unwrap();
        let record: (String, i64, i64) = sqlx::query_as(
            "SELECT instance_id, revision, target_revision FROM managed_instances"
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(record, ("local-game".into(), 3, 3));
        let snapshot: (String, i64) =
            sqlx::query_as("SELECT body, updated FROM managed_manifest_cache")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(snapshot, (body, 10));
    }

    #[test]
    fn accepts_the_supported_manifest_schema() {
        assert!(validate_manifest_schema(SUPPORTED_MANIFEST_SCHEMA).is_ok());
    }

    #[test]
    fn rejects_a_missing_or_newer_manifest_schema() {
        assert!(
            validate_manifest_schema(SUPPORTED_MANIFEST_SCHEMA + 1).is_err()
        );
        assert!(validate_manifest_schema(0).is_err());
    }

    #[test]
    fn rejects_identifiers_that_could_escape_the_cache_directory() {
        assert!(validate_server_instance_id("survival-12_a").is_ok());
        assert!(validate_server_instance_id("").is_err());
        assert!(validate_server_instance_id("../escape").is_err());
        assert!(
            validate_server_instance_id(&"a".repeat(MAX_INSTANCE_ID_LEN + 1))
                .is_err()
        );
    }
}

fn validate_launcher_policy(
    policy: &ManagedLauncherPolicy,
    current: &str,
) -> crate::Result<()> {
    if !policy.force_update {
        return Ok(());
    }
    if let Some(minimum) = &policy.min_version {
        let invalid = |e: semver::Error| {
            crate::ErrorKind::InputError(format!(
                "Invalid launcher version policy: {e}"
            ))
        };
        if semver::Version::parse(current).map_err(invalid)?
            < semver::Version::parse(minimum).map_err(invalid)?
        {
            return Err(crate::ErrorKind::InputError(format!("This official instance requires YSUMC Launcher {minimum} or newer")).as_error());
        }
    }
    Ok(())
}
