use super::*;
use axum::{body::Body, extract::Request, routing::any, Router};
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn serve(router: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Server { url, task }
}
async fn page(label: &'static str) -> Server {
    serve(
        Router::new().fallback(any(move |request: Request| async move {
            (
                [
                    ("content-type", "text/html"),
                    ("set-cookie", "reader=shared; Path=/"),
                ],
                format!(
                    "{label}|{}|{}",
                    request.headers().get("host").unwrap().to_str().unwrap(),
                    request.uri().path()
                ),
            )
        })),
    )
    .await
}
fn session() -> Arc<Session> {
    Arc::new(Session::new(Dns::new(&NetworkConfig::default()).unwrap(), None).unwrap())
}
async fn fetch(session: Arc<Session>, url: &str, mapping: Value) -> Result<HttpResponse> {
    with_context(
        RequestContext::new(ConnectTo::parse(Some(&mapping))?),
        session.request(
            "GET",
            url,
            None,
            &[],
            Instant::now() + Duration::from_secs(3),
        ),
    )
    .await
}
#[test]
fn mappings_are_optional_exact_bounded_and_accept_ipv6_destinations() {
    for value in [None, Some(&Value::Null), Some(&json!({}))] {
        assert!(ConnectTo::parse(value).unwrap().is_empty());
    }
    let map = ConnectTo::parse(Some(&json!({"EXAMPLE.org:443":"[::1]:8443"}))).unwrap();
    assert_eq!(
        map.target("example.org", 443),
        Some("[::1]:8443".parse().unwrap())
    );
    assert!(map.target("other.example.org", 443).is_none());
    assert!(map.target("example.org", 80).is_none());
    for value in [
        json!([]),
        json!({"example.org":"127.0.0.1:1"}),
        json!({"*.example.org:443":"127.0.0.1:1"}),
        json!({"example.org:443/path":"127.0.0.1:1"}),
        json!({"example.org:443":"localhost:1"}),
        json!({"example.org:443":"127.0.0.1:0"}),
    ] {
        assert!(ConnectTo::parse(Some(&value)).is_err(), "{value}");
    }
}
#[test]
fn network_namespace_inherits_defaults_and_overrides_legacy_fields_per_option() {
    let global = NetworkConfig {
        mode: NetworkMode::Proxy,
        proxy: Some("http://127.0.0.1:8888".into()),
        doh: true,
        ..Default::default()
    };
    for options in [json!({}), json!({"network":null}), json!({"network":{}})] {
        assert_eq!(global.request(&options).unwrap().0, global);
    }
    let (config, map) = global.request(&json!({"proxy":"socks5://127.0.0.1:9999","connectTo":{"localhost:1":"127.0.0.1:1"},"network":{"proxy":null,"doh":{"enabled":false},"mapping":{"*":"127.0.0.1:2","example.org:443":"[::1]:3"}}})).unwrap();
    assert_eq!(config.mode, NetworkMode::Direct);
    assert_eq!(config.proxy, None);
    assert!(!config.doh);
    assert_eq!(config.doh_url, DOH_URL);
    assert_eq!(
        map.target("example.org", 443),
        Some("[::1]:3".parse().unwrap())
    );
    assert_eq!(
        map.target("example.org", 80),
        Some("127.0.0.1:2".parse().unwrap())
    );
    assert_eq!(
        map.target("other.invalid", 443),
        Some("127.0.0.1:2".parse().unwrap())
    );
    let options = json!({"proxy":"socks5h://127.0.0.1:9999","network":"{\"doh\":{\"url\":\"https://dns.example/dns-query\"}}"});
    let (config, _) = global.request(&options).unwrap();
    assert_eq!(config.proxy.as_deref(), Some("socks5h://127.0.0.1:9999"));
    assert!(config.doh);
    assert_eq!(config.doh_url, "https://dns.example/dns-query");
    for network in [
        json!([]),
        json!("not-json"),
        json!({"unknown":true}),
        json!({"proxy":true}),
        json!({"doh":true}),
        json!({"doh":{"enabled":"true"}}),
        json!({"doh":{"url":"http://remote.example/dns"}}),
        json!({"doh":{"unknown":true}}),
        json!({"mapping":{"*":"example.org:443"}}),
    ] {
        let error = global.request(&json!({"network":network})).unwrap_err();
        assert_eq!(error.status, 400, "{network}");
    }
    assert!(global
        .request(&json!({"connectTo":{"localhost:1":"127.0.0.1:1"},"network":{"mapping":{}}}))
        .unwrap()
        .1
        .is_empty());
}
#[tokio::test]
async fn mapping_overrides_explicit_port_preserves_host_cookie_and_original_session_state() {
    let first = page("A").await;
    let second = page("B").await;
    let session = session();
    session.verified.store(true, Ordering::Release);
    session.generation.store(7, Ordering::Release);
    let target = "http://localhost:1/chapter";
    let mapping = |server: &Server| json!({"localhost:1":server.url.trim_start_matches("http://")});
    let (a, b) = tokio::join!(
        fetch(session.clone(), target, mapping(&first)),
        fetch(session.clone(), target, mapping(&second))
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.text, "A|localhost:1|/chapter");
    assert_eq!(b.text, "B|localhost:1|/chapter");
    assert_eq!(a.url, target);
    assert!(session
        .cookies
        .matches(target)
        .any(|c| c.name() == "reader"));
    assert!(session.verified.load(Ordering::Acquire));
    assert_eq!(session.generation.load(Ordering::Acquire), 7);
    assert!(
        fetch(session, target, json!({})).await.is_err(),
        "A subsequent unmapped request must not reuse a mapped connection"
    );
}
#[tokio::test]
async fn mapping_does_not_create_a_new_logical_service_session() {
    let a = page("A").await;
    let b = page("B").await;
    let service = crate::service::Service::new(crate::engine::Engine::default()).unwrap();
    for server in [&a, &b] {
        let result=service.fetch(json!({"url":"http://localhost:1/chapter","session":"same-reader","connectTo":{"localhost:1":server.url.trim_start_matches("http://")}}),false,false).await.unwrap();
        assert_eq!(result["status"], 200);
    }
    assert_eq!(service.status()["runtime"]["sessions"], 1);
}
pub(super) fn dns_response(packet: &[u8]) -> Vec<u8> {
    let mut response = packet.to_vec();
    response[2] = 0x81;
    response[3] = 0x80;
    response[6] = 0;
    response[7] = 1;
    let kind = u16::from_be_bytes([packet[packet.len() - 4], packet[packet.len() - 3]]);
    response.extend_from_slice(&[0xc0, 0x0c]);
    response.extend_from_slice(&kind.to_be_bytes());
    response.extend_from_slice(&[0, 1, 0, 0, 0, 60]);
    let ip: std::net::IpAddr = if kind == 1 { "127.0.0.1" } else { "::1" }.parse().unwrap();
    match ip {
        std::net::IpAddr::V4(ip) => {
            response.extend_from_slice(&[0, 4]);
            response.extend_from_slice(&ip.octets());
        }
        std::net::IpAddr::V6(ip) => {
            response.extend_from_slice(&[0, 16]);
            response.extend_from_slice(&ip.octets());
        }
    }
    response
}
#[test]
fn dns_wire_validates_questions_compression_and_bounds() {
    for kind in [1, 28] {
        let query = dns_wire::query("test.example", kind).unwrap();
        let response = dns_response(&query);
        assert_eq!(
            dns_wire::answer(&response, "test.example", kind)
                .unwrap()
                .0
                .len(),
            1
        );
        assert!(dns_wire::answer(&response, "different.example", kind).is_err());
        for length in 0..response.len() {
            assert!(dns_wire::answer(&response[..length], "test.example", kind).is_err());
        }
    }
    let mut bad = dns_response(&dns_wire::query("test.example", 1).unwrap());
    bad[12] = 0xc0;
    bad[13] = 12;
    assert!(dns_wire::answer(&bad, "test.example", 1).is_err());
}
async fn doh_server(fail: bool, calls: Arc<AtomicUsize>) -> Server {
    use base64::Engine;
    serve(Router::new().fallback(any(move |request: Request| {
        let calls = calls.clone();
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            if fail {
                return axum::http::Response::builder()
                    .status(503)
                    .body(Body::empty())
                    .unwrap();
            }
            let query = url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
                .find(|(key, _)| key == "dns")
                .unwrap()
                .1
                .into_owned();
            let packet = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(query)
                .unwrap();
            axum::http::Response::builder()
                .header("content-type", "application/dns-message")
                .body(Body::from(dns_response(&packet)))
                .unwrap()
        }
    })))
    .await
}
#[tokio::test]
async fn doh_uses_configured_endpoint_singleflight_and_never_falls_back() {
    let calls = Arc::new(AtomicUsize::new(0));
    let server = doh_server(false, calls.clone()).await;
    let config = NetworkConfig {
        doh: true,
        doh_url: format!("{}/dns-query", server.url),
        ..Default::default()
    };
    let dns = Dns::new(&config).unwrap();
    let (a, b) = tokio::join!(dns.lookup("test.example"), dns.lookup("test.example"));
    assert_eq!(a.unwrap().len(), 2);
    assert_eq!(b.unwrap().len(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    dns.lookup("test.example").await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let broken = doh_server(true, calls.clone()).await;
    let dns = Dns::new(&NetworkConfig {
        doh: true,
        doh_url: broken.url.clone(),
        ..Default::default()
    })
    .unwrap();
    assert!(dns.lookup("localhost").await.is_err());
    assert!(dns.lookup("localhost").await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    let system = Dns::new(&NetworkConfig {
        doh: false,
        doh_url: broken.url.clone(),
        ..Default::default()
    })
    .unwrap();
    assert!(!system.lookup("localhost").await.unwrap().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 6);
}
#[tokio::test]
async fn dns_is_inside_the_total_request_deadline() {
    let server = serve(Router::new().fallback(any(|| async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        "slow"
    })))
    .await;
    let dns = Dns::new(&NetworkConfig {
        doh: true,
        doh_url: server.url.clone(),
        ..Default::default()
    })
    .unwrap();
    let session = Session::new(dns, None).unwrap();
    let started = Instant::now();
    let error = session
        .request(
            "GET",
            "http://test.example/",
            None,
            &[],
            started + Duration::from_millis(50),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "TIMEOUT");
    assert!(started.elapsed() < Duration::from_secs(1));
}
#[tokio::test]
async fn failed_proxy_does_not_connect_directly_to_a_reachable_target() {
    let target = page("reachable").await;
    let config = NetworkConfig {
        mode: NetworkMode::Proxy,
        proxy: Some("http://127.0.0.1:1".into()),
        ..Default::default()
    };
    let session = Session::new(Dns::new(&config).unwrap(), config.proxy.clone()).unwrap();
    assert!(session
        .request(
            "GET",
            &target.url,
            None,
            &[],
            Instant::now() + Duration::from_secs(2)
        )
        .await
        .is_err());
}

async fn proxy_fixture(socks: bool) -> (Server, Arc<Mutex<Vec<String>>>) {
    use base64::Engine;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let records = seen.clone();
    let task = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                connection=listener.accept()=>{
                    let (mut client,_)=connection.unwrap();let records=records.clone();
                    tasks.spawn(async move {
                        let destination=if socks {
                            assert_eq!(client.read_u8().await.unwrap(),5);let count=client.read_u8().await.unwrap();let mut methods=vec![0;count as usize];client.read_exact(&mut methods).await.unwrap();client.write_all(&[5,0]).await.unwrap();
                            let mut header=[0;4];client.read_exact(&mut header).await.unwrap();assert_eq!(&header[..3],&[5,1,0]);
                            let host=match header[3]{1=>{let mut bytes=[0;4];client.read_exact(&mut bytes).await.unwrap();std::net::Ipv4Addr::from(bytes).to_string()},4=>{let mut bytes=[0;16];client.read_exact(&mut bytes).await.unwrap();std::net::Ipv6Addr::from(bytes).to_string()},3=>{let n=client.read_u8().await.unwrap();let mut bytes=vec![0;n as usize];client.read_exact(&mut bytes).await.unwrap();String::from_utf8(bytes).unwrap()},_=>panic!("address")};
                            let port=client.read_u16().await.unwrap();let destination=config::authority(&host,port);records.lock().unwrap().push(destination.clone());destination
                        } else {
                            let mut bytes=Vec::new();while !bytes.ends_with(b"\r\n\r\n"){bytes.push(client.read_u8().await.unwrap());assert!(bytes.len()<16384);}
                            let header=String::from_utf8(bytes).unwrap();let mut first=header.lines().next().unwrap().split_whitespace();let method=first.next().unwrap();let destination=first.next().unwrap().to_owned();
                            if method!="CONNECT" {
                                let request=url::Url::parse(&destination).unwrap();let encoded=request.query_pairs().find(|(key,_)|key=="dns").unwrap().1.into_owned();
                                records.lock().unwrap().push("DOH".into());let packet=base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded).unwrap();let response=dns_response(&packet);
                                client.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/dns-message\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",response.len()).as_bytes()).await.unwrap();client.write_all(&response).await.unwrap();return;
                            }
                            records.lock().unwrap().push(destination.clone());
                            if let Some(auth)=header.lines().find(|line|line.to_ascii_lowercase().starts_with("proxy-authorization:")){records.lock().unwrap().push(auth.to_owned());}
                            destination
                        };
                        let port=destination.rsplit_once(':').unwrap().1.parse::<u16>().unwrap();
                        let mut remote=match tokio::net::TcpStream::connect(("127.0.0.1",port)).await{Ok(stream)=>stream,Err(_)=>return};
                        if socks{client.write_all(&[5,0,0,1,127,0,0,1,0,0]).await.unwrap();}else{client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await.unwrap();}
                        let _=tokio::io::copy_bidirectional(&mut client,&mut remote).await;
                    });
                }
                _=tasks.join_next(),if !tasks.is_empty()=>{}
            }
        }
    });
    (
        Server {
            url: format!("{}://{address}", if socks { "socks5" } else { "http" }),
            task,
        },
        seen,
    )
}

