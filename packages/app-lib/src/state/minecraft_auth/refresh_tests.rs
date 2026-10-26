use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) struct MockHttp {
    pub root: String,
    pub requests: tokio::sync::mpsc::UnboundedReceiver<String>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for MockHttp {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) async fn mock_http(
    responses: Vec<Option<(u16, serde_json::Value)>>,
) -> MockHttp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = format!("http://{}", listener.local_addr().unwrap());
    let (tx, requests) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        for response in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
                if let Some(end) =
                    request.windows(4).position(|w| w == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..end])
                        .to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .map(|value| value.trim().parse().unwrap())
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            tx.send(String::from_utf8(request).unwrap()).unwrap();
            let Some((status, body)) = response else {
                std::future::pending::<()>().await;
                unreachable!();
            };
            let body = if status == 204 {
                String::new()
            } else {
                body.to_string()
            };
            socket.write_all(format!(
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()
            ).as_bytes()).await.unwrap();
        }
    });
    MockHttp {
        root,
        requests,
        task,
    }
}

pub(super) async fn saved_account(
    api_root: &str,
    oidc: bool,
) -> (sqlx::SqlitePool, Credentials) {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!().run(&pool).await.unwrap();
    let mut credentials = yggdrasil::create_credentials(
        YggdrasilProfile {
            id: Uuid::new_v4(),
            name: "BeforeRefresh".into(),
        },
        "old-access".into(),
        if oidc { "" } else { "client-token" }.into(),
        YggdrasilMetadata {
            api_root: api_root.into(),
            server_name: "Test".into(),
            raw: "{}".into(),
        },
        "",
    );
    credentials.refresh_token = if oidc { "old-refresh" } else { "" }.into();
    credentials.expires = Utc::now() - Duration::days(2);
    sqlx::query("INSERT INTO minecraft_users (account_id, uuid, active, username, account_type, access_token, refresh_token, expires, yggdrasil_api_root, yggdrasil_server_name, yggdrasil_login, yggdrasil_client_token) VALUES ($1, $2, TRUE, $3, 'yggdrasil', $4, $5, $6, $7, 'Test', '', $8)")
        .bind(credentials.account_id()).bind(credentials.offline_profile.id.to_string())
        .bind(&credentials.offline_profile.name).bind(&credentials.access_token)
        .bind(&credentials.refresh_token).bind(credentials.expires.timestamp()).bind(api_root)
        .bind(&credentials.yggdrasil.as_ref().unwrap().client_token)
        .execute(&pool).await.unwrap();
    (pool, credentials)
}

#[tokio::test]
async fn launch_refreshes_expired_classic_token_once_for_concurrent_readers() {
    let mut server = mock_http(vec![
        Some((403, json!({"error":"ForbiddenOperationException"}))),
        Some((200, json!({"accessToken":"new-access","clientToken":"client-token",
            "selectedProfile":{"id":"0123456789abcdef0123456789abcdef","name":"Renamed"}}))),
    ]).await;
    let (pool, _) = saved_account(&server.root, false).await;
    let (first, second) =
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(
                Credentials::get_default_credential(&pool),
                Credentials::get_default_credential(&pool)
            )
        })
        .await
        .unwrap();
    for result in [first, second] {
        let account = result.unwrap().unwrap();
        assert_eq!(account.access_token, "new-access");
        assert_eq!(account.offline_profile.name, "Renamed");
    }
    let validate = server.requests.recv().await.unwrap();
    assert!(validate.starts_with("POST /authserver/validate "));
    let refresh = server.requests.recv().await.unwrap();
    assert!(refresh.starts_with("POST /authserver/refresh "));
    let body: serde_json::Value =
        serde_json::from_str(refresh.split("\r\n\r\n").nth(1).unwrap())
            .unwrap();
    assert!(body.get("selectedProfile").is_none());
    assert_eq!(body["accessToken"], "old-access");
    assert_eq!(body["clientToken"], "client-token");
    let stored = Credentials::get_active_without_refresh(&pool)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.access_token, "new-access");
    assert!(stored.expires > Utc::now());
}

#[tokio::test]
async fn rate_limit_does_not_mark_an_expired_token_valid() {
    let server =
        mock_http(vec![Some((429, json!({"error":"TooManyRequests"})))]).await;
    let (pool, before) = saved_account(&server.root, false).await;
    assert!(Credentials::get_default_credential(&pool).await.is_err());
    let after = Credentials::get_active_without_refresh(&pool)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.expires.timestamp(), before.expires.timestamp());
    assert_eq!(after.access_token, before.access_token);
}

