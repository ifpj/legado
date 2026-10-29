use crate::{re, require, static_data::validate_url, Error, Result};
use futures_util::StreamExt;
use serde_json::{Map, Value};
use std::{collections::HashSet, future::Future, sync::Arc, time::Instant};
use tokio::{io::AsyncReadExt, sync::RwLock};
use wreq::{
    cookie::Jar,
    header::{HeaderMap, HeaderName, HeaderValue},
    Client, Method,
};
use wreq_util::{Emulation, Platform, Profile};
mod config;
mod dns_wire;
mod resolver;
mod transport;
pub use config::{ConnectTo, NetworkConfig, NetworkMode, DOH_URL};
pub use resolver::Dns;
use transport::Transport;
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
pub struct RequestContext {
    pub connect_to: ConnectTo,
    binary_images: bool,
    dns: Option<Dns>,
    transport: tokio::sync::OnceCell<Transport>,
}
impl RequestContext {
    pub fn new(connect_to: ConnectTo) -> Arc<Self> {
        Arc::new(Self {
            connect_to,
            binary_images: false,
            dns: None,
            transport: tokio::sync::OnceCell::new(),
        })
    }
    pub fn configured(connect_to: ConnectTo, dns: Dns) -> Arc<Self> {
        Self::for_response(connect_to, dns, false)
    }
    pub(crate) fn for_response(connect_to: ConnectTo, dns: Dns, binary_images: bool) -> Arc<Self> {
        Arc::new(Self {
            connect_to,
            binary_images,
            dns: Some(dns),
            transport: tokio::sync::OnceCell::new(),
        })
    }
}
tokio::task_local! { static REQUEST_CONTEXT: Arc<RequestContext>; }
fn current_context() -> Option<Arc<RequestContext>> {
    REQUEST_CONTEXT.try_with(Arc::clone).ok()
}
pub async fn with_context<F: Future>(context: Arc<RequestContext>, future: F) -> F::Output {
    REQUEST_CONTEXT.scope(context, future).await
}
pub fn inherit_context<F: Future>(future: F) -> impl Future<Output = F::Output> {
    let context = current_context();
    async move {
        if let Some(context) = context {
            with_context(context, future).await
        } else {
            future.await
        }
    }
}
pub struct Session {
    client: tokio::sync::OnceCell<Transport>,
    pub cookies: Arc<Jar>,
    pub dns: Dns,
    pub proxy: Option<String>,
    pub gate: RwLock<()>,
    pub reads: tokio::sync::Semaphore,
    pub verified: std::sync::atomic::AtomicBool,
    pub generation: std::sync::atomic::AtomicU64,
}
impl Session {
    pub fn new(dns: Dns, proxy: Option<String>) -> Result<Self> {
        Self::with_cookies(dns, proxy, Arc::new(Jar::default()))
    }
    pub(crate) fn with_cookies(dns: Dns, proxy: Option<String>, cookies: Arc<Jar>) -> Result<Self> {
        let dns = dns.for_proxy(proxy.clone())?;
        Ok(Self {
            client: tokio::sync::OnceCell::new(),
            cookies,
            dns,
            proxy,
            gate: RwLock::new(()),
            reads: tokio::sync::Semaphore::new(4),
            verified: false.into(),
            generation: 0.into(),
        })
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
    /// Opens the TCP or proxy tunnel for a host this session will contact
    /// next. No TLS, HTTP request or cookie is sent before the real request.
    pub(crate) async fn prewarm(&self, url: &str) {
        let Ok(url) = validate_url(url) else {
            return;
        };
        let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else {
            return;
        };
        let context = current_context();
        if let Some(context) = context.as_ref().filter(|c| {
            !c.connect_to.is_empty()
                || c.dns
                    .as_ref()
                    .is_some_and(|dns| dns.config != self.dns.config)
        }) {
            let dns = context.dns.clone().unwrap_or_else(|| self.dns.clone());
            let proxy = dns.config.proxy.clone();
            if let Ok(transport) = context
                .transport
                .get_or_try_init(|| {
                    Transport::new(dns, self.cookies.clone(), proxy, context.connect_to.clone())
                })
                .await
            {
                transport.prewarm(host, port);
            }
        } else if let Ok(transport) = self
            .client
            .get_or_try_init(|| {
                Transport::new(
                    self.dns.clone(),
                    self.cookies.clone(),
                    self.proxy.clone(),
                    ConnectTo::default(),
                )
            })
            .await
        {
            transport.prewarm(host, port);
        }
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
        validate_url(url)?;
        let context = current_context();
        let transport = if let Some(context) = context.as_ref().filter(|c| {
            !c.connect_to.is_empty()
                || c.dns
                    .as_ref()
                    .is_some_and(|dns| dns.config != self.dns.config)
        }) {
            let dns = context.dns.clone().unwrap_or_else(|| self.dns.clone());
            let proxy = dns.config.proxy.clone();
            context
                .transport
                .get_or_try_init(|| {
                    Transport::new(dns, self.cookies.clone(), proxy, context.connect_to.clone())
                })
                .await?
        } else {
            self.client
                .get_or_try_init(|| {
                    Transport::new(
                        self.dns.clone(),
                        self.cookies.clone(),
                        self.proxy.clone(),
                        ConnectTo::default(),
                    )
                })
                .await?
        };
        let client = transport.client.clone();
        let route = transport.route(&validate_url(url)?);
        let mut request = client
            .request(Method::from_bytes(method.as_bytes()).map_err(error)?, url)
            .redirect(wreq::redirect::Policy::none())
            .timeout(remaining);
        if let Some(route) = route {
            request = request.proxy(route);
        }
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
        let text = response_text(
            &content,
            &headers,
            context.as_ref().is_some_and(|c| c.binary_images),
        );
        Ok(HttpResponse {
            status,
            url: final_url,
            headers,
            content,
            text: text.into(),
            download_size,
        })
    }
}
fn response_text(content: &[u8], headers: &Map<String, Value>, binary_images: bool) -> String {
    // Image URLs can return HTML challenges, even with an incorrect MIME type.
    // Skip decoding only for the same byte signatures accepted by /image.
    if binary_images && image_kind(content).is_some() {
        String::new()
    } else {
        decode(content, headers)
    }
}
pub(crate) fn image_kind(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if b.get(4..8) == Some(b"ftyp") && matches!(b.get(8..12), Some(b"avif" | b"avis")) {
        Some("image/avif")
    } else {
        None
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
    pub text: crate::html::Document,
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
mod tests;
