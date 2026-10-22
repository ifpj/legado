use crate::{Error, Result};
use serde_json::Value;
use std::{collections::BTreeMap, net::SocketAddr};

pub const DOH_URL: &str = "https://cloudflare-dns.com/dns-query";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum NetworkMode {
    #[default]
    Direct,
    Proxy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkConfig {
    pub mode: NetworkMode,
    pub proxy: Option<String>,
    pub doh: bool,
    pub doh_url: String,
}
impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            mode: NetworkMode::Direct,
            proxy: None,
            doh: false,
            doh_url: DOH_URL.into(),
        }
    }
}
pub(crate) fn invalid(code: &str, message: &str) -> Error {
    let mut error = Error::new(code, message);
    error.status = 400;
    error
}
impl NetworkConfig {
    /// Request overrides are ephemeral; none of these options changes session keys.
    pub fn request(&self, options: &Value) -> Result<(Self, ConnectTo)> {
        let decoded;
        let network = match options.get("network") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => {
                decoded = serde_json::from_str::<Value>(text)
                    .map_err(|_| invalid("INVALID_NETWORK", "network 必须是 JSON 对象"))?;
                Some(&decoded)
            }
            Some(value) => Some(value),
        };
        let network = network
            .map(|value| {
                value.as_object().ok_or_else(|| {
                    invalid("INVALID_NETWORK", "network 必须包含 proxy、doh 或 mapping")
                })
            })
            .transpose()?;
        if network.is_some_and(|n| {
            n.keys()
                .any(|k| !matches!(k.as_str(), "proxy" | "doh" | "mapping"))
        }) {
            return Err(invalid(
                "INVALID_NETWORK",
                "network 只支持 proxy、doh、mapping",
            ));
        }
        let mut config = self.clone();
        if let Some(value) = network
            .and_then(|n| n.get("proxy"))
            .or_else(|| options.get("proxy").filter(|value| !value.is_null()))
        {
            config.proxy = match value {
                Value::Null => None,
                Value::String(text) if text.trim().is_empty() => None,
                Value::String(text) => Some(text.trim().to_owned()),
                _ => {
                    return Err(invalid(
                        "INVALID_PROXY",
                        "proxy 需要代理地址或 null（直连）",
                    ))
                }
            };
            config.mode = if config.proxy.is_some() {
                NetworkMode::Proxy
            } else {
                NetworkMode::Direct
            };
        }
        if let Some(value) = network.and_then(|n| n.get("doh")) {
            let doh = value
                .as_object()
                .ok_or_else(|| invalid("INVALID_DOH", "doh 需要包含 enabled、url 的对象"))?;
            if doh.keys().any(|k| !matches!(k.as_str(), "enabled" | "url")) {
                return Err(invalid("INVALID_DOH", "doh 只支持 enabled、url"));
            }
            if let Some(enabled) = doh.get("enabled") {
                config.doh = enabled
                    .as_bool()
                    .ok_or_else(|| invalid("INVALID_DOH", "doh.enabled 必须是布尔值"))?;
            }
            if let Some(url) = doh.get("url") {
                config.doh_url = url
                    .as_str()
                    .ok_or_else(|| invalid("INVALID_DOH", "doh.url 必须是服务器地址"))?
                    .to_owned();
            }
        }
        let mapping = ConnectTo::parse(
            network
                .and_then(|n| n.get("mapping"))
                .or_else(|| options.get("connectTo")),
        )?;
        config.validate()?;
        Ok((config, mapping))
    }
    pub fn validate(&self) -> Result<()> {
        match (self.mode, self.proxy.as_deref()) {
            (NetworkMode::Proxy, None) => {
                return Err(invalid(
                    "INVALID_PROXY",
                    "代理模式需要 --proxy 或 CF_GATEWAY_PROXY",
                ))
            }
            (NetworkMode::Direct, Some(_)) => {
                return Err(invalid("INVALID_PROXY", "配置代理地址时请选择 proxy 模式"))
            }
            (_, Some(value)) => {
                proxy_url(value)?;
            }
            _ => {}
        }
        let url = url::Url::parse(&self.doh_url)
            .map_err(|_| invalid("INVALID_DOH", "DoH 服务器地址无效"))?;
        let local = matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"));
        if (url.scheme() != "https" && !(url.scheme() == "http" && local))
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
        {
            return Err(invalid(
                "INVALID_DOH",
                "DoH 需要不带账号、查询参数和片段的 HTTPS 地址；本地测试允许 HTTP",
            ));
        }
        Ok(())
    }
    pub fn dns_mode(&self) -> &'static str {
        if self.doh {
            "doh"
        } else if self.mode == NetworkMode::Proxy {
            "proxy"
        } else {
            "system"
        }
    }
}
pub(crate) fn proxy_url(value: &str) -> Result<url::Url> {
    let url = url::Url::parse(value).map_err(|_| invalid("INVALID_PROXY", "代理地址无效"))?;
    if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h")
        || url.host_str().is_none()
        || url.port_or_known_default().unwrap_or(0) == 0
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "INVALID_PROXY",
            "代理需要使用 HTTP/HTTPS/SOCKS5 地址并指定有效端口",
        ));
    }
    Ok(url)
}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(transparent)]
pub struct ConnectTo(pub(crate) BTreeMap<String, SocketAddr>);

