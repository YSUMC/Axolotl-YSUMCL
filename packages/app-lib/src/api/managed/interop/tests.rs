use super::*;
use serde_json::json;

fn friends_document() -> Value {
    json!({
        "schema":1, "generated_at":"2026-10-23T08:00:00Z", "sites":[{
            "site_id":"sitmc", "name":"SIT", "domain":"skin.sitmc.club",
            "description":"SIT friends", "stale":true,
            "manifest_url":"https://skin.sitmc.club/.well-known/mc-interop-v1.json",
            "instances":[{
                "id":"sitmc:survival", "name":"SIT Survival", "revision":12,
                "minecraft":{"game_version":"26.2", "loader":"fabric", "loader_version":"0.19.5"},
                "pack":{"kind":"mrpack", "url":"https://skin.sitmc.club/pack.mrpack", "size":123, "sha512":"a".repeat(128)},
                "server":{"address":"play.sitmc.club"}
            }, {
                "id":"sitmc:vanilla", "name":"Vanilla server", "revision":null,
                "minecraft":{"game_version":"26.2", "loader":null},
                "pack":null, "server":{"address":"play.sitmc.club", "port":25565}
            }]
        }]
    })
}

#[test]
fn split_friends_preserve_identity_hashes_server_only_and_stale_state() {
    let friends = parse_friends(&friends_document().to_string()).unwrap();
    assert_eq!(friends.instances.len(), 2);
    let peer = &friends.instances[0];
    assert_eq!(peer.id, "sitmc:survival");
    assert_eq!(peer.pack.sha1, None);
    assert_eq!(peer.pack.sha512.as_deref(), Some("a".repeat(128).as_str()));
    assert_eq!(peer.source.as_ref().unwrap().name, "SIT");
    assert!(peer.source.as_ref().unwrap().stale);
    assert_eq!(peer.server.as_ref().unwrap().port, None);
    assert!(!peer.required);
    assert_eq!(friends.instances[1].pack.kind, "server");
    assert_eq!(friends.instances[1].revision, 1);
    assert_eq!(
        friends.instances[1].minecraft.loader.as_deref(),
        Some("vanilla")
    );
    assert!(
        parse_manifest(
            crate::ysumc::MANIFEST_URL,
            &serde_json::to_string(&friends).unwrap()
        )
        .is_ok()
    );
}

#[test]
fn split_merge_replaces_legacy_peers_and_preserves_home_and_bindings() {
    let mut catalog = parse(&document().to_string()).unwrap();
    let previous = catalog.clone();
    merge_friends(
        &mut catalog,
        parse_friends(&friends_document().to_string()).unwrap(),
    );
    reconcile_server_revisions(&mut catalog, Some(&previous));
    assert_eq!(
        catalog
            .instances
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["survival", "sitmc:survival", "sitmc:vanilla"]
    );
    assert_eq!(catalog.instances[1].revision, 12);
    assert_eq!(
        catalog.instances[1].pack.url,
        "https://skin.sitmc.club/pack.mrpack"
    );
    let mut empty = friends_document();
    empty["sites"] = json!([]);
    merge_friends(&mut catalog, parse_friends(&empty.to_string()).unwrap());
    assert_eq!(catalog.instances.len(), 1);
    assert_eq!(catalog.instances[0].id, "survival");
}

#[test]
fn friends_reject_wrong_namespaces_home_identity_and_bad_entries_individually()
{
    let mut doc = friends_document();
    let valid = doc["sites"][0]["instances"][0].clone();
    for id in ["survival", "ysumc:survival", "sitmc:third:pack", "../evil"] {
        let mut bad = valid.clone();
        bad["id"] = json!(id);
        doc["sites"][0]["instances"]
            .as_array_mut()
            .unwrap()
            .push(bad);
    }
    doc["sites"][0]["instances"]
        .as_array_mut()
        .unwrap()
        .push(valid);
    let mut own = doc["sites"][0].clone();
    own["domain"] = json!("SKIN.YSUMC.NET");
    doc["sites"]
        .as_array_mut()
        .unwrap()
        .extend([own, json!({"malformed":true})]);
    let parsed = parse_friends(&doc.to_string()).unwrap();
    assert_eq!(parsed.instances.len(), 2);
    assert_eq!(parsed.diagnostics.len(), 7);
    for field in ["schema", "generated_at", "sites"] {
        let mut bad = friends_document();
        bad.as_object_mut().unwrap().remove(field);
        assert!(parse_friends(&bad.to_string()).is_err());
    }
    doc["schema"] = json!(2);
    assert!(parse_friends(&doc.to_string()).is_err());
}

