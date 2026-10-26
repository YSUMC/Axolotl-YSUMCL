use super::*;
use tokio::io::AsyncWriteExt;

fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn mock_route(
    source: RequestRouteSource,
    response: String,
) -> (RequestRoute, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) =
            tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let count = stream.read(&mut buffer).await.unwrap();
            assert!(count > 0, "request ended before its body");
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n")
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
        if !response.is_empty() {
            stream.write_all(response.as_bytes()).await.unwrap();
        }
        String::from_utf8(request).unwrap()
    });
    (
        RequestRoute {
            url: format!("http://{address}/v1/categories"),
            source,
            use_launcher_token: source == RequestRouteSource::Official,
            use_system_proxy: false,
        },
        task,
    )
}

async fn execute(
    routes: &[RequestRoute],
    key: Option<&str>,
    method: Method,
    body: Option<&Value>,
    unauthorized: &AtomicBool,
) -> crate::Result<Value> {
    let semaphore = tokio::sync::Semaphore::new(1);
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        request_json_with_routes(
            method,
            &[("gameId".to_string(), "432".to_string())],
            body,
            routes,
            key,
            &semaphore,
            unauthorized,
        ),
    )
    .await
    .expect("fallback should not wait for a failed host's Retry-After");
    assert_eq!(semaphore.available_permits(), 1);
    result
}

