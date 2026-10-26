use super::*;

#[tokio::test]
async fn sha512_packs_are_retained_without_server_only_entries_pinning_all_caches()
 {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql("CREATE TABLE instances(id TEXT); CREATE TABLE managed_installations(instance_id TEXT, spec_json TEXT); INSERT INTO instances VALUES ('pack'), ('server');")
        .execute(&pool).await.unwrap();
    let hash = "a".repeat(128);
    for (id, spec) in [
        (
            "pack",
            serde_json::json!({"pack":{"kind":"mrpack", "sha512":hash, "sha1":null}}),
        ),
        (
            "server",
            serde_json::json!({"pack":{"kind":"server", "sha512":null, "sha1":null}}),
        ),
    ] {
        sqlx::query("INSERT INTO managed_installations VALUES (?, ?)")
            .bind(id)
            .bind(spec.to_string())
            .execute(&pool)
            .await
            .unwrap();
    }
    let retained = retained_managed_packs(&pool)
        .await
        .unwrap()
        .into_iter()
        .collect();
    assert!(!disposable(
        &format!("ysumc-packs/{hash}.mrpack"),
        &retained
    ));
    assert!(disposable(
        &format!("ysumc-packs/{}.mrpack", "b".repeat(128)),
        &retained
    ));
    assert!(disposable(
        &format!("ysumc-packs/{}.mrpack", "b".repeat(40)),
        &retained
    ));
}

#[tokio::test]
async fn uninstall_cache_keeps_other_channel_packs_and_removes_only_orphans() {
    let temp = tempfile::tempdir().unwrap();
    let directories = DirectoryInfo {
        settings_dir: temp.path().into(),
        config_dir: temp.path().into(),
        app_identifier: "test".into(),
    };
    let active = "a".repeat(40);
    let orphan = "b".repeat(40);
    for channel in ["release", "beta"] {
        std::fs::create_dir_all(temp.path().join(channel)).unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(temp.path().join(channel).join("app.db"))
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        sqlx::raw_sql("CREATE TABLE install_jobs(status TEXT, state TEXT); CREATE TABLE instances(id TEXT PRIMARY KEY); CREATE TABLE managed_instances(instance_id TEXT); CREATE TABLE managed_installations(instance_id TEXT, spec_json TEXT);").execute(&pool).await.unwrap();
        if channel == "beta" {
            sqlx::query("INSERT INTO instances VALUES ('other-pack')")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO managed_installations VALUES ('other-pack', ?)",
            )
            .bind(serde_json::json!({"pack":{"sha1":active}}).to_string())
            .execute(&pool)
            .await
            .unwrap();
        }
        pool.close().await;
    }
    let packs = temp.path().join("caches/ysumc-packs");
    std::fs::create_dir_all(&packs).unwrap();
    for name in [
        format!("{active}.mrpack"),
        format!("{orphan}.mrpack"),
        format!("{orphan}.mrpack.part"),
        "user-file.zip".into(),
    ] {
        std::fs::write(packs.join(name), b"pack").unwrap();
    }
    let retained = retained_content(&directories).await.unwrap();
    let root = cache_root(&directories).unwrap();
    let mut failures = Vec::new();
    walk(&root, "", Some(&retained), &mut failures).unwrap();
    assert!(failures.is_empty());
    assert!(packs.join(format!("{active}.mrpack")).exists());
    assert!(!packs.join(format!("{orphan}.mrpack")).exists());
    assert!(packs.join(format!("{orphan}.mrpack.part")).exists());
    assert!(packs.join("user-file.zip").exists());
}

#[tokio::test]
async fn uninstall_cache_preserves_legacy_installed_packs_until_spec_is_known()
{
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE install_jobs(status TEXT, state TEXT); CREATE TABLE instances(id TEXT); CREATE TABLE managed_instances(instance_id TEXT); INSERT INTO instances VALUES ('legacy'); INSERT INTO managed_instances VALUES ('legacy');").execute(&pool).await.unwrap();
    let archive = format!("ysumc-packs/{}.mrpack", "a".repeat(40));
    let retained = retained_in_channel(&pool)
        .await
        .unwrap()
        .into_iter()
        .collect();
    assert!(!disposable(&archive, &retained));
    sqlx::query("DELETE FROM instances")
        .execute(&pool)
        .await
        .unwrap();
    let retained = retained_in_channel(&pool)
        .await
        .unwrap()
        .into_iter()
        .collect();
    assert!(disposable(&archive, &retained));
    pool.close().await;
}

