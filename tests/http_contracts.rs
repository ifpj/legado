//! Local HTTP fixtures: no external sites, accounts, tokens or devices.
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use cf_gateway::{
    engine::Engine,
    network::{Dns, Session},
    service::Service,
    web,
};
use flate2::{write::GzEncoder, Compression};
use futures_util::future::join_all;
use serde_json::json;
use std::{
    io::Write,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const ALPHABET: &str = "7KcLp9tIZ2Su+xagjiv05Q$EbdyoOU8rPT3qkCRXGshWnfDmw4zHJYF1Al-VNBe6M";
fn pixel() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aU1sAAAAASUVORK5CYII=").unwrap()
}
#[derive(Default)]
struct Counters {
    get: AtomicUsize,
    post: AtomicUsize,
    active: AtomicUsize,
    maximum: AtomicUsize,
}
struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn serve(routes: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
    Server { url, task }
}
async fn upstream(State(c): State<Arc<Counters>>, r: Request) -> Response {
    let path = r.uri().path();
    if r.method() == "POST" {
        c.post.fetch_add(1, Ordering::SeqCst);
        let mut response = "accepted".into_response();
        response.headers_mut().insert(
            "set-cookie",
            HeaderValue::from_static("cf_clearance=fixture; Path=/; HttpOnly"),
        );
        return response;
    }
    let ordinal = c.get.fetch_add(1, Ordering::SeqCst) + 1;
    match path {
        "/jsd"|"/cover-shielded"|"/reject" if !r.headers().get("cookie").and_then(|h|h.to_str().ok()).is_some_and(|h|h.contains("cf_clearance=fixture"))||path=="/reject" => {
            let html="<html><body><script>window.__CF$cv$params={r:'0123456789abcdef',t:'MTc5MjQ4MDAwMA=='};</script></body></html>";
            (StatusCode::FORBIDDEN,[("content-type","text/html")],html).into_response()
        }
        "/cdn-cgi/challenge-platform/scripts/jsd/main.js" => format!("window._cf_chl_opt={{v:'b'}};var alphabet='{ALPHABET}',path='/jsd/oneshot/fixture/';/*2166136261 16777619 2779062077 Uint8Array*/").into_response(),
        "/managed" => (StatusCode::FORBIDDEN,[("cf-mitigated","challenge"),("content-type","text/html")],"<script>window._cf_chl_opt={cType:'managed'};</script>").into_response(),
        "/loop" => (StatusCode::FOUND,[("location","/loop")],"").into_response(),
        "/missing" => (StatusCode::NOT_FOUND,[("content-type","text/html; charset=utf-8")],"<html>实际网站页面</html>").into_response(),
        "/gzip" => {let mut encoder=GzEncoder::new(Vec::new(),Compression::default());encoder.write_all(&b"test-content-".repeat(1000)).unwrap();let bytes=encoder.finish().unwrap();let mut response=(StatusCode::OK,[("content-encoding","gzip"),("set-cookie","fresh-session=value; Path=/")],bytes).into_response();let length=response.body().size_hint().exact().unwrap().to_string();response.headers_mut().insert("content-length",length.parse().unwrap());response}
        "/large" => Response::new(Body::from(vec![b'x';8*1024*1024+1])),
        "/bomb" => {let mut encoder=GzEncoder::new(Vec::new(),Compression::fast());encoder.write_all(&vec![b'x';8*1024*1024+1]).unwrap();(StatusCode::OK,[("content-encoding","gzip")],encoder.finish().unwrap()).into_response()}
        "/cookie" => r.headers().get("cookie").and_then(|h|h.to_str().ok()).unwrap_or("").to_string().into_response(),
        "/invalid-encoding" => (StatusCode::OK,[("content-encoding","unsupported-test-encoding")],"invalid response").into_response(),
        "/cover"|"/cover-shielded" => (StatusCode::OK,[("content-type","image/png")],pixel()).into_response(),
        "/cover-gzip" => {let mut encoder=GzEncoder::new(Vec::new(),Compression::fast());encoder.write_all(&pixel()).unwrap();(StatusCode::OK,[("content-type","image/png"),("content-encoding","gzip")],encoder.finish().unwrap()).into_response()},
        "/cover-svg" => (StatusCode::OK,[("content-type","image/svg+xml")],"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>").into_response(),
        "/headers" => axum::Json(r.headers().iter().map(|(k,v)|(k.to_string(),json!(v.to_str().unwrap_or("")))).collect::<serde_json::Map<_,_>>()).into_response(),
        _ => {
            let active=c.active.fetch_add(1,Ordering::SeqCst)+1;c.maximum.fetch_max(active,Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(if path=="/slow"{1500}else{35})).await;
            c.active.fetch_sub(1,Ordering::SeqCst);
            (StatusCode::OK,[("content-type","text/html; charset=utf-8")],format!("<html><body>fresh {path} {ordinal}</body></html>")).into_response()
        }
    }
}
use axum::body::HttpBody;
async fn fixture() -> (Server, Arc<Counters>) {
    let counters = Arc::new(Counters::default());
    let server = serve(
        Router::new()
            .fallback(upstream)
            .with_state(counters.clone()),
    )
    .await;
    (server, counters)
}
fn service() -> Service {
    Service::new(Engine::default()).unwrap()
}
fn session() -> Session {
    Session::new(Dns::new(false).unwrap(), None).unwrap()
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[tokio::test]
async fn original_status_utf8_and_no_response_cache() {
    let (server, c) = fixture().await;
    let service = service();
    let missing = service
        .fetch(json!({"url":server.url.clone()+"/missing"}), false, false)
        .await
        .unwrap();
    assert_eq!(missing["status"], 404);
    assert!(missing["body"].as_str().unwrap().contains("实际网站页面"));
    let a = service
        .fetch(json!({"url":server.url.clone()+"/chapter"}), false, false)
        .await
        .unwrap();
    let b = service
        .fetch(json!({"url":server.url.clone()+"/chapter"}), false, false)
        .await
        .unwrap();
    assert_ne!(a["body"], b["body"]);
    assert_eq!(c.get.load(Ordering::SeqCst), 3);
    assert_eq!(b["contentCached"], false);
}
#[tokio::test]
async fn analyze_never_submits_and_does_not_claim_native_success() {
    let (server, c) = fixture().await;
    let s = service();
    let result = s
        .fetch(json!({"url":server.url.clone()+"/jsd"}), true, false)
        .await
        .unwrap();
    assert_eq!(c.post.load(Ordering::SeqCst), 0);
    assert_eq!(result["analyzed"], true);
    assert_eq!(result["strictPureAlgorithmAcceptance"], false);
}
#[tokio::test]
async fn jsd_submission_requires_independent_final_page() {
    let (server, c) = fixture().await;
    let s = service();
    let result = s
        .fetch(json!({"url":server.url.clone()+"/jsd"}), false, false)
        .await
        .unwrap();
    assert_eq!(result["status"], 200);
    assert!(result["body"].as_str().unwrap().contains("fresh /jsd"));
    assert_eq!(c.post.load(Ordering::SeqCst), 1);
    assert_eq!(c.get.load(Ordering::SeqCst), 3);
    assert!(s
        .fetch(
            json!({"url":server.url.clone()+"/reject","session":"reject"}),
            false,
            false
        )
        .await
        .is_err());
}
#[tokio::test]
async fn solve_exports_only_target_credentials_and_redacts_recent_records() {
    let (server, _) = fixture().await;
    let s = service();
    let r = s
        .fetch(json!({"url":server.url.clone()+"/jsd"}), false, true)
        .await
        .unwrap();
    assert!(r.get("body").is_none());
    assert_eq!(r["credentials"]["cookieHeader"], "cf_clearance=fixture");
    assert_eq!(r["credentials"]["clientCompatibilityVerified"], false);
    let recent = s.status()["recent"][0].clone();
    assert_eq!(recent["operation"], "solve");
    assert!(recent.get("credentials").is_none());
    assert!(!recent.to_string().contains("cf_clearance"));
}
#[tokio::test]
async fn unsupported_challenge_and_redirect_loop_never_report_success() {
    let (server, _) = fixture().await;
    let s = service();
    assert!(s
        .fetch(json!({"url":server.url.clone()+"/managed"}), false, false)
        .await
        .is_err());
    assert!(s
        .fetch(json!({"url":server.url.clone()+"/loop"}), false, false)
        .await
        .is_err());
    let r = s.status();
    assert_eq!(r["failed"], 2);
    assert_eq!(r["success"], 0);
    assert_eq!(r["active"], 0);
}
#[tokio::test]
async fn verified_reads_are_four_way_and_distinct() {
    let (server, c) = fixture().await;
    let s = Arc::new(service());
    s.fetch(json!({"url":server.url.clone()+"/prime"}), false, false)
        .await
        .unwrap();
    let rows = join_all((0..8).map(|i| {
        let s = s.clone();
        let url = format!("{}/chapter-{i}", server.url);
        async move { s.fetch(json!({"url":url}), false, false).await.unwrap() }
    }))
    .await;
    for (i, row) in rows.iter().enumerate() {
        assert!(row["body"]
            .as_str()
            .unwrap()
            .contains(&format!("/chapter-{i}")));
    }
    assert!((2..=4).contains(&c.maximum.load(Ordering::SeqCst)));
    assert_eq!(s.status()["success"], 9);
    assert_eq!(s.status()["active"], 0);
}
#[tokio::test]
async fn simultaneous_initial_requests_share_only_verification() {
    let (server, c) = fixture().await;
    let s = Arc::new(service());
    let rows = join_all((0..8).map(|i| {
        let s = s.clone();
        let url = format!("{}/initial-{i}", server.url);
        async move { s.fetch(json!({"url":url}), false, false).await.unwrap() }
    }))
    .await;
    assert_eq!(rows.len(), 8);
    assert_eq!(c.get.load(Ordering::SeqCst), 8);
    assert!((2..=4).contains(&c.maximum.load(Ordering::SeqCst)));
    assert_eq!(s.status()["active"], 0);
}
#[tokio::test]
async fn redirects_preserve_or_rewrite_method_and_bound_origin_headers() {
    async fn redirect(r: Request) -> Response {
        if r.uri().path() == "/receive" {
            return axum::Json(json!({"method":r.method().as_str(),"authorization":r.headers().get("authorization").and_then(|v|v.to_str().ok()),"body":String::from_utf8_lossy(&axum::body::to_bytes(r.into_body(),1024).await.unwrap())})).into_response();
        }
        let status = if r.uri().path() == "/307" {
            StatusCode::TEMPORARY_REDIRECT
        } else {
            StatusCode::SEE_OTHER
        };
        (status, [("location", "/receive")], "").into_response()
    }
    let server = serve(Router::new().fallback(redirect)).await;
    let s = session();
    let a = s
        .request(
            "POST",
            &(server.url.clone() + "/307"),
            Some(b"body".to_vec()),
            &[("authorization", "private-test".into())],
            deadline(),
        )
        .await
        .unwrap();
    let a: serde_json::Value = serde_json::from_str(&a.text).unwrap();
    assert_eq!(a["method"], "POST");
    assert_eq!(a["body"], "body");
    assert_eq!(a["authorization"], "private-test");
    let b = s
        .request(
            "POST",
            &(server.url.clone() + "/303"),
            Some(b"body".to_vec()),
            &[],
            deadline(),
        )
        .await
        .unwrap();
    let b: serde_json::Value = serde_json::from_str(&b.text).unwrap();
    assert_eq!(b["method"], "GET");
    assert_eq!(b["body"], "");
    let destination = server.url.clone() + "/receive";
    let other = serve(Router::new().fallback(move || {
        let url = destination.clone();
        async move { (StatusCode::FOUND, [("location", url)], "") }
    }))
    .await;
    let r = s
        .request(
            "GET",
            &other.url,
            None,
            &[("authorization", "must-not-cross-origin".into())],
            deadline(),
        )
        .await
        .unwrap();
    let r: serde_json::Value = serde_json::from_str(&r.text).unwrap();
    assert!(r["authorization"].is_null());
}
#[tokio::test]
async fn transport_retains_wire_size_and_cookies_without_cross_session_leaks() {
    let (server, _) = fixture().await;
    let s = session();
    let r = s
        .request(
            "GET",
            &(server.url.clone() + "/gzip"),
            None,
            &[],
            deadline(),
        )
        .await
        .unwrap();
    assert_eq!(r.content, b"test-content-".repeat(1000));
    assert_eq!(
        r.download_size,
        r.headers["content-length"]
            .as_str()
            .unwrap()
            .parse::<usize>()
            .unwrap()
    );
    assert!(r.download_size < r.content.len());
    let cookie = s
        .request(
            "GET",
            &(server.url.clone() + "/cookie"),
            None,
            &[],
            deadline(),
        )
        .await
        .unwrap();
    assert!(cookie.text.contains("fresh-session=value"));
    let other = session()
        .request(
            "GET",
            &(server.url.clone() + "/cookie"),
            None,
            &[],
            deadline(),
        )
        .await
        .unwrap();
    assert!(other.text.is_empty());
}
#[tokio::test]
async fn both_wire_and_decompressed_body_have_hard_limits() {
    let (server, _) = fixture().await;
    let s = session();
    for path in ["/large", "/bomb"] {
        let e = s
            .request("GET", &(server.url.clone() + path), None, &[], deadline())
            .await
            .unwrap_err();
        assert_eq!(e.code, "RESPONSE_TOO_LARGE");
    }
}
#[tokio::test]
async fn expired_deadline_never_starts_http_and_queue_timeout_releases_access() {
    let (server, c) = fixture().await;
    let s = session();
    assert_eq!(
        s.request("GET", &server.url, None, &[], Instant::now())
            .await
            .unwrap_err()
            .code,
        "TIMEOUT"
    );
    assert_eq!(c.get.load(Ordering::SeqCst), 0);
    let g = s.gate.write().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(10), s.gate.read())
            .await
            .is_err()
    );
    drop(g);
    assert!(s.gate.try_write().is_ok());
}
#[tokio::test]
async fn cancelled_request_releases_active_statistics() {
    let (server, c) = fixture().await;
    let s = Arc::new(service());
    let other = s.clone();
    let url = server.url.clone() + "/slow";
    let task = tokio::spawn(async move { other.fetch(json!({"url":url}), false, false).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while c.active.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    task.abort();
    let _ = task.await;
    assert_eq!(s.status()["active"], 0);
    assert_eq!(s.status()["failed"], 1);
    assert_eq!(s.status()["recent"][0]["code"], "CANCELLED");
}
#[tokio::test]
async fn web_auth_source_address_and_raw_html_contracts() {
    let (upstream, _) = fixture().await;
    let server = serve(web::router(service(), "secret-local".into())).await;
    let client = wreq::Client::builder().no_proxy().build().unwrap();
    let index = client.get(&server.url).send().await.unwrap();
    assert_eq!(index.status(), 200);
    assert!(index.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("script-src 'self'"));
    assert_eq!(
        client
            .get(format!("{}/v1/status", server.url))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{}/v1/status", server.url))
            .bearer_auth("secret-local")
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let source = client
        .get(format!("{}/source.js", server.url))
        .query(&[("base", "https://reader.example:8443/gateway")])
        .bearer_auth("secret-local")
        .send()
        .await
        .unwrap();
    assert_eq!(source.headers()["cache-control"], "no-store");
    let text = source.text().await.unwrap();
    assert!(text.contains("var CF_BASE = \"https://reader.example:8443/gateway\";"));
    assert!(text.contains("var CF_TOKEN = \"secret-local\";"));
    let raw = client
        .get(format!("{}/fetch", server.url))
        .query(&[
            ("url", upstream.url.clone() + "/missing"),
            ("timeoutMs", "1000".into()),
        ])
        .bearer_auth("secret-local")
        .send()
        .await
        .unwrap();
    assert_eq!(raw.status(), 404);
    assert!(raw.headers().contains_key("x-cf-gateway-ms"));
    assert!(raw.text().await.unwrap().contains("实际网站页面"));
    assert!(web::source("https://example.org/#fragment", "").is_err());
}
#[tokio::test]
async fn history_retains_replay_inputs_and_source_token_is_scoped_to_export() {
    let (upstream, _) = fixture().await;
    let server = serve(web::router(service(), "local-only".into())).await;
    let client = wreq::Client::builder().no_proxy().build().unwrap();
    let target = format!("{}/managed?case=history", upstream.url);
    let failed = client
        .post(format!("{}/v1/fetch", server.url))
        .bearer_auth("local-only")
        .json(&json!({"url":target,"session":"reproduce","timeoutMs":4321}))
        .send()
        .await
        .unwrap();
    assert!(!failed.status().is_success());
    assert_eq!(
        client
            .get(format!("{}/v1/history?token=local-only", server.url))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let history: serde_json::Value = client
        .get(format!("{}/v1/history", server.url))
        .bearer_auth("local-only")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let row = &history["items"][0];
    assert_eq!(row["ok"], false);
    assert_eq!(row["url"], target);
    assert_eq!(row["request"]["session"], "reproduce");
    assert_eq!(row["request"]["timeoutMs"], 4321.0);
    assert!(row["id"]
        .as_str()
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok()));
    assert!(!history.to_string().contains("local-only"));
    for site in ["twkan", "69shuba", "banxia"] {
        let source = client
            .get(format!("{}/source.js", server.url))
            .query(&[
                ("site", site),
                ("base", "https://reader.example:8443/gateway"),
                ("token", "local-only"),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(source.status(), 200);
        let script = source.text().await.unwrap();
        assert!(script.contains("java.t2s"));
        assert!(script.contains("function cfDiscovery"));
        assert!(!script.contains("[CF comparison]"));
    }
    assert_eq!(
        client
            .get(format!(
                "{}/source.js?site=unknown&token=local-only",
                server.url
            ))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .get(format!("{}/source.js?token=wrong", server.url))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{}/v1/status?token=local-only", server.url))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{}/logo.png", server.url))
            .send()
            .await
            .unwrap()
            .headers()["content-type"],
        "image/png"
    );
}
#[tokio::test]
async fn image_proxy_preserves_decoded_bytes_scopes_auth_and_records_rejections() {
    let (upstream, c) = fixture().await;
    let server = serve(web::router(service(), "image-secret".into())).await;
    let client = wreq::Client::builder().no_proxy().build().unwrap();
    assert_eq!(
        client
            .get(format!("{}/image", server.url))
            .query(&[("url", upstream.url.clone() + "/cover")])
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    for path in ["/cover", "/cover-gzip", "/cover-shielded"] {
        let response = client
            .get(format!("{}/image", server.url))
            .query(&[
                ("url", upstream.url.clone() + path),
                ("session", format!("image-{path}")),
                ("token", "image-secret".into()),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "image/png");
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.bytes().await.unwrap().as_ref(), pixel().as_slice());
    }
    assert!(
        c.post.load(Ordering::SeqCst) > 0,
        "The image path must also complete an actual verification round trip"
    );
    for path in ["/plain", "/cover-svg", "/missing"] {
        let response = client
            .get(format!("{}/image", server.url))
            .query(&[("url", upstream.url.clone() + path)])
            .bearer_auth("image-secret")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 502);
        let error: serde_json::Value = response.json().await.unwrap();
        assert_eq!(error["code"], "IMAGE_UPSTREAM_ERROR");
    }
    let history: serde_json::Value = client
        .get(format!("{}/v1/history", server.url))
        .bearer_auth("image-secret")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(history["items"][0]["operation"], "image");
    assert_eq!(history["items"][0]["ok"], false);
    assert!(!history.to_string().contains("image-secret"));
    assert_eq!(
        client
            .get(format!("{}/fetch?token=image-secret", server.url))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
}
#[tokio::test]
async fn request_validation_is_local_and_statistics_are_bounded() {
    let s = service();
    for options in [
        json!([]),
        json!({}),
        json!({"url":"file:///test"}),
        json!({"url":"https://example.org/","timeoutMs":true}),
        json!({"url":"https://example.org/","timeoutMs":999}),
        json!({"url":"https://example.org/","session":""}),
        json!({"url":"https://example.org/","proxy":"file:///test"}),
    ] {
        assert!(s.fetch(options, false, false).await.is_err());
    }
    assert_eq!(s.status()["requests"], 0);
    let (server, _) = fixture().await;
    for _ in 0..33 {
        s.fetch(json!({"url":server.url.clone()+"/managed"}), false, false)
            .await
            .unwrap_err();
    }
    assert_eq!(s.status()["recent"].as_array().unwrap().len(), 30);
    assert_eq!(s.status()["active"], 0);
}
#[tokio::test]
async fn integrity_is_one_cookie_free_get_and_preserves_deadline_failures() {
    use cf_gateway::http_tasks::{self, Task};
    let (server, c) = fixture().await;
    let s = Arc::new(session());
    s.cookies.add("login=must-not-send; Path=/", &server.url);
    let make = |url: String| Task {
        id: "test".into(),
        kind: "integrity".into(),
        url,
        timestamp: String::new(),
        digest: String::new(),
    };
    let (_, fields, _, _, record) = http_tasks::run(
        make(server.url.clone() + "/headers"),
        s.clone(),
        "https://challenges.cloudflare.com/frame".into(),
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(fields, json!({"hkqe2":"fetch_error"}));
    assert_eq!(record["status"], 200);
    assert_eq!(c.post.load(Ordering::SeqCst), 0);
    // Inspect the actual request at the local peer, independently of the
    // transport's header construction implementation.
    let seen = Arc::new(std::sync::Mutex::new(None));
    let saved = seen.clone();
    let peer = serve(Router::new().fallback(move |r: Request| {
        let saved = saved.clone();
        async move {
            *saved.lock().unwrap() = Some((r.method().clone(), r.headers().clone()));
            (
                StatusCode::NO_CONTENT,
                [
                    (
                        "access-control-allow-origin",
                        "https://challenges.cloudflare.com",
                    ),
                    ("access-control-expose-headers", "cf-chl-peek"),
                    ("cf-chl-peek", "current-test-token"),
                ],
                "",
            )
        }
    }))
    .await;
    s.cookies.add("login=private; Path=/", &peer.url);
    let (_, fields, _, _, _) = http_tasks::run(
        make(peer.url.clone()),
        s.clone(),
        "https://challenges.cloudflare.com/frame".into(),
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(fields, json!({"hkqe2":"ok"}));
    let (method, headers) = seen.lock().unwrap().clone().unwrap();
    assert_eq!(method, "GET");
    assert!(headers.get("cookie").is_none_or(|v| v.is_empty()));
    assert_eq!(headers["cache-control"], "no-cache");
    assert!(!headers.contains_key("access-control-request-headers"));
    let closed = server.url.clone() + "/invalid-encoding";
    let (_, fields, _, _, _) = http_tasks::run(
        make(closed.clone()),
        s.clone(),
        peer.url.clone(),
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(fields, json!({"hkqe2":"fetch_error"}));
    assert!(
        http_tasks::run(make(closed), s, peer.url.clone(), Instant::now())
            .await
            .is_err()
    );
}