#[tokio::test]
async fn friends_requests_validate_304_and_use_separate_snapshot_keys() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let body = friends_document().to_string();
    let (url, task) = http_response(format!("HTTP/1.1 200 OK\r\nETag: \"friends\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body)).await;
    let snapshot = request(&client, &url, None, CatalogKind::Friends)
        .await
        .unwrap();
    let headers = task.await.unwrap().to_lowercase();
    assert!(headers.contains("x-ysumc-catalog: split-v1"));
    assert!(!headers.contains("authorization:"));
    assert!(!headers.contains("cookie:"));
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::query("CREATE TABLE managed_manifest_cache(manifest_url TEXT PRIMARY KEY, body TEXT, updated INTEGER)").execute(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    store_manifest_body(
        &mut tx,
        crate::ysumc::FRIENDS_URL,
        &serde_json::to_string(&snapshot).unwrap(),
        1,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(
        cached_snapshot(&pool, crate::ysumc::INTEROP_URL, CatalogKind::Site)
            .await
            .unwrap()
            .is_none()
    );
    let cached =
        cached_snapshot(&pool, crate::ysumc::FRIENDS_URL, CatalogKind::Friends)
            .await
            .unwrap()
            .unwrap();
    let (url, task) = http_response(
        "HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert_eq!(
        request(&client, &url, Some(&cached), CatalogKind::Friends)
            .await
            .unwrap()
            .body,
        body
    );
    assert!(task.await.unwrap().contains("\"friends\""));
    let (url, task) = http_response(
        "HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(
        request(&client, &url, Some(&cached), CatalogKind::Site)
            .await
            .is_err()
    );
    task.await.unwrap();
}

fn document() -> Value {
    json!({
        "site":{"id":"ysumc", "name":"YSUMC", "description":"", "domain":"skin.ysumc.net"},
        "updatedAt":"2026-10-22T09:34:51Z", "peers":[],
        "packs":[{
            "id":"survival", "name":"YSU x CSU", "gameVersion":"26.3", "loader":"fabric",
            "loaderVersion":"0.19.5", "revision":1,
            "artifact":{"format":"mrpack", "url":"https://skin.ysumc.net/api/launcher/packs/survival/1",
                "size":19289037, "sha1":"d268d8cc1a640832d0f4bbfd82459cb09c15b561", "sha512":null},
            "server":{"address":"play.southmount.cn", "port":25565}
        }, {
            "id":"sitmc:survival", "name":"SIT Survival", "gameVersion":"26.2", "loader":"fabric",
            "loaderVersion":"0.19.5", "revision":12,
            "artifact":{"format":"mrpack", "url":"https://skin.sitmc.club/api/launcher/packs/survival/12",
                "size":511459395, "sha1":"b00e0021aada1321b81cbd9693782df39b5f67a0", "sha512":null},
            "server":{"address":"play.sitmc.club"},
            "source":{"siteId":"sitmc", "domain":"skin.sitmc.club", "stale":true}
        }]
    })
}

#[test]
fn catalog_preserves_peer_namespace_array_order_and_missing_port() {
    let doc = parse(&document().to_string()).unwrap();
    assert_eq!(doc.instances.len(), 2);
    assert_eq!(doc.instances[0].id, "survival");
    assert_eq!(doc.instances[1].id, "sitmc:survival");
    assert_eq!(doc.instances[1].server.as_ref().unwrap().port, None);
    assert!(doc.instances[1].source.as_ref().unwrap().stale);
    assert_eq!(doc.instances[1].revision, 12);
    assert_eq!(doc.instances[1].sort, 1);
    // The normalized snapshot remains readable by the native install/launch path.
    assert!(
        parse_manifest(
            crate::ysumc::MANIFEST_URL,
            &serde_json::to_string(&doc).unwrap()
        )
        .is_ok()
    );
}

#[test]
fn peer_details_survive_normalization_without_changing_pack_identity() {
    let mut doc = document();
    doc["peers"] = json!([{
        "siteId":"sitmc", "domain":"skin.sitmc.club",
        "name":"SIT-Minecraft", "description":"Shanghai Institute of Technology Minecraft Club"
    }]);
    let normalized = parse(&doc.to_string()).unwrap();
    assert!(normalized.instances[0].source.is_none());
    let peer = &normalized.instances[1];
    assert_eq!(peer.id, "sitmc:survival");
    let source = peer.source.as_ref().unwrap();
    assert_eq!(source.name, "SIT-Minecraft");
    assert_eq!(
        source.description,
        "Shanghai Institute of Technology Minecraft Club"
    );
    assert!(source.stale);
    let round_trip: ManagedManifest =
        serde_json::from_str(&serde_json::to_string(&normalized).unwrap())
            .unwrap();
    assert_eq!(
        round_trip.instances[1].source.as_ref().unwrap().name,
        source.name
    );
}

#[test]
fn missing_or_mismatched_peer_details_keep_the_valid_catalog_usable() {
    let mut doc = document();
    doc["peers"] = json!([
        {"siteId":"sitmc", "domain":"wrong.example", "name":"Wrong club"},
        {"siteId":"other", "domain":"skin.sitmc.club", "name":"Other club"},
        {"siteId":"sitmc", "domain":"skin.sitmc.club", "name":42}
    ]);
    let parsed = parse(&doc.to_string()).unwrap();
    assert_eq!(parsed.instances.len(), 2);
    assert!(parsed.instances[1].source.as_ref().unwrap().name.is_empty());
    doc.as_object_mut().unwrap().remove("peers");
    assert_eq!(parse(&doc.to_string()).unwrap().instances.len(), 2);
}

#[test]
fn malformed_and_unknown_entries_do_not_hide_valid_packs() {
    let mut doc = document();
    let good = doc["packs"][0].clone();
    let mut unknown = good.clone();
    unknown["loader"] = json!("new-loader");
    let mut malformed = good.clone();
    malformed["artifact"]["sha1"] = json!("bad");
    let mut namespace = good.clone();
    namespace["id"] = json!("sitmc:survival");
    doc["packs"]
        .as_array_mut()
        .unwrap()
        .extend([unknown, malformed, namespace, good]);
    doc["futureField"] = json!({"ignored":true});
    let parsed = parse(&doc.to_string()).unwrap();
    assert_eq!(parsed.instances.len(), 2);
    assert_eq!(parsed.diagnostics.len(), 4);
    for id in ["../x", "a:b:c", "a:", ":b", "a/evil", ".."] {
        assert!(validate_server_instance_id(id).is_err(), "{id}");
    }
    assert!(
        validate_server_instance_id(&format!(
            "{}:{}",
            "a".repeat(128),
            "b".repeat(128)
        ))
        .is_ok()
    );
}

#[test]
fn sha512_only_and_known_but_unsupported_formats_remain_visible() {
    let mut doc = document();
    doc["packs"][0]["artifact"]["sha1"] = Value::Null;
    doc["packs"][0]["artifact"]["sha512"] = json!("a".repeat(128));
    doc["packs"][1]["artifact"]["format"] = json!("curseforge-zip");
    let parsed = parse(&doc.to_string()).unwrap();
    assert_eq!(parsed.instances.len(), 2);
    assert_eq!(parsed.instances[0].pack.sha1, None);
    assert_eq!(parsed.instances[1].pack.kind, "curseforge-zip");
    doc["packs"][1]["artifact"]["format"] = json!("future-format");
    assert_eq!(parse(&doc.to_string()).unwrap().instances.len(), 1);
}

#[test]
fn refuses_wrong_home_identity_missing_catalog_and_insecure_artifacts() {
    for field in ["site", "packs", "updatedAt"] {
        let mut doc = document();
        doc.as_object_mut().unwrap().remove(field);
        assert!(parse(&doc.to_string()).is_err());
    }
    let mut doc = document();
    doc["site"]["domain"] = json!("other.example");
    assert!(parse(&doc.to_string()).is_err());
    let mut doc = document();
    doc["packs"][0]["artifact"]["url"] = json!("http://example.com/a");
    assert_eq!(parse(&doc.to_string()).unwrap().instances.len(), 1);
}

#[test]
fn home_publication_is_independent_of_public_listing_and_peer_namespaces() {
    let mut catalog = parse(&document().to_string()).unwrap();
    let mut legacy = catalog.clone();
    legacy.instances[0].name = "Official current name".into();
    legacy.instances[0].pack.url =
        "https://skin.ysumc.net/current.mrpack".into();
    legacy.instances[1].name = "Must not override peer".into();
    legacy.instances[0].server.as_mut().unwrap().opened_on =
        Some("2022-01-01".into());
    legacy.cloud_map_url = Some("https://ysumc.net".into());
    let mut hidden = legacy.instances[0].clone();
    hidden.id = "published-but-unlisted".into();
    legacy.instances.push(hidden);
    merge_home_manifest(&mut catalog, &legacy);
    assert_eq!(catalog.instances.len(), 3);
    assert_eq!(catalog.instances[0].name, "Official current name");
    assert_eq!(
        catalog.instances[0].pack.url,
        "https://skin.ysumc.net/current.mrpack"
    );
    assert!(
        catalog
            .instances
            .iter()
            .any(|s| s.id == "published-but-unlisted")
    );
    assert_ne!(catalog.instances[2].name, "Must not override peer");
    assert_eq!(
        catalog.instances[0]
            .server
            .as_ref()
            .unwrap()
            .opened_on
            .as_deref(),
        Some("2022-01-01")
    );
    assert_eq!(catalog.cloud_map_url, legacy.cloud_map_url);
    catalog.instances.clear();
    merge_home_manifest(&mut catalog, &legacy);
    assert_eq!(catalog.instances.len(), 2);
}

#[test]
fn home_withdrawal_removes_own_mrpack_but_keeps_interop_only_formats() {
    let mut catalog = parse(&document().to_string()).unwrap();
    let mut server = catalog.instances[0].clone();
    server.id = "server-only".into();
    server.pack.kind = "server".into();
    catalog.instances.push(server);
    let mut sha512_pack = catalog.instances[0].clone();
    sha512_pack.id = "sha512-only".into();
    sha512_pack.pack.sha1 = None;
    sha512_pack.pack.sha512 = Some("a".repeat(128));
    catalog.instances.push(sha512_pack);
    let mut home = catalog.clone();
    home.instances.clear();
    merge_home_manifest(&mut catalog, &home);
    assert_eq!(
        catalog
            .instances
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["sitmc:survival", "server-only", "sha512-only"]
    );
}

#[test]
fn server_only_uses_requested_game_and_loader_and_tracks_version_changes() {
    let mut doc = document();
    doc["packs"][0].as_object_mut().unwrap().remove("artifact");
    doc["packs"][0].as_object_mut().unwrap().remove("revision");
    let old = parse(&doc.to_string()).unwrap();
    let mut new = old.clone();
    reconcile_server_revisions(&mut new, Some(&old));
    assert_eq!(new.instances[0].revision, 1);
    new.instances[0].minecraft.game_version = "26.4".into();
    reconcile_server_revisions(&mut new, Some(&old));
    assert_eq!(new.instances[0].revision, 2);
    let archive = server_archive(&new.instances[0]).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).unwrap();
    let index: Value =
        serde_json::from_reader(zip.by_name("modrinth.index.json").unwrap())
            .unwrap();
    assert_eq!(index["dependencies"]["minecraft"], "26.4");
    assert_eq!(index["dependencies"]["fabric-loader"], "0.19.5");
    assert_eq!(index["files"], json!([]));
}

async fn http_response(
    response: String,
) -> (String, tokio::task::JoinHandle<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/catalog", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = vec![0u8; 8192];
        let read = socket.read(&mut bytes).await.unwrap();
        socket.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8_lossy(&bytes[..read]).into_owned()
    });
    (url, task)
}