#[test]
fn cache_totals_include_previous_location_without_counting_shared_roots_twice()
{
    let temp = tempfile::tempdir().unwrap();
    let mut directories = DirectoryInfo {
        settings_dir: temp.path().join("settings"),
        config_dir: temp.path().join("data"),
        app_identifier: "test".into(),
    };
    for (root, size) in [
        (&directories.settings_dir, 11),
        (&directories.config_dir, 13),
    ] {
        std::fs::create_dir_all(root.join("caches")).unwrap();
        std::fs::write(root.join("caches/file"), vec![0; size]).unwrap();
    }
    assert_eq!(cache_bytes(&directories, &mut vec![]).unwrap(), 24);
    directories.config_dir = directories.settings_dir.clone();
    assert_eq!(cache_bytes(&directories, &mut vec![]).unwrap(), 11);
}

#[test]
fn download_cleanup_keeps_runtime_files_and_referenced_content() {
    let temp = tempfile::tempdir().unwrap();
    let digest = "a".repeat(128);
    let orphan = "b".repeat(128);
    let files = [
        "content/modrinth/release/mod.jar".to_string(),
        "curseforge/modpacks/pack.zip".to_string(),
        "modpacks/project/pack.mrpack".to_string(),
        format!("content-store/{orphan}"),
        format!("content-store/{digest}"),
        "icons/instance.png".to_string(),
        "minecraft-natives/native.jar".to_string(),
        "resource-staging/job/file.jar".to_string(),
        "unknown/user.txt".to_string(),
        "resources/beta/resource-cache.db".to_string(),
    ];
    for file in &files {
        let path = temp.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"cache").unwrap();
    }
    let root = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority())
        .unwrap();
    let mut failures = vec![];
    assert_eq!(walk(&root, "", None, &mut failures).unwrap(), 50);
    walk(&root, "", Some(&HashSet::from([digest])), &mut failures).unwrap();
    assert!(failures.is_empty());
    assert_eq!(walk(&root, "", None, &mut failures).unwrap(), 30);
    for (index, file) in files.iter().enumerate() {
        assert_eq!(temp.path().join(file).exists(), index >= 4, "{file}");
    }
}

#[tokio::test]
async fn channel_references_keep_installed_retained_and_incomplete_store_objects()
 {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE install_jobs(status TEXT, state TEXT); CREATE TABLE store_blobs(digest TEXT); CREATE TABLE store_instance_files(digest TEXT); CREATE TABLE store_retained_refs(digest TEXT); CREATE TABLE store_operations(digest TEXT, state TEXT); INSERT INTO store_instance_files VALUES ('INSTALLED'); INSERT INTO store_retained_refs VALUES ('HISTORY'); INSERT INTO store_operations VALUES ('PENDING','published'), ('DONE','materialized');").execute(&pool).await.unwrap();
    let retained: HashSet<_> = retained_in_channel(&pool)
        .await
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(
        retained,
        HashSet::from(["installed".into(), "history".into(), "pending".into()])
    );
    sqlx::query("INSERT INTO install_jobs VALUES ('waiting_for_user', '{}')")
        .execute(&pool)
        .await
        .unwrap();
    assert!(retained_in_channel(&pool).await.is_err());
    sqlx::query("UPDATE install_jobs SET status = 'failed', state = '{\"rollback\":{}}'").execute(&pool).await.unwrap();
    assert!(retained_in_channel(&pool).await.is_err());
    pool.close().await;
}

#[tokio::test]
async fn old_channels_without_a_content_store_are_supported_but_partial_schemas_are_refused()
 {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("CREATE TABLE install_jobs(status TEXT, state TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    assert!(retained_in_channel(&pool).await.unwrap().is_empty());
    sqlx::query("CREATE TABLE store_blobs(digest TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    assert!(retained_in_channel(&pool).await.is_err());
    pool.close().await;
}

#[cfg(unix)]
#[test]
fn cleanup_does_not_follow_links_outside_cache() {
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("keep.jar"), b"keep").unwrap();
    std::os::unix::fs::symlink(outside.path(), temp.path().join("content"))
        .unwrap();
    let root = Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority())
        .unwrap();
    walk(&root, "", Some(&HashSet::new()), &mut vec![]).unwrap();
    assert!(outside.path().join("keep.jar").exists());
}
