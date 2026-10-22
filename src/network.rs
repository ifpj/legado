use crate::{re, require, static_data::validate_url, Error, Result};
use futures_util::StreamExt;
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;
use wreq::{
    cookie::Jar,
    dns::{Addrs, Name, Resolve, Resolving},
    header::{HeaderMap, HeaderName, HeaderValue},
    Client, Method,
};
use wreq_util::{Emulation, Platform, Profile};
pub const USER_AGENT:&str="Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36";
pub const IDENTITY_HEADERS: [(&str, &str); 6] = [
    ("user-agent", USER_AGENT),
    ("accept-encoding", "gzip, deflate, br, zstd"),
    ("accept-language", "zh-CN,zh;q=0.9,en;q=0.8"),
    (
        "sec-ch-ua",
        r#""Chromium";v="149", "Google Chrome";v="149", "Not_A Brand";v="99""#,
    ),
    ("sec-ch-ua-mobile", "?0"),
    ("sec-ch-ua-platform", r#""Windows""#),
];
pub const EDGE_IPS: [&str; 5] = [
    "104.16.184.241",
    "104.17.136.61",
    "104.17.245.203",
    "104.17.249.203",
    "172.64.146.157",
];
pub const DOH_URL: &str = "https://cloudflare-dns.com/dns-query";
fn error(e: impl std::fmt::Display) -> Error {
    Error::new("UPSTREAM_ERROR", e.to_string())
}
fn http_error(e: wreq::Error) -> Error {
    let timeout = e.is_timeout();
    let mut error = Error::new(
        if timeout { "TIMEOUT" } else { "UPSTREAM_ERROR" },
        if timeout {
            "网站请求超时"
        } else {
            "网站连接失败"
        },
    );
    error.status = if timeout { 504 } else { 502 };
    error
}
fn profile() -> Emulation {
    Emulation::builder()
        .profile(Profile::Chrome149)
        .platform(Platform::Windows)
        .headers(false)
        .build()
}
#[derive(Clone)]
struct DnsRecord {
    addresses: Vec<SocketAddr>,
    expires: Instant,
    cf: bool,
}
#[derive(Clone)]
pub struct Dns {
    client: Client,
    entries: Arc<Mutex<HashMap<String, DnsRecord>>>,
    refused: Arc<Mutex<HashMap<String, Instant>>>,
    mapping: bool,
    lookup_url: Arc<str>,
    pending: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    mapped_hosts: Arc<Mutex<HashSet<String>>>,
}
impl Dns {
    pub fn new(mapping: bool) -> Result<Self> {
        let client = Client::builder()
            .emulation(profile())
            .no_proxy()
            .retry(wreq::retry::Policy::never())
            .resolve_to_addrs(
                "cloudflare-dns.com",
                EDGE_IPS
                    .iter()
                    .map(|ip| SocketAddr::new(ip.parse().unwrap(), 443)),
            )
            .build()
            .map_err(error)?;
        Ok(Self {
            client,
            entries: Arc::new(Mutex::new(HashMap::new())),
            refused: Arc::new(Mutex::new(HashMap::new())),
            mapping,
            lookup_url: DOH_URL.into(),
            pending: Arc::default(),
            mapped_hosts: Arc::default(),
        })
    }
    pub fn mark_refused(&self, host: &str) {
        let expires = self
            .entries
            .lock()
            .unwrap()
            .get(host)
            .map_or(Instant::now() + Duration::from_secs(30), |e| e.expires);
        let mut refused = self.refused.lock().unwrap();
        refused.retain(|_, v| *v > Instant::now());
        if refused.len() < 256 {
            refused.insert(host.into(), expires);
        }
    }
    pub fn ordinary(&self) -> Self {
        Self {
            mapping: false,
            ..self.clone()
        }
    }
    async fn lookup(
        &self,
        host: &str,
    ) -> std::result::Result<Vec<SocketAddr>, Box<dyn std::error::Error + Send + Sync>> {
        let normalized = host.trim_end_matches('.').to_ascii_lowercase();
        let host = normalized.as_str();
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(vec![SocketAddr::new(ip, 0)]);
        }
        if self.mapping
            && known_cf_host(host)
            && !self
                .refused
                .lock()
                .unwrap()
                .get(host)
                .is_some_and(|v| *v > Instant::now())
        {
            self.mapped_hosts.lock().unwrap().insert(host.to_string());
            return Ok(EDGE_IPS
                .iter()
                .map(|ip| SocketAddr::new(ip.parse().unwrap(), 0))
                .collect());
        }
        let gate = {
            let mut pending = self.pending.lock().unwrap();
            pending.retain(|_, g| Arc::strong_count(g) > 1);
            pending.entry(host.to_string()).or_default().clone()
        };
        let _lookup = gate.lock().await;
        let now = Instant::now();
        let cached = self
            .entries
            .lock()
            .unwrap()
            .get(host)
            .filter(|v| v.expires > now)
            .cloned();
        let record = if let Some(r) = cached {
            r
        } else {
            let query = |kind: &'static str| {
                let client = self.client.clone();
                let host = host.to_string();
                let endpoint = self.lookup_url.clone();
                async move {
                    client
                        .get(endpoint.as_ref())
                        .query(&[("name", host.as_str()), ("type", kind)])
                        .header("accept", "application/dns-json")
                        .timeout(Duration::from_secs(3))
                        .send()
                        .await?
                        .json::<Value>()
                        .await
                }
            };
            let (a, aaaa) = tokio::join!(query("A"), query("AAAA"));
            let mut ips = Vec::new();
            let mut ipv4 = Vec::new();
            let mut ttl = 300u64;
            let mut cf = false;
            for answer in [aaaa, a].into_iter().flatten() {
                if answer["Status"] != 0 {
                    continue;
                }
                let valid_question = answer["Question"].as_array().is_some_and(|q| {
                    q.iter().any(|q| {
                        matches!(q["type"].as_u64(), Some(1 | 28))
                            && q["name"]
                                .as_str()
                                .is_some_and(|n| n.trim_end_matches('.').eq_ignore_ascii_case(host))
                    })
                });
                if !valid_question {
                    continue;
                }
                if cloudflare_answer(&answer, host).0 {
                    cf = true;
                }
                for item in answer["Answer"].as_array().into_iter().flatten() {
                    ttl = ttl.min(item["TTL"].as_u64().unwrap_or(30).min(300));
                    if matches!(item["type"].as_u64(), Some(1 | 28)) {
                        if let Some(ip) =
                            item["data"].as_str().and_then(|v| v.parse::<IpAddr>().ok())
                        {
                            if ip.is_ipv4() {
                                ipv4.push(ip);
                            }
                            ips.push(SocketAddr::new(ip, 0));
                            ttl = ttl.min(item["TTL"].as_u64().unwrap_or(30));
                        }
                    }
                }
            }
            if ips.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "DoH returned no addresses",
                )));
            }
            let cf = cf && !ipv4.is_empty() && ipv4.iter().all(|ip| is_cf(*ip));
            let record = DnsRecord {
                addresses: ips,
                expires: now + Duration::from_secs(ttl.min(300)),
                cf,
            };
            let mut entries = self.entries.lock().unwrap();
            entries.retain(|_, v| v.expires > now);
            if entries.len() < 256 {
                entries.insert(host.into(), record.clone());
            }
            record
        };
        let refused = self
            .refused
            .lock()
            .unwrap()
            .get(host)
            .is_some_and(|expiry| *expiry > now);
        if self.mapping
            && !refused
            && (record.cf
                || host == "challenges.cloudflare.com"
                || host.ends_with(".challenges.cloudflare.com"))
        {
            self.mapped_hosts.lock().unwrap().insert(host.to_string());
            return Ok(EDGE_IPS
                .iter()
                .map(|ip| SocketAddr::new(ip.parse().unwrap(), 0))
                .collect());
        }
        Ok(record.addresses)
    }
}
fn known_cf_host(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    host == "challenges.cloudflare.com" || host.ends_with(".challenges.cloudflare.com")
}
pub(crate) fn cloudflare_answer(payload: &Value, host: &str) -> (bool, u64) {
    let invalid = (false, 30);
    if payload["Status"] != 0
        || !payload["Question"].as_array().is_some_and(|q| {
            q.iter().any(|q| {
                q["type"] == 1
                    && q["name"]
                        .as_str()
                        .is_some_and(|n| n.trim_end_matches('.').eq_ignore_ascii_case(host))
            })
        })
    {
        return invalid;
    }
    let Some(answers) = payload["Answer"].as_array().filter(|a| a.len() <= 128) else {
        return invalid;
    };
    let (mut count, mut all, mut ttl) = (0, true, 300);
    for answer in answers {
        if !answer.is_object() {
            return invalid;
        }
        if answer["type"] == 1 {
            let Some(ip) = answer["data"].as_str().and_then(|s| s.parse().ok()) else {
                return invalid;
            };
            all &= is_cf(ip);
            count += 1;
        }
        let value = answer
            .get("TTL")
            .map(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|v| v.parse().ok()))
            })
            .unwrap_or(Some(30));
        let Some(value) = value else { return invalid };
        ttl = ttl.min(value.clamp(0, 300) as u64);
    }
    (count > 0 && all, if answers.is_empty() { 30 } else { ttl })
}
fn is_cf(ip: IpAddr) -> bool {
    let IpAddr::V4(ip) = ip else { return false };
    let n = u32::from(ip);
    [
        ("173.245.48.0", 20),
        ("103.21.244.0", 22),
        ("103.22.200.0", 22),
        ("103.31.4.0", 22),
        ("141.101.64.0", 18),
        ("108.162.192.0", 18),
        ("190.93.240.0", 20),
        ("188.114.96.0", 20),
        ("197.234.240.0", 22),
        ("198.41.128.0", 17),
        ("162.158.0.0", 15),
        ("104.16.0.0", 13),
        ("104.24.0.0", 14),
        ("172.64.0.0", 13),
        ("131.0.72.0", 22),
    ]
    .into_iter()
    .any(|(ip, prefix)| {
        let mask = u32::MAX << (32 - prefix);
        n & mask == u32::from(ip.parse::<std::net::Ipv4Addr>().unwrap()) & mask
    })
}
impl Resolve for Dns {
    fn resolve(&self, name: Name) -> Resolving {
        let dns = self.clone();
        Box::pin(async move {
            let addresses = dns.lookup(name.as_str()).await?;
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}
pub struct Session {
    pub client: RwLock<Client>,
    pub cookies: Arc<Jar>,
    pub dns: Dns,
    pub proxy: Option<String>,
    pub gate: RwLock<()>,
    pub reads: tokio::sync::Semaphore,
    pub verified: std::sync::atomic::AtomicBool,
    pub generation: std::sync::atomic::AtomicU64,
    fallback_used: std::sync::atomic::AtomicBool,
    ordinary: tokio::sync::OnceCell<Client>,
    origins: Mutex<HashSet<String>>,
}
impl Session {
    pub fn new(mut dns: Dns, proxy: Option<String>) -> Result<Self> {
        dns.mapped_hosts = Arc::default();
        let cookies = Arc::new(Jar::default());
        let client = client(dns.clone(), cookies.clone(), proxy.as_deref())?;
        Ok(Self {
            client: RwLock::new(client),
            cookies,
            dns,
            proxy,
            gate: RwLock::new(()),
            reads: tokio::sync::Semaphore::new(4),
            verified: false.into(),
            generation: 0.into(),
            fallback_used: false.into(),
            ordinary: tokio::sync::OnceCell::new(),
            origins: Mutex::default(),
        })
    }
    pub fn used_mapping(&self, url: &str) -> bool {
        if self.proxy.is_some()
            || self
                .fallback_used
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return false;
        }
        validate_url(url).ok().is_some_and(|url| {
            url.scheme() == "https"
                && url.port_or_known_default() == Some(443)
                && url
                    .host_str()
                    .is_some_and(|h| self.dns.mapped_hosts.lock().unwrap().contains(h))
        })
    }
    pub async fn fallback(&self, url: &str) -> Result<()> {
        let host = validate_url(url)?.host_str().unwrap().to_string();
        self.dns.mark_refused(&host);
        self.origins
            .lock()
            .unwrap()
            .insert(crate::static_data::origin(url)?);
        let origins = self.origins.lock().unwrap().clone();
        for cookie in self
            .cookies
            .get_all()
            .filter(|c| matches!(c.name(), "cf_clearance" | "__cf_bm"))
        {
            let path = cookie.path().unwrap_or("/");
            if let Some(domain) = cookie.domain() {
                self.cookies
                    .remove(cookie.clone(), format!("https://{domain}{path}"));
            } else {
                for origin in &origins {
                    self.cookies
                        .remove(cookie.clone(), format!("{origin}{path}"));
                }
            }
        }
        let replacement = client(
            self.dns.ordinary(),
            self.cookies.clone(),
            self.proxy.as_deref(),
        )?;
        *self.client.write().await = replacement;
        self.fallback_used
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }
    pub async fn request(
        &self,
        method: &str,
        url: &str,
        body: Option<Vec<u8>>,
        headers: &[(&str, String)],
        deadline: Instant,
    ) -> Result<HttpResponse> {
        let mut url = validate_url(url)?;
        let mut method = method.to_string();
        let mut body = body;
        let mut supplied = request_headers(&method, headers)?;
        let mut visited = HashSet::new();
        for redirects in 0..=10 {
            if !visited.insert(url.to_string()) {
                return Err(Error::new("UPSTREAM_ERROR", "网站重定向循环"));
            }
            let supplied_headers = supplied
                .iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect::<Vec<_>>();
            let response = self
                .request_once(
                    &method,
                    url.as_str(),
                    body.clone(),
                    &supplied_headers,
                    deadline,
                )
                .await?;
            if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
                return Ok(response);
            }
            let Some(location) = response.headers.get("location").and_then(Value::as_str) else {
                return Ok(response);
            };
            if redirects == 10 {
                return Err(Error::new("UPSTREAM_ERROR", "网站重定向次数超过限制"));
            }
            require(
                !location.chars().any(|c| c < ' '),
                "Invalid redirect location",
            )?;
            let next = url
                .join(location)
                .map_err(|_| Error::new("UPSTREAM_ERROR", "网站重定向地址无效"))?;
            let next = validate_url(next.as_str())
                .map_err(|_| Error::new("UPSTREAM_ERROR", "网站重定向地址无效"))?;
            if next.origin() != url.origin() {
                supplied.shift_remove("authorization");
                supplied.shift_remove("proxy-authorization");
                supplied.insert("sec-fetch-site".into(), "cross-site".into());
            }
            if url.scheme() == "https" && next.scheme() == "http" {
                supplied.shift_remove("referer");
            } else {
                supplied.insert("referer".into(), url.to_string());
            }
            if response.status == 303 && method != "HEAD"
                || matches!(response.status, 301 | 302) && method == "POST"
            {
                method = "GET".into();
                body = None;
                for name in ["content-type", "content-length", "origin"] {
                    supplied.shift_remove(name);
                }
            }
            url = next;
        }
        unreachable!()
    }
    async fn request_once(
        &self,
        method: &str,
        url: &str,
        body: Option<Vec<u8>>,
        headers: &[(&str, String)],
        deadline: Instant,
    ) -> Result<HttpResponse> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let mut e = Error::new("TIMEOUT", "请求总时限已用完");
            e.status = 504;
            return Err(e);
        }
        let supplied = request_headers(method, headers)?;
        let parsed = validate_url(url)?;
        self.origins
            .lock()
            .unwrap()
            .insert(parsed.origin().ascii_serialization());
        let client = if self.proxy.is_some()
            || parsed.scheme() != "https"
            || parsed.port_or_known_default() != Some(443)
        {
            self.ordinary
                .get_or_try_init(|| async {
                    client(
                        self.dns.ordinary(),
                        self.cookies.clone(),
                        self.proxy.as_deref(),
                    )
                })
                .await?
                .clone()
        } else {
            self.client.read().await.clone()
        };
        let mut request = client
            .request(Method::from_bytes(method.as_bytes()).map_err(error)?, url)
            .redirect(wreq::redirect::Policy::none())
            .timeout(remaining);
        for (k, v) in supplied {
            request = request.header(k, v);
        }
        if let Some(body) = body {
            request = request.body(body);
        }
        let response = request.send().await.map_err(http_error)?;
        let status = response.status().as_u16();
        let final_url = response.uri().to_string();
        let mut headers = Map::new();
        for (k, v) in response.headers() {
            if k != "set-cookie" {
                headers.insert(
                    k.to_string(),
                    Value::String(v.to_str().unwrap_or("").into()),
                );
            }
        }
        let mut stream = response.bytes_stream();
        let mut content = Vec::new();
        while let Some(block) = stream.next().await {
            let block = block.map_err(http_error)?;
            if content.len() + block.len() > 8 * 1024 * 1024 {
                return Err(too_large());
            }
            content.extend(&block);
        }
        let download_size = content.len();
        content = decompress(
            content,
            headers
                .get("content-encoding")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )
        .await?;
        let text = decode(&content, &headers);
        Ok(HttpResponse {
            status,
            url: final_url,
            headers,
            content,
            text,
            download_size,
        })
    }
}
pub(crate) fn request_headers(
    method: &str,
    headers: &[(&str, String)],
) -> Result<indexmap::IndexMap<String, String>> {
    let mut supplied = indexmap::IndexMap::new();
    for (k, v) in headers {
        supplied.insert(k.to_lowercase(), v.clone());
    }
    let mode = supplied
        .get("sec-fetch-mode")
        .map(String::as_str)
        .unwrap_or(if method == "GET" && headers.is_empty() {
            "navigate"
        } else {
            "cors"
        });
    let navigation = mode == "navigate";
    let mut common = vec![
        (
            "accept",
            if navigation {
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
            } else {
                "*/*"
            }
            .to_string(),
        ),
        ("sec-fetch-mode", mode.into()),
        (
            "sec-fetch-dest",
            if navigation { "document" } else { "empty" }.into(),
        ),
        (
            "priority",
            if navigation { "u=0, i" } else { "u=1, i" }.into(),
        ),
    ];
    if navigation {
        common.push(("upgrade-insecure-requests", "1".into()));
        if !supplied.contains_key("referer") {
            common.push(("sec-fetch-site", "none".into()));
            common.push(("sec-fetch-user", "?1".into()));
        }
    }
    if method == "POST" {
        if let Some(referer) = supplied.get("referer") {
            common.push(("origin", crate::static_data::origin(referer)?));
        }
    }
    for (k, v) in common {
        supplied.entry(k.into()).or_insert(v);
    }
    Ok(supplied)
}
fn client(dns: Dns, cookies: Arc<Jar>, proxy: Option<&str>) -> Result<Client> {
    let mut headers = HeaderMap::new();
    for (k, v) in IDENTITY_HEADERS {
        headers.insert(
            HeaderName::from_bytes(k.as_bytes()).unwrap(),
            HeaderValue::from_static(v),
        );
    }
    let mut builder = Client::builder()
        .emulation(profile())
        .no_proxy()
        .default_headers(headers)
        .cookie_provider(cookies)
        .gzip(false)
        .deflate(false)
        .brotli(false)
        .zstd(false)
        .dns_resolver(dns)
        .retry(wreq::retry::Policy::never())
        .redirect(wreq::redirect::Policy::limited(10));
    if let Some(proxy) = proxy {
        builder = builder.proxy(wreq::Proxy::all(proxy).map_err(error)?)
    }
    builder.build().map_err(error)
}
#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub url: String,
    pub headers: Map<String, Value>,
    pub content: Vec<u8>,
    pub text: String,
    pub download_size: usize,
}
pub(crate) fn response_encoding(
    bytes: &[u8],
    headers: &Map<String, Value>,
) -> (&'static encoding_rs::Encoding, usize) {
    if let Some(found) = encoding_rs::Encoding::for_bom(bytes) {
        return found;
    }
    let content = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .and_then(|(_, v)| v.as_str())
        .unwrap_or("");
    let declared =
        |text: &str| {
            re(r#"(?i)(?:^|;)\s*charset\s*=\s*(?:"([^"]{1,80})"|'([^']{1,80})'|([^;\s]{1,80}))"#)
                .captures(text)
                .and_then(|c| {
                    c.iter().skip(1).flatten().next().and_then(|v| {
                        encoding_rs::Encoding::for_label(v.as_str().trim().as_bytes())
                    })
                })
        };
    if let Some(e) = declared(content) {
        return (e, 0);
    }
    let html = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    if content.to_ascii_lowercase().contains("html")
        || (content.is_empty() && re(r"(?i)^\s*(?:<!doctype html|<html)").is_match(&html))
    {
        let doc = scraper::Html::parse_document(&html);
        for meta in doc.select(&scraper::Selector::parse("meta").unwrap()) {
            let e = meta
                .value()
                .attr("charset")
                .and_then(|s| encoding_rs::Encoding::for_label(s.as_bytes()))
                .or_else(|| {
                    if meta
                        .value()
                        .attr("http-equiv")
                        .is_some_and(|v| v.eq_ignore_ascii_case("content-type"))
                    {
                        declared(meta.value().attr("content").unwrap_or(""))
                    } else {
                        None
                    }
                });
            if let Some(e) = e {
                return (
                    if e == encoding_rs::UTF_16LE || e == encoding_rs::UTF_16BE {
                        encoding_rs::UTF_8
                    } else {
                        e
                    },
                    0,
                );
            }
        }
    }
    (encoding_rs::UTF_8, 0)
}
pub(crate) fn decode(bytes: &[u8], headers: &Map<String, Value>) -> String {
    let (encoding, skip) = response_encoding(bytes, headers);
    encoding.decode(&bytes[skip..]).0.into_owned()
}

async fn decompress(content: Vec<u8>, encoding: &str) -> Result<Vec<u8>> {
    use async_compression::tokio::bufread::{BrotliDecoder, GzipDecoder, ZlibDecoder, ZstdDecoder};
    let cursor = std::io::Cursor::new(content);
    let decoder: Box<dyn tokio::io::AsyncRead + Unpin + Send> = match encoding.trim() {
        "" | "identity" => return Ok(cursor.into_inner()),
        "gzip" => Box::new(GzipDecoder::new(cursor)),
        "deflate" => Box::new(ZlibDecoder::new(cursor)),
        "br" => Box::new(BrotliDecoder::new(cursor)),
        "zstd" => Box::new(ZstdDecoder::new(cursor)),
        _ => {
            return Err(Error::new(
                "UNSUPPORTED_ENCODING",
                "Unknown HTTP content encoding",
            ))
        }
    };
    let mut output = Vec::new();
    decoder
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut output)
        .await?;
    if output.len() > 8 * 1024 * 1024 {
        return Err(too_large());
    }
    Ok(output)
}

