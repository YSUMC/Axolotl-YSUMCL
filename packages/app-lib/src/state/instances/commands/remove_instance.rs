use crate::state::State;
use crate::state::instances::adapters::sqlite::instance_rows;
use crate::state::instances::config_sync;
use crate::util::io;
use std::path::{Path, PathBuf};

pub(crate) async fn remove_instance(
    instance_id: &str,
    state: &State,
) -> crate::Result<()> {
    remove_instance_with_policy(instance_id, state, false).await
}

pub(crate) async fn remove_instance_preserving_external_files(
    instance_id: &str,
    state: &State,
) -> crate::Result<()> {
    remove_instance_with_policy(instance_id, state, true).await
}

async fn remove_instance_with_policy(
    instance_id: &str,
    state: &State,
    preserve_external_files: bool,
) -> crate::Result<()> {
    let _maintenance_guard =
        crate::api::instance::lock_instance_maintenance(instance_id).await;
    crate::api::instance::begin_instance_deletion(instance_id).await?;
    let result = remove_instance_files_and_state(
        instance_id,
        state,
        preserve_external_files,
    )
    .await;
    if let Err(error) = result {
        if let Err(cancel_error) =
            crate::api::instance::cancel_instance_deletion(instance_id).await
        {
            tracing::warn!(
                instance_id,
                %cancel_error,
                "Failed to clear pending backup deletion marker"
            );
        }
        return Err(error);
    }
    crate::api::instance::delete_instance_backups(instance_id).await?;
    if !preserve_external_files {
        if let Err(error) =
            crate::storage::cleanup_uninstalled_packs(&state.directories).await
        {
            // Another active install can temporarily own the shared cache.
            // The storage cleanup action will retry it once that work finishes.
            tracing::warn!(%error, "Deferred cleanup of uninstalled pack archives");
        }
    }
    Ok(())
}

async fn remove_instance_files_and_state(
    instance_id: &str,
    state: &State,
    preserve_external_files: bool,
) -> crate::Result<()> {
    let _synced_options_lock = state.lock_synced_options().await;
    let _instance_lock = state.lock_instance_content(instance_id).await;
    let _config_guard = config_sync::lock_config_files().await;

    let Some(instance) =
        instance_rows::get_instance_by_id(instance_id, &state.pool).await?
    else {
        // The external-instance watcher can remove a record while a stale
        // library view still has its delete dialog open. Deletion is
        // idempotent, so finish cleanup successfully instead of surfacing a
        // spurious "Unknown instance" error.
        tracing::debug!(instance_id, "Instance was already removed");
        return Ok(());
    };

    // Directly associated instances have no Axolotl profile directory. Their
    // version directory is the instance itself, so removal deliberately
    // deletes the externally managed content in place. Keep the shared
    // `.minecraft` root (assets/libraries/other versions) intact.
    let managed_path = io::safe_relative_join(
        &state.directories.instances_dir(),
        &instance.path,
    )?;
    let path = if preserve_external_files || instance.symlink_target.is_some() {
        managed_path.clone()
    } else if instance.is_direct_linked() {
        crate::launcher::DirectLinkedLaunch::from_instance(&instance)?
            .ok_or_else(|| {
                crate::ErrorKind::LauncherError(
                    "Direct instance link metadata is incomplete".to_string(),
                )
            })?
            .version_dir()
    } else if let Some(game_dir_override) = instance
        .game_dir_override
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| is_version_isolated_game_dir(path))
    {
        // New instances created against a configured `.minecraft` root use
        // a private `versions/<name>` directory. Remove that external
        // directory when the instance is deleted, while preserving shared
        // (non-isolated) overrides for backwards compatibility.
        game_dir_override
    } else {
        managed_path.clone()
    };
    let watched_path = crate::launcher::linked_game_dir(&instance)
        .or_else(|| instance.linked_dot_minecraft.as_deref().map(PathBuf::from))
        .unwrap_or_else(|| state.directories.instance_game_dir(&instance));
    crate::state::instances::watcher::unwatch_instance_folder(
        &instance.path,
        &watched_path,
        &state.file_watcher,
    )
    .await;
    crate::api::instance::remove_generated_instance_files(instance_id, state)
        .await?;
    io::remove_dir_all(&path).await?;
    if path != managed_path {
        // Isolated external game directories also have a launcher-owned
        // profile containing metadata. Both belong to this uninstall.
        io::remove_dir_all(&managed_path).await?;
    }

    let jobs = crate::install::store::mark_instance_deleted(instance_id, state)
        .await?;
    instance_rows::delete_instance_by_id(&instance.id, &state.pool).await?;
    sqlx::query("UPDATE managed_instances SET instance_id = '', revision = 0 WHERE instance_id = ?")
        .bind(instance_id).execute(&state.pool).await?;
    config_sync::remove_config_file(&state.directories, &instance.path).await?;
    for job in jobs {
        if let Err(error) =
            crate::install::events::emit_install_job(&job.snapshot()).await
        {
            tracing::warn!(
                "Failed to emit deleted instance download state: {error}"
            );
        }
    }

    Ok(())
}