pub(crate) fn authority(host: &str, port: u16) -> String {
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}
impl ConnectTo {
    pub fn parse(value: Option<&Value>) -> Result<Self> {
        let decoded;
        let value = match value {
            None | Some(Value::Null) => return Ok(Self::default()),
            Some(Value::String(text)) => {
                decoded = serde_json::from_str(text)
                    .map_err(|_| invalid("INVALID_CONNECT_TO", "connectTo 必须是 JSON 对象"))?;
                &decoded
            }
            Some(value) => value,
        };
        let object = value
            .as_object()
            .filter(|v| v.len() <= 32)
            .ok_or_else(|| invalid("INVALID_CONNECT_TO", "connectTo 必须是最多 32 项的对象"))?;
        let mut entries = BTreeMap::new();
        for (name, destination) in object {
            let destination = destination
                .as_str()
                .and_then(|s| s.parse::<SocketAddr>().ok())
                .filter(|a| a.port() > 0)
                .ok_or_else(|| {
                    invalid(
                        "INVALID_CONNECT_TO",
                        "映射目标必须是 IP:端口，IPv6 使用 [IP]:端口",
                    )
                })?;
            if name == "*" {
                entries.insert(name.clone(), destination);
                continue;
            }
            let url = url::Url::parse(&format!("http://{name}/"))
                .map_err(|_| invalid("INVALID_CONNECT_TO", "映射键需要域名:端口"))?;
            let (host, port) = name
                .rsplit_once(':')
                .ok_or_else(|| invalid("INVALID_CONNECT_TO", "映射键必须显式指定端口"))?;
            let port = port
                .parse::<u16>()
                .ok()
                .filter(|p| *p > 0)
                .ok_or_else(|| invalid("INVALID_CONNECT_TO", "映射端口无效"))?;
            if host.is_empty()
                || host
                    .chars()
                    .any(|c| c.is_whitespace() || matches!(c, '/' | '?' | '#' | '@'))
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/"
            {
                return Err(invalid(
                    "INVALID_CONNECT_TO",
                    "映射键需要精确域名和端口，或单独的 *；不支持路径",
                ));
            }
            let host = url
                .host_str()
                .filter(|h| !h.contains('*'))
                .ok_or_else(|| invalid("INVALID_CONNECT_TO", "映射域名无效"))?;
            if entries.insert(authority(host, port), destination).is_some() {
                return Err(invalid("INVALID_CONNECT_TO", "映射中存在重复的域名和端口"));
            }
        }
        Ok(Self(entries))
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn target(&self, host: &str, port: u16) -> Option<SocketAddr> {
        self.0
            .get(&authority(host, port))
            .or_else(|| self.0.get("*"))
            .copied()
    }
}