#[tokio::test]
async fn http_and_socks_proxy_resolve_unmapped_names_remotely_and_apply_explicit_ip_ports() {
    for socks in [false, true] {
        let (proxy, seen) = proxy_fixture(socks).await;
        let target = page("proxied").await;
        let port = url::Url::parse(&target.url).unwrap().port().unwrap();
        let config = NetworkConfig {
            mode: NetworkMode::Proxy,
            proxy: Some(proxy.url.clone()),
            ..Default::default()
        };
        let session =
            Arc::new(Session::new(Dns::new(&config).unwrap(), config.proxy.clone()).unwrap());
        let url = format!("http://unresolvable.invalid:{port}/chapter");
        assert!(fetch(session.clone(), &url, json!({}))
            .await
            .unwrap()
            .text
            .starts_with("proxied|unresolvable.invalid:"));
        assert_eq!(
            seen.lock().unwrap()[0],
            format!("unresolvable.invalid:{port}")
        );
        let mapping = json!({format!("unresolvable.invalid:{port}"):format!("203.0.113.4:{port}")});
        fetch(session, &url, mapping).await.unwrap();
        assert_eq!(seen.lock().unwrap()[1], format!("203.0.113.4:{port}"));
    }
}

#[tokio::test]
async fn proxy_doh_queries_use_same_proxy_and_target_tunnel_uses_resolved_ip() {
    for socks in [false, true] {
        let (proxy, seen) = proxy_fixture(socks).await;
        let calls = Arc::new(AtomicUsize::new(0));
        let doh = doh_server(false, calls.clone()).await;
        let target = page("resolved").await;
        let port = url::Url::parse(&target.url).unwrap().port().unwrap();
        let config = NetworkConfig {
            mode: NetworkMode::Proxy,
            proxy: Some(proxy.url.clone()),
            doh: true,
            doh_url: format!("{}/dns-query", doh.url),
        };
        let session =
            Arc::new(Session::new(Dns::new(&config).unwrap(), config.proxy.clone()).unwrap());
        let result = fetch(
            session,
            &format!("http://unresolvable.invalid:{port}/"),
            json!({}),
        )
        .await
        .unwrap();
        assert!(result.text.starts_with("resolved|unresolvable.invalid:"));
        let records = seen.lock().unwrap();
        if socks {
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        } else {
            assert_eq!(records.iter().filter(|s| *s == "DOH").count(), 2);
        }
        assert!(records
            .iter()
            .any(|s| s == &format!("[::1]:{port}") || s == &format!("127.0.0.1:{port}")));
        assert!(!records
            .iter()
            .any(|s| s.starts_with("unresolvable.invalid:")));
    }
}