fn is_version_isolated_game_dir(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some("versions")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::instances::{
        CreateInstance, InstanceLink, create_instance,
    };
    use crate::state::{DirectoryInfo, ModLoader};
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn uninstall_removes_owned_folders_and_records_but_preserves_shared_games()
     {
        for (isolated, preserve_external) in
            [(true, false), (true, true), (false, false)]
        {
            let temp = tempfile::tempdir().unwrap();
            let dirs = DirectoryInfo {
                settings_dir: temp.path().join("launcher"),
                config_dir: temp.path().join("launcher"),
                app_identifier: "test".into(),
            };
            std::fs::create_dir_all(dirs.instances_dir()).unwrap();
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            sqlx::migrate!().run(&pool).await.unwrap();
            let state =
                crate::state::test_state(dirs, pool.clone()).await.unwrap();
            let external_root = temp.path().join("minecraft");
            let game_dir = if isolated {
                external_root.join("versions/pack")
            } else {
                external_root.clone()
            };
            std::fs::create_dir_all(game_dir.join("saves/world")).unwrap();
            std::fs::write(game_dir.join("saves/world/level.dat"), b"save")
                .unwrap();
            std::fs::create_dir_all(external_root.join("libraries")).unwrap();
            std::fs::write(
                external_root.join("libraries/shared.jar"),
                b"shared",
            )
            .unwrap();
            let instance = create_instance(
                CreateInstance {
                    name: "Pack".into(),
                    path: None,
                    game_version: "1.21.4".into(),
                    loader: ModLoader::Vanilla,
                    loader_version: None,
                    icon_path: None,
                    link: InstanceLink::Unmanaged,
                    symlink_target: None,
                    game_dir_override: Some(
                        game_dir.to_string_lossy().into_owned(),
                    ),
                },
                &state,
            )
            .await
            .unwrap();
            config_sync::sync_instance(&state, &instance.id)
                .await
                .unwrap();
            sqlx::query("INSERT INTO managed_instances(server_instance_id, instance_id, revision, target_revision, name, updated) VALUES ('pack', ?, 1, 1, 'Pack', 0)")
                .bind(&instance.id).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO managed_installations(instance_id, spec_json) VALUES (?, '{}')").bind(&instance.id).execute(&pool).await.unwrap();
            remove_instance_files_and_state(
                &instance.id,
                &state,
                preserve_external,
            )
            .await
            .unwrap();
            config_sync::sync_instance(&state, &instance.id)
                .await
                .unwrap();
            assert!(
                !state
                    .directories
                    .instances_dir()
                    .join(&instance.path)
                    .exists()
            );
            assert_eq!(
                game_dir.join("saves/world/level.dat").exists(),
                preserve_external || !isolated
            );
            assert!(external_root.join("libraries/shared.jar").exists());
            let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM instances), (SELECT COUNT(*) FROM managed_installations), (SELECT COUNT(*) FROM managed_instances WHERE instance_id <> '')").fetch_one(&pool).await.unwrap();
            assert_eq!(counts, (0, 0, 0));
            // A repeated removal remains harmless.
            remove_instance_files_and_state(
                &instance.id,
                &state,
                preserve_external,
            )
            .await
            .unwrap();
            drop(state);
            pool.close().await;
        }
    }
}