#[tokio::test]
async fn conditional_request_reuses_304_snapshot_without_credentials() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let cached = Snapshot {
        body: document().to_string(),
        etag: Some("\"catalog-v1\"".into()),
    };
    let (url, task) = http_response(
        "HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    let result = request(&client, &url, Some(&cached), CatalogKind::Site)
        .await
        .unwrap();
    assert_eq!(result.body, cached.body);
    let headers = task.await.unwrap().to_lowercase();
    assert!(headers.contains("if-none-match: \"catalog-v1\""));
    assert!(headers.contains("x-ysumc-catalog: split-v1"));
    assert!(!headers.contains("authorization:"));
    assert!(!headers.contains("cookie:"));
    let (url, task) = http_response(
        "HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(
        request(&client, &url, None, CatalogKind::Site)
            .await
            .is_err()
    );
    task.await.unwrap();
}

#[tokio::test]
async fn request_rejects_oversized_and_invalid_documents() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for response in [
        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", MAX_DOCUMENT_BYTES + 1),
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".into(),
        "HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
    ] {
        let (url, task) = http_response(response).await;
        assert!(request(&client, &url, None, CatalogKind::Site).await.is_err());
        task.await.unwrap();
    }
}

#[tokio::test]
async fn sha512_pack_verification_precedes_sha1_and_detects_corruption() {
    use crate::api::pack::install_from::verify_pack_file;
    use sha2::{Digest, Sha512};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pack.mrpack");
    let bytes = b"verified pack bytes";
    tokio::fs::write(&path, bytes).await.unwrap();
    let hash = format!("{:x}", Sha512::digest(bytes));
    verify_pack_file(&path, None, Some(&hash), Some(bytes.len() as u64))
        .await
        .unwrap();
    verify_pack_file(
        &path,
        Some(&"0".repeat(40)),
        Some(&hash),
        Some(bytes.len() as u64),
    )
    .await
    .unwrap();
    assert!(
        verify_pack_file(
            &path,
            None,
            Some(&"0".repeat(128)),
            Some(bytes.len() as u64)
        )
        .await
        .is_err()
    );
    assert!(
        verify_pack_file(&path, None, Some(&hash), Some(1))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn database_keeps_home_and_peer_bindings_separate_and_port_unspecified() {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql("CREATE TABLE settings(theme TEXT); INSERT INTO settings VALUES ('dark');")
        .execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261020120000_ysumc-managed-instances.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let catalog = parse(&document().to_string()).unwrap();
    let mut tx = pool.begin().await.unwrap();
    for spec in &catalog.instances {
        store_spec(&mut tx, spec, spec.revision, 1).await.unwrap();
    }
    tx.commit().await.unwrap();
    sqlx::query("UPDATE managed_instances SET instance_id = 'existing-ysu', revision = 1 WHERE server_instance_id = 'survival'")
        .execute(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    for spec in &catalog.instances {
        store_spec(&mut tx, spec, spec.revision, 2).await.unwrap();
    }
    tx.commit().await.unwrap();
    let rows: Vec<(String, String, i64)> = sqlx::query_as("SELECT server_instance_id, instance_id, server_port FROM managed_instances ORDER BY sort")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(
        rows,
        vec![
            ("survival".into(), "existing-ysu".into(), 25565),
            ("sitmc:survival".into(), "".into(), 0)
        ]
    );
}

#[test]
fn audience_catalog_never_reintroduces_restricted_home_entries_from_public_cache()
 {
    let public = document();
    let mut restricted = document();
    restricted["packs"] = json!([]);
    let home = json!({"catalog": restricted});
    let mut catalog =
        audience_catalog(&home.to_string(), &public.to_string()).unwrap();
    assert!(catalog.instances.is_empty());
    merge_friends(
        &mut catalog,
        parse_friends(&friends_document().to_string()).unwrap(),
    );
    assert_eq!(catalog.instances.len(), 2);
    assert!(catalog.instances.iter().all(|entry| entry.source.is_some()));
    assert!(
        audience_catalog(r#"{"catalog":null}"#, &public.to_string()).is_err()
    );
    assert!(
        !audience_catalog("{}", &public.to_string())
            .unwrap()
            .instances
            .is_empty()
    );
}