#[tokio::test]
async fn proxy_credentials_are_decoded_for_connect_and_never_become_website_headers() {
    let (proxy, seen) = proxy_fixture(false).await;
    let target = serve(Router::new().fallback(any(|request: Request| async move {
        assert!(!request.headers().contains_key("proxy-authorization"));
        "auth"
    })))
    .await;
    let port = url::Url::parse(&target.url).unwrap().port().unwrap();
    let proxy = proxy
        .url
        .replacen("http://", "http://user%40example:pass%3Aword@", 1);
    let config = NetworkConfig {
        mode: NetworkMode::Proxy,
        proxy: Some(proxy),
        ..Default::default()
    };
    let session = Arc::new(Session::new(Dns::new(&config).unwrap(), config.proxy.clone()).unwrap());
    fetch(
        session,
        &format!("http://unresolvable.invalid:{port}/"),
        json!({}),
    )
    .await
    .unwrap();
    assert!(seen
        .lock()
        .unwrap()
        .iter()
        .any(|s| s == "Proxy-Authorization: Basic dXNlckBleGFtcGxlOnBhc3M6d29yZA=="));
}

#[tokio::test]
async fn mapped_context_is_inherited_by_spawned_verification_work() {
    let target = page("child").await;
    let session = session();
    let mapping =
        ConnectTo::parse(Some(&json!({"*":target.url.trim_start_matches("http://")}))).unwrap();
    let result = with_context(RequestContext::new(mapping), async move {
        tokio::spawn(inherit_context(async move {
            session
                .request(
                    "GET",
                    "http://localhost:1/",
                    None,
                    &[],
                    Instant::now() + Duration::from_secs(2),
                )
                .await
        }))
        .await
        .unwrap()
    })
    .await
    .unwrap();
    assert!(result.text.starts_with("child|localhost:1"));
}