#[tokio::test]
async fn launch_checks_server_even_when_local_expiry_is_in_the_future() {
    let mut server = mock_http(vec![
        Some((403, json!({"error":"ForbiddenOperationException"}))),
        Some((403, json!({"error":"ForbiddenOperationException"}))),
        Some((200, json!({"accessToken":"launch-access", "clientToken":"client-token",
            "selectedProfile":{"id":"0123456789abcdef0123456789abcdef", "name":"Player"}}))),
        Some((204, json!(null))),
    ]).await;
    let (pool, mut account) = saved_account(&server.root, false).await;
    account.expires = Utc::now() + Duration::hours(1);
    account.persist_refreshed(&pool).await.unwrap();
    let ready = Credentials::get_credential_for_launch(&pool)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready.access_token, "launch-access");
    let mut requests = Vec::new();
    for _ in 0..4 {
        requests.push(server.requests.recv().await.unwrap());
    }
    assert!(requests[3].contains("launch-access"));
    assert!(requests[3].starts_with("POST /authserver/validate "));
}

#[tokio::test]
async fn launch_rejects_unreachable_auth_server_instead_of_passing_stale_token_to_game()
 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let (pool, _) = saved_account(&root, false).await;
    assert!(Credentials::get_credential_for_launch(&pool).await.is_err());
    // Optional account reads retain their existing offline fallback.
    assert!(
        Credentials::get_default_credential(&pool)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn launch_reuses_a_valid_token_without_rotating_it() {
    let mut server = mock_http(vec![Some((204, json!(null)))]).await;
    let (pool, mut account) = saved_account(&server.root, false).await;
    account.expires = Utc::now() + Duration::hours(1);
    account.persist_refreshed(&pool).await.unwrap();
    let ready = Credentials::get_credential_for_launch(&pool)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready.access_token, "old-access");
    assert!(
        server
            .requests
            .recv()
            .await
            .unwrap()
            .starts_with("POST /authserver/validate ")
    );
}

#[tokio::test]
async fn permanently_invalid_classic_token_requires_login() {
    let server = mock_http(vec![
        Some((403, json!({"error":"ForbiddenOperationException"}))),
        Some((403, json!({"error":"ForbiddenOperationException"}))),
    ])
    .await;
    let (pool, _) = saved_account(&server.root, false).await;
    let error = Credentials::get_default_credential(&pool)
        .await
        .unwrap_err();
    assert!(matches!(&*error.raw, ErrorKind::CredentialsExpired { .. }));
}

#[tokio::test]
async fn new_classic_login_does_not_reuse_previous_sessions_refresh() {
    let server = mock_http(vec![Some((204, json!(null)))]).await;
    let (pool, mut account) = saved_account(&server.root, false).await;
    let key = account.account_id();
    CREDENTIAL_REFRESHES.insert(
        key.clone(),
        Arc::new(Mutex::new(CredentialRefreshState {
            source_token: Some("different-login".into()),
            failure: None,
            refreshed: Some(RefreshedTokens {
                access_token: "wrong-session".into(),
                refresh_token: String::new(),
                expires: Utc::now() + Duration::hours(1),
                yggdrasil: account.yggdrasil.clone(),
                profile: account.offline_profile.clone(),
            }),
        })),
    );
    account.refresh(&pool).await.unwrap();
    assert_eq!(account.access_token, "old-access");
    CREDENTIAL_REFRESHES.remove(&key);
}

#[tokio::test]
async fn rejected_token_reuses_concurrent_rotation_even_with_shorter_lifetime()
{
    let (pool, mut account) = saved_account("http://127.0.0.1:1", false).await;
    account.expires = Utc::now() + Duration::hours(1);
    let key = account.account_id();
    CREDENTIAL_REFRESHES.insert(
        key.clone(),
        Arc::new(Mutex::new(CredentialRefreshState {
            source_token: Some(account.access_token.clone()),
            failure: None,
            refreshed: Some(RefreshedTokens {
                access_token: "concurrent-access".into(),
                refresh_token: String::new(),
                expires: Utc::now() + Duration::minutes(5),
                yggdrasil: account.yggdrasil.clone(),
                profile: account.offline_profile.clone(),
            }),
        })),
    );
    account.refresh_inner(&pool, true).await.unwrap();
    assert_eq!(account.access_token, "concurrent-access");
    CREDENTIAL_REFRESHES.remove(&key);
}

#[tokio::test]
async fn oidc_refreshes_before_expiry_and_never_falls_back_to_password_protocol()
 {
    let (pool, mut account) =
        saved_account(crate::ysumc::YGGDRASIL_API_ROOT, true).await;
    account.expires = Utc::now() + Duration::minutes(2);
    assert!(account.needs_refresh());
    account.refresh_token.clear();
    let error = account.refresh(&pool).await.unwrap_err();
    assert!(matches!(&*error.raw, ErrorKind::CredentialsExpired { .. }));
    account.yggdrasil.as_mut().unwrap().client_token = "classic-login".into();
    assert!(!account.needs_refresh());
}