#[tokio::test]
async fn mirror_503_falls_back_immediately_and_keeps_token_off_mirror() {
    let (mirror, mirror_request) = mock_route(RequestRouteSource::Mirror,
        "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 30\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()).await;
    let (official, official_request) = mock_route(
        RequestRouteSource::Official,
        http_response("200 OK", r#"{"data":[{"id":6}]}"#),
    )
    .await;
    let result = execute(
        &[mirror, official],
        Some("test-launcher-token"),
        Method::GET,
        None,
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert_eq!(result["data"][0]["id"], 6);
    let mirror_request = mirror_request.await.unwrap().to_ascii_lowercase();
    let official_request = official_request.await.unwrap().to_ascii_lowercase();
    assert!(mirror_request.starts_with("get /v1/categories?gameid=432 "));
    assert!(!mirror_request.contains("authorization:"));
    assert!(!mirror_request.contains("test-launcher-token"));
    assert!(official_request.starts_with("get /v1/categories?gameid=432 "));
    assert!(
        official_request.contains("authorization: bearer test-launcher-token")
    );
}

#[tokio::test]
async fn mirror_invalid_json_and_interrupted_body_fall_back() {
    for response in [
        http_response("200 OK", "<html>maintenance</html>"),
        "HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n{\"data\":".to_string(),
        String::new(),
    ] {
        let (mirror, mirror_request) = mock_route(RequestRouteSource::Mirror, response).await;
        let (official, official_request) = mock_route(RequestRouteSource::Official,
            http_response("200 OK", r#"{"data":[]}"#)).await;
        assert_eq!(execute(&[mirror, official], Some("test-launcher-token"), Method::GET,
            None, &AtomicBool::new(false)).await.unwrap(), json!({"data": []}));
        mirror_request.await.unwrap();
        official_request.await.unwrap();
    }
}

#[tokio::test]
async fn read_only_post_fallback_preserves_body_and_query() {
    let (mirror, mirror_request) = mock_route(
        RequestRouteSource::Mirror,
        http_response("503 Service Unavailable", ""),
    )
    .await;
    let (official, official_request) = mock_route(
        RequestRouteSource::Official,
        http_response("200 OK", r#"{"data":[]}"#),
    )
    .await;
    let body = json!({"modIds": [1, 2]});
    execute(
        &[mirror, official],
        Some("test-launcher-token"),
        Method::POST,
        Some(&body),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    for request in [
        mirror_request.await.unwrap(),
        official_request.await.unwrap(),
    ] {
        assert!(request.starts_with("POST /v1/categories?gameId=432 "));
        let (_, request_body) = request.split_once("\r\n\r\n").unwrap();
        assert_eq!(serde_json::from_str::<Value>(request_body).unwrap(), body);
    }
}

#[tokio::test]
async fn missing_login_is_not_hidden_by_mirror_failure_in_either_order() {
    for mirror_first in [true, false] {
        let (mirror, mirror_request) = mock_route(
            RequestRouteSource::Mirror,
            http_response("503 Service Unavailable", ""),
        )
        .await;
        let official = RequestRoute {
            url: "http://127.0.0.1:1/must-not-be-requested".to_string(),
            source: RequestRouteSource::Official,
            use_launcher_token: true,
            use_system_proxy: false,
        };
        let routes = if mirror_first {
            vec![mirror, official]
        } else {
            vec![official, mirror]
        };
        let error =
            execute(&routes, None, Method::GET, None, &AtomicBool::new(false))
                .await
                .unwrap_err();
        assert!(error.to_string().contains("Sign in to YSUMC again"));
        mirror_request.await.unwrap();
    }
}

#[tokio::test]
async fn mirror_remains_usable_without_ysumc_session() {
    let (mirror, request) = mock_route(
        RequestRouteSource::Mirror,
        http_response("200 OK", r#"{"data":[]}"#),
    )
    .await;
    execute(&[mirror], None, Method::GET, None, &AtomicBool::new(false))
        .await
        .unwrap();
    assert!(
        !request
            .await
            .unwrap()
            .to_ascii_lowercase()
            .contains("authorization:")
    );
}

#[tokio::test]
async fn mirror_auth_failure_does_not_poison_official_credentials() {
    let unauthorized = AtomicBool::new(false);
    let (mirror, request) = mock_route(
        RequestRouteSource::Mirror,
        http_response("401 Unauthorized", ""),
    )
    .await;
    assert!(
        execute(&[mirror], None, Method::GET, None, &unauthorized)
            .await
            .is_err()
    );
    assert!(!unauthorized.load(Ordering::Relaxed));
    request.await.unwrap();
}

#[tokio::test]
async fn official_auth_failure_can_use_mirror_without_clearing_auth_failure() {
    let unauthorized = AtomicBool::new(false);
    let (official, official_request) = mock_route(
        RequestRouteSource::Official,
        http_response("401 Unauthorized", ""),
    )
    .await;
    let (mirror, mirror_request) = mock_route(
        RequestRouteSource::Mirror,
        http_response("200 OK", r#"{"data":[]}"#),
    )
    .await;
    execute(
        &[official, mirror],
        Some("test-launcher-token"),
        Method::GET,
        None,
        &unauthorized,
    )
    .await
    .unwrap();
    assert!(unauthorized.load(Ordering::Relaxed));
    official_request.await.unwrap();
    mirror_request.await.unwrap();
    let (official, official_request) = mock_route(
        RequestRouteSource::Official,
        http_response("200 OK", r#"{"data":[]}"#),
    )
    .await;
    execute(
        &[official],
        Some("test-launcher-token"),
        Method::GET,
        None,
        &unauthorized,
    )
    .await
    .unwrap();
    assert!(!unauthorized.load(Ordering::Relaxed));
    official_request.await.unwrap();
}

#[tokio::test]
async fn exhausted_sources_return_failure_instead_of_fake_empty_results() {
    let (mirror, mirror_request) = mock_route(
        RequestRouteSource::Mirror,
        http_response("503 Service Unavailable", ""),
    )
    .await;
    let (official, official_request) = mock_route(
        RequestRouteSource::Official,
        http_response("503 Service Unavailable", ""),
    )
    .await;
    assert!(
        execute(
            &[mirror, official],
            Some("test-launcher-token"),
            Method::GET,
            None,
            &AtomicBool::new(false)
        )
        .await
        .is_err()
    );
    mirror_request.await.unwrap();
    official_request.await.unwrap();
}