#[tokio::test]
async fn wildcard_mapping_follows_new_domains_and_precise_rules_win_without_dns() {
    let calls = Arc::new(AtomicUsize::new(0));
    let doh = doh_server(true, calls.clone()).await;
    let destination = page("wildcard").await;
    let origin = serve(Router::new().fallback(any(|| async {
        (
            axum::http::StatusCode::FOUND,
            [("location", "http://new-domain.invalid:2/chapter")],
            "",
        )
    })))
    .await;
    let config = NetworkConfig {
        doh: true,
        doh_url: doh.url.clone(),
        ..Default::default()
    };
    let session = Arc::new(Session::new(Dns::new(&config).unwrap(), None).unwrap());
    let result=fetch(session,"http://initial.invalid:1/",json!({"initial.invalid:1":origin.url.trim_start_matches("http://"),"*":destination.url.trim_start_matches("http://")})).await.unwrap();
    assert_eq!(result.text, "wildcard|new-domain.invalid:2|/chapter");
    assert_eq!(result.url, "http://new-domain.invalid:2/chapter");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn request_network_overrides_are_ephemeral_and_keep_existing_session_keys() {
    let calls = Arc::new(AtomicUsize::new(0));
    let doh = doh_server(false, calls.clone()).await;
    let target = page("request").await;
    let port = url::Url::parse(&target.url).unwrap().port().unwrap();
    let service = crate::service::Service::new(crate::engine::Engine::default()).unwrap();
    let url = format!("http://unresolvable.invalid:{port}/");
    let first=service.fetch(json!({"url":url,"session":"same","network":{"doh":{"enabled":true,"url":format!("{}/dns-query",doh.url)}}}),false,false).await.unwrap();
    assert_eq!(first["status"], 200);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    // Same session, concurrent different DNS settings: the disabled request must
    // use its mapping rather than the first request's cached DNS/transport.
    let (mapped, resolved)=tokio::join!(service.fetch(json!({"url":url,"session":"same","network":{"doh":{"enabled":false},"mapping":{"*":target.url.trim_start_matches("http://")}}}),false,false),service.fetch(json!({"url":url,"session":"same","network":{"doh":{"enabled":true,"url":doh.url}}}),false,false));
    assert!(mapped.is_ok());
    assert!(resolved.is_ok());
    assert_eq!(service.status()["runtime"]["sessions"], 1);
    assert_eq!(service.status()["runtime"]["dnsMode"], "system");
    // A later legacy request does not retain request overrides.
    assert!(service
        .fetch(
            json!({"url":url,"session":"same","timeoutMs":1000}),
            false,
            false
        )
        .await
        .is_err());
    let history = service.history();
    assert_eq!(history["items"][0]["dnsMode"], "system");
    assert!(history["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |r| r["request"]["network"]["mapping"]["*"] == target.url.trim_start_matches("http://")
        ));
}

#[tokio::test]
async fn nested_proxy_doh_and_mapping_use_the_selected_proxy_and_exclude_infrastructure() {
    for socks in [false, true] {
        let (proxy, seen) = proxy_fixture(socks).await;
        let calls = Arc::new(AtomicUsize::new(0));
        let doh = doh_server(false, calls.clone()).await;
        let target = page("nested").await;
        let port = url::Url::parse(&target.url).unwrap().port().unwrap();
        let service = crate::service::Service::new(crate::engine::Engine::default()).unwrap();
        let url = format!("http://unresolvable.invalid:{port}/");
        let result=service.fetch(json!({"url":url,"session":"nested","network":{"proxy":proxy.url,"doh":{"enabled":true,"url":doh.url}}}),false,false).await.unwrap();
        assert_eq!(result["status"], 200);
        let result=service.fetch(json!({"url":url,"session":"nested","network":{"proxy":proxy.url,"doh":{"enabled":true,"url":doh.url},"mapping":{"*":format!("203.0.113.8:{port}")}}}),false,false).await.unwrap();
        assert_eq!(result["status"], 200);
        assert!(seen
            .lock()
            .unwrap()
            .iter()
            .any(|v| v == &format!("203.0.113.8:{port}")));
        assert_eq!(service.status()["runtime"]["sessions"], 1);
        // Explicit null bypasses an inherited application proxy.
        let global = NetworkConfig {
            mode: NetworkMode::Proxy,
            proxy: Some(proxy.url.clone()),
            doh: true,
            doh_url: doh.url.clone(),
        };
        let service =
            crate::service::Service::with_network(crate::engine::Engine::default(), global)
                .unwrap();
        assert!(service
            .fetch(
                json!({"url":target.url,"network":{"proxy":null,"doh":{"enabled":false}}}),
                false,
                false
            )
            .await
            .is_ok());
        assert_eq!(service.history()["items"][0]["networkMode"], "direct");
        assert_eq!(service.history()["items"][0]["dnsMode"], "system");
        assert_eq!(service.status()["runtime"]["networkMode"], "proxy");
    }
}

#[tokio::test]
async fn legacy_proxy_still_works_and_history_never_contains_proxy_credentials() {
    let (proxy, _) = proxy_fixture(false).await;
    let target = page("legacy").await;
    let port = url::Url::parse(&target.url).unwrap().port().unwrap();
    let service = crate::service::Service::new(crate::engine::Engine::default()).unwrap();
    let auth = proxy
        .url
        .replacen("http://", "http://private-user:private-pass@", 1);
    for options in [
        json!({"proxy":proxy.url}),
        json!({"network":{"proxy":auth}}),
    ] {
        let mut options = options;
        options["url"] = json!(format!("http://unresolvable.invalid:{port}/"));
        assert!(service.fetch(options, false, false).await.is_ok());
    }
    let history = service.history();
    assert_eq!(history["items"][0]["proxyCredentialsOmitted"], true);
    assert!(!history.to_string().contains("private-user"));
    assert!(!history.to_string().contains("private-pass"));
    assert!(history["items"][0]["request"]["network"]
        .get("proxy")
        .is_none());
}

#[tokio::test]
async fn redirects_keep_request_mappings_exact_and_do_not_leak_to_other_ports() {
    let destination = page("redirected").await;
    let origin = serve(Router::new().fallback(any(|| async {
        (
            axum::http::StatusCode::FOUND,
            [("location", "http://localhost:2/chapter")],
            "",
        )
    })))
    .await;
    let session = session();
    let response=fetch(session.clone(),"http://localhost:1/",json!({"localhost:1":origin.url.trim_start_matches("http://"),"localhost:2":destination.url.trim_start_matches("http://")})).await.unwrap();
    assert_eq!(response.url, "http://localhost:2/chapter");
    assert_eq!(response.text, "redirected|localhost:2|/chapter");
    assert!(fetch(
        session,
        "http://localhost:1/",
        json!({"localhost:1":origin.url.trim_start_matches("http://")})
    )
    .await
    .is_err());
}

#[tokio::test]
async fn explicit_mapping_skips_doh_and_never_retries_a_different_address() {
    let calls = Arc::new(AtomicUsize::new(0));
    let doh = doh_server(true, calls.clone()).await;
    let target = page("mapped").await;
    let config = NetworkConfig {
        doh: true,
        doh_url: doh.url.clone(),
        ..Default::default()
    };
    let session = Arc::new(Session::new(Dns::new(&config).unwrap(), None).unwrap());
    let value = fetch(
        session.clone(),
        "http://localhost:1/",
        json!({"localhost:1":target.url.trim_start_matches("http://")}),
    )
    .await
    .unwrap();
    assert!(value.text.starts_with("mapped|localhost:1"));
    assert!(fetch(
        session,
        "http://localhost:1/",
        json!({"localhost:1":"127.0.0.1:2"})
    )
    .await
    .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn mapped_https_keeps_original_sni_and_rejects_untrusted_certificates() {
    use btls::{
        asn1::Asn1Time,
        hash::MessageDigest,
        pkey::PKey,
        rsa::Rsa,
        ssl::{NameType, SslAcceptor, SslMethod},
        x509::{X509NameBuilder, X509},
    };
    let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "target.example").unwrap();
    let name = name.build();
    let mut certificate = X509::builder().unwrap();
    certificate.set_version(2).unwrap();
    certificate.set_subject_name(&name).unwrap();
    certificate.set_issuer_name(&name).unwrap();
    certificate.set_pubkey(&key).unwrap();
    certificate
        .set_not_before(Asn1Time::days_from_now(0).unwrap().as_ref())
        .unwrap();
    certificate
        .set_not_after(Asn1Time::days_from_now(1).unwrap().as_ref())
        .unwrap();
    certificate.sign(&key, MessageDigest::sha256()).unwrap();
    let mut acceptor = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls()).unwrap();
    acceptor.set_certificate(&certificate.build()).unwrap();
    acceptor.set_private_key(&key).unwrap();
    let seen = Arc::new(Mutex::new(None));
    let received = seen.clone();
    acceptor.set_servername_callback(move |ssl, _| {
        *received.lock().unwrap() = ssl.servername(NameType::HOST_NAME).map(String::from);
        Ok(())
    });
    let acceptor = acceptor.build();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let destination = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let ssl = btls::ssl::Ssl::new(acceptor.context()).unwrap();
        let mut stream = tokio_btls::SslStream::new(ssl, tcp).unwrap();
        let _ = std::pin::Pin::new(&mut stream).accept().await;
    });
    let result = fetch(
        session(),
        "https://target.example:4443/",
        json!({"target.example:4443":destination.to_string()}),
    )
    .await;
    assert!(result.is_err());
    task.await.unwrap();
    assert_eq!(seen.lock().unwrap().as_deref(), Some("target.example"));
}