fn too_large() -> Error {
    let mut e = Error::new("RESPONSE_TOO_LARGE", "网站响应超过8MiB");
    e.status = 502;
    e
}

#[cfg(test)]
mod regression {
    use super::*;
    use axum::{
        extract::{Query, State},
        routing::get,
        Json, Router,
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    #[derive(Default)]
    struct ServerState {
        calls: AtomicUsize,
        fail: AtomicBool,
        slow: AtomicBool,
    }
    struct Server {
        task: tokio::task::JoinHandle<()>,
        state: Arc<ServerState>,
        url: String,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    async fn answer(
        State(s): State<Arc<ServerState>>,
        Query(q): Query<HashMap<String, String>>,
    ) -> Json<Value> {
        s.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(if s.slow.load(Ordering::SeqCst) {
            500
        } else {
            20
        }))
        .await;
        let kind = if q["type"] == "A" { 1 } else { 28 };
        Json(
            json!({"Status":if s.fail.load(Ordering::SeqCst){2}else{0},"Question":[{"name":q["name"],"type":kind}],"Answer":[{"type":kind,"data":if kind==1{"104.20.16.234"}else{"2606:4700::1111"},"TTL":60}]}),
        )
    }
    async fn fixture() -> (Server, Dns) {
        let state = Arc::new(ServerState::default());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/dns-query", l.local_addr().unwrap());
        let router = Router::new()
            .route("/dns-query", get(answer))
            .with_state(state.clone());
        let task = tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
        let mut dns = Dns::new(true).unwrap();
        dns.lookup_url = url.clone().into();
        (Server { task, state, url }, dns)
    }
    #[tokio::test]
    async fn dns_singleflight_cache_expiry_and_known_suffix_boundaries() {
        let (server, dns) = fixture().await;
        let rows =
            futures_util::future::join_all((0..4).map(|_| dns.lookup("novels.example"))).await;
        assert!(rows.iter().all(|r| r.as_ref().unwrap().len() == 5));
        assert_eq!(server.state.calls.load(Ordering::SeqCst), 2);
        dns.lookup("novels.example").await.unwrap();
        assert_eq!(server.state.calls.load(Ordering::SeqCst), 2);
        dns.entries
            .lock()
            .unwrap()
            .get_mut("novels.example")
            .unwrap()
            .expires = Instant::now() - Duration::from_millis(1);
        dns.lookup("novels.example").await.unwrap();
        assert_eq!(server.state.calls.load(Ordering::SeqCst), 4);
        dns.lookup("challenges.cloudflare.com").await.unwrap();
        dns.lookup("brunhild.challenges.cloudflare.com")
            .await
            .unwrap();
        assert_eq!(server.state.calls.load(Ordering::SeqCst), 4);
        dns.lookup("challenges.cloudflare.com.attacker.example")
            .await
            .unwrap();
        assert_eq!(server.state.calls.load(Ordering::SeqCst), 6);
    }
    #[tokio::test]
    async fn failed_dns_is_not_cached_and_refusal_expires_with_metadata() {
        let (server, dns) = fixture().await;
        server.state.fail.store(true, Ordering::SeqCst);
        assert!(dns.lookup("novels.example").await.is_err());
        assert!(dns.entries.lock().unwrap().is_empty());
        server.state.fail.store(false, Ordering::SeqCst);
        assert_eq!(dns.lookup("novels.example").await.unwrap().len(), 5);
        dns.mark_refused("novels.example");
        assert_eq!(dns.lookup("novels.example").await.unwrap().len(), 2);
        assert_eq!(dns.lookup("other.example").await.unwrap().len(), 5);
        dns.refused.lock().unwrap().insert(
            "novels.example".into(),
            Instant::now() - Duration::from_millis(1),
        );
        assert_eq!(dns.lookup("novels.example").await.unwrap().len(), 5);
    }
    #[tokio::test]
    async fn dns_time_is_in_the_request_deadline() {
        let (server, dns) = fixture().await;
        server.state.slow.store(true, Ordering::SeqCst);
        let s = Session::new(dns, None).unwrap();
        let start = Instant::now();
        let e = s
            .request(
                "GET",
                "https://unresolved.example/",
                None,
                &[],
                start + Duration::from_millis(50),
            )
            .await
            .unwrap_err();
        assert_eq!(e.code, "TIMEOUT");
        assert!(start.elapsed() < Duration::from_secs(1));
        let _ = &server.url;
    }
    #[tokio::test]
    async fn fallback_retains_login_cookies_and_discards_clearance() {
        let s = Session::new(Dns::new(true).unwrap(), None).unwrap();
        let url = "https://novels.example/";
        s.cookies.add("reader-login=private-test; Path=/", url);
        s.cookies.add("cf_clearance=old-route; Path=/", url);
        s.cookies.add("__cf_bm=old-route; Path=/", url);
        assert!(!s.used_mapping(url));
        s.dns
            .mapped_hosts
            .lock()
            .unwrap()
            .insert("novels.example".into());
        assert!(s.used_mapping(url));
        s.fallback(url).await.unwrap();
        assert!(!s.used_mapping(url));
        let names = s
            .cookies
            .matches(url)
            .map(|c| c.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(names, ["reader-login"]);
    }
    #[tokio::test]
    async fn engine_retries_only_the_initial_pinned_rejection_and_keeps_failure_context() {
        use axum::{body::Body, http::StatusCode, response::IntoResponse};
        let state = Arc::new(ServerState::default());
        let count = state.clone();
        let router = Router::new().fallback(move |request: axum::extract::Request| {
            let count = count.clone();
            async move {
                count.calls.fetch_add(1, Ordering::SeqCst);
                if request.uri().path() == "/bad" {
                    (
                        StatusCode::OK,
                        [("content-encoding", "invalid-test-encoding")],
                        Body::from("invalid body"),
                    )
                        .into_response()
                } else {
                    (
                        StatusCode::OK,
                        [("content-type", "text/html")],
                        Body::from("<html><body>Fresh site document</body></html>"),
                    )
                        .into_response()
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let server = Server { task, state, url };
        let mapped = || {
            let s = Arc::new(Session::new(Dns::new(true).unwrap(), None).unwrap());
            s.dns
                .mapped_hosts
                .lock()
                .unwrap()
                .insert("novels.example".into());
            s
        };
        let rejected = |status| HttpResponse {
            status,
            url: "https://novels.example/".into(),
            headers: json!({"cf-ray":"public-test-ray","content-type":"text/html"})
                .as_object()
                .unwrap()
                .clone(),
            content: b"<html>Denied</html>".to_vec(),
            text: "<html>Denied</html>".into(),
            download_size: 19,
        };
        let engine = crate::engine::Engine::default();
        let deadline = || Instant::now() + Duration::from_secs(2);
        let outcome = engine
            .run(
                mapped(),
                &server.url,
                deadline(),
                false,
                Some(rejected(403)),
            )
            .await
            .unwrap();
        assert!(outcome.fallback);
        assert_eq!(outcome.response.status, 200);
        assert_eq!(outcome.details["routeFallback"]["attempts"], 2);
        assert_eq!(server.state.calls.load(Ordering::SeqCst), 1);
        let error = engine
            .run(
                mapped(),
                &(server.url.clone() + "/bad"),
                deadline(),
                false,
                Some(rejected(403)),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "UNSUPPORTED_ENCODING");
        assert_eq!(error.details["routeFallback"]["attempts"], 2);
        assert_eq!(server.state.calls.load(Ordering::SeqCst), 2);
        let before = server.state.calls.load(Ordering::SeqCst);
        let _ = engine
            .run(mapped(), &server.url, deadline(), true, Some(rejected(403)))
            .await;
        let ordinary = engine
            .run(
                mapped(),
                &server.url,
                deadline(),
                false,
                Some(rejected(400)),
            )
            .await
            .unwrap();
        assert!(!ordinary.fallback);
        assert_eq!(server.state.calls.load(Ordering::SeqCst), before);
    }
    #[test]
    fn mapping_does_not_apply_to_plain_http_custom_ports_or_explicit_proxies() {
        let s = Session::new(Dns::new(true).unwrap(), None).unwrap();
        s.dns
            .mapped_hosts
            .lock()
            .unwrap()
            .insert("novels.example".into());
        assert!(s.used_mapping("https://novels.example/"));
        for url in [
            "http://novels.example/",
            "https://novels.example:8443/",
            "https://other.example/",
        ] {
            assert!(!s.used_mapping(url));
        }
        let proxy = Session::new(
            Dns::new(true).unwrap(),
            Some("http://127.0.0.1:8080".into()),
        )
        .unwrap();
        proxy
            .dns
            .mapped_hosts
            .lock()
            .unwrap()
            .insert("novels.example".into());
        assert!(!proxy.used_mapping("https://novels.example/"));
    }
}
