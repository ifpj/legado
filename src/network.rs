use anyhow::{Result, anyhow, ensure};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const DOH_ENDPOINT: &str = "https://dns.alidns.com/dns-query";
pub struct Network {
    client: reqwest::Client,
    resolver: Arc<Resolver>,
}
impl Network {
    pub fn new() -> Result<Self> {
        let resolver = Arc::new(Resolver {
            // Only the DoH transport itself uses reqwest's built-in bootstrap
            // resolution, avoiding recursion. All service upstream names use DoH.
            bootstrap: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()?,
            cache: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            stats: Arc::new(Mutex::new(Stats::default())),
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .gzip(true)
            .http2_adaptive_window(true)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .no_proxy()
            .dns_resolver(resolver.clone())
            .build()?;
        Ok(Self { client, resolver })
    }
    pub fn client(&self) -> reqwest::Client {
        self.client.clone()
    }
    pub fn status(&self) -> Value {
        json!({"mode":"doh","endpoint":DOH_ENDPOINT,"ipv6Preferred":true,"dnsOnlyTtlCache":true,
            "stats":self.resolver.stats.lock().unwrap().clone()})
    }
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Stats {
    lookups: u64,
    cache_hits: u64,
    failures: u64,
    last_lookup_ms: u64,
    last_host: String,
    last_error: Option<String>,
    addresses: Vec<String>,
}
#[derive(Clone)]
struct Entry {
    addresses: Vec<SocketAddr>,
    expires: Instant,
}
struct Resolver {
    bootstrap: reqwest::Client,
    cache: Arc<tokio::sync::Mutex<HashMap<String, Entry>>>,
    stats: Arc<Mutex<Stats>>,
}
impl Resolve for Resolver {
    fn resolve(&self, name: Name) -> Resolving {
        let name = name.as_str().to_owned();
        let client = self.bootstrap.clone();
        // Resolve has no lifetime on its future, so cache/stats are shared handles.
        let cache = self.cache_handle();
        let stats = self.stats_handle();
        Box::pin(async move {
            let mut entries = cache.lock().await;
            entries.retain(|_, e| e.expires > Instant::now());
            if let Some(entry) = entries.get(&name) {
                stats.lock().unwrap().cache_hits += 1;
                return Ok(Box::new(entry.addresses.clone().into_iter()) as Addrs);
            }
            let began = Instant::now();
            let (v6, v4) = tokio::join!(query(&client, &name, 28), query(&client, &name, 1));
            let mut addresses = vec![];
            let mut ttl = u32::MAX;
            let mut errors = vec![];
            for response in [v6, v4] {
                match response {
                    Ok((ips, t)) => {
                        if !ips.is_empty() {
                            ttl = ttl.min(t);
                            addresses.extend(ips.into_iter().map(|ip| SocketAddr::new(ip, 443)));
                        }
                    }
                    Err(e) => errors.push(e.to_string()),
                }
            }
            {
                let mut report = stats.lock().unwrap();
                report.lookups += 1;
                report.last_host = name.clone();
                report.last_lookup_ms = began.elapsed().as_millis() as u64;
                if addresses.is_empty() {
                    report.failures += 1;
                    report.addresses.clear();
                    report.last_error = Some(if errors.is_empty() {
                        "DNS 未返回可用地址".into()
                    } else {
                        errors.join("；")
                    });
                } else {
                    report.last_error = if errors.is_empty() {
                        None
                    } else {
                        Some(errors.join("；"))
                    };
                    report.addresses = addresses.iter().map(|a| a.ip().to_string()).collect();
                }
            }
            if addresses.is_empty() {
                drop(entries);
                return Err(anyhow!("DoH 无法解析 {name}：{}", errors.join("；")).into());
            }
            if entries.len() < 64 && ttl > 0 {
                entries.insert(
                    name,
                    Entry {
                        addresses: addresses.clone(),
                        expires: Instant::now() + Duration::from_secs(ttl as u64),
                    },
                );
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}
// Handles are separate from the reqwest client to avoid a recursive resolver.
impl Resolver {
    fn cache_handle(&self) -> Arc<tokio::sync::Mutex<HashMap<String, Entry>>> {
        self.cache.clone()
    }
    fn stats_handle(&self) -> Arc<Mutex<Stats>> {
        self.stats.clone()
    }
}
async fn query(client: &reqwest::Client, name: &str, kind: u16) -> Result<(Vec<IpAddr>, u32)> {
    let id = rand::random::<u16>();
    let request = question(name, kind, id)?;
    let response = client
        .post(DOH_ENDPOINT)
        .header("content-type", "application/dns-message")
        .header("accept", "application/dns-message")
        .body(request)
        .send()
        .await?
        .error_for_status()?;
    let bytes = response.bytes().await?;
    ensure!(bytes.len() <= 65535, "DNS 响应过大");
    let (ips, ttl) = answer(&bytes, id)?;
    Ok((
        ips.into_iter()
            .filter(|ip| match kind {
                28 => ip.is_ipv6(),
                _ => ip.is_ipv4(),
            })
            .collect(),
        ttl,
    ))
}
fn question(name: &str, kind: u16, id: u16) -> Result<Vec<u8>> {
    ensure!(name.len() <= 253, "DNS 名称过长");
    let mut bytes = id.to_be_bytes().to_vec();
    bytes.extend([1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.trim_end_matches('.').split('.') {
        ensure!(
            !label.is_empty() && label.len() <= 63 && label.is_ascii(),
            "DNS 名称无效"
        );
        bytes.push(label.len() as u8);
        bytes.extend(label.as_bytes());
    }
    bytes.push(0);
    bytes.extend(kind.to_be_bytes());
    bytes.extend([0, 1]);
    Ok(bytes)
}
fn word(bytes: &[u8], offset: usize) -> Result<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| anyhow!("DNS 响应截断"))?;
    Ok(u16::from_be_bytes([value[0], value[1]]))
}
fn skip_name(bytes: &[u8], offset: &mut usize) -> Result<()> {
    for _ in 0..128 {
        let value = *bytes.get(*offset).ok_or_else(|| anyhow!("DNS 名称截断"))?;
        *offset += 1;
        if value == 0 {
            return Ok(());
        }
        if value & 0xc0 == 0xc0 {
            ensure!(*offset < bytes.len(), "DNS 指针截断");
            *offset += 1;
            return Ok(());
        }
        ensure!(value <= 63, "DNS 标签无效");
        *offset += value as usize;
        ensure!(*offset <= bytes.len(), "DNS 标签截断");
    }
    Err(anyhow!("DNS 名称过长"))
}
fn answer(bytes: &[u8], id: u16) -> Result<(Vec<IpAddr>, u32)> {
    ensure!(bytes.len() >= 12 && word(bytes, 0)? == id, "DNS 响应不匹配");
    let flags = word(bytes, 2)?;
    ensure!(
        flags & 0x8000 != 0 && flags & 0xf == 0 && flags & 0x0200 == 0,
        "DNS 返回错误或截断"
    );
    let mut offset = 12;
    for _ in 0..word(bytes, 4)? {
        skip_name(bytes, &mut offset)?;
        offset += 4;
        ensure!(offset <= bytes.len(), "DNS 问题截断");
    }
    let mut ips = vec![];
    let mut ttl = u32::MAX;
    for _ in 0..word(bytes, 6)? {
        skip_name(bytes, &mut offset)?;
        let kind = word(bytes, offset)?;
        let class = word(bytes, offset + 2)?;
        let t = bytes
            .get(offset + 4..offset + 8)
            .ok_or_else(|| anyhow!("DNS TTL 截断"))?;
        let t = u32::from_be_bytes(t.try_into()?);
        let length = word(bytes, offset + 8)? as usize;
        offset += 10;
        let data = bytes
            .get(offset..offset + length)
            .ok_or_else(|| anyhow!("DNS 记录截断"))?;
        offset += length;
        if class == 1 {
            ttl = ttl.min(t);
            match (kind, length) {
                (1, 4) => ips.push(IpAddr::from(<[u8; 4]>::try_from(data)?)),
                (28, 16) => ips.push(IpAddr::from(<[u8; 16]>::try_from(data)?)),
                _ => {}
            }
        }
    }
    Ok((ips, ttl))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dns_wire_accepts_compression_and_rejects_truncation_and_foreign_id() {
        let mut bytes = question("reading.snssdk.com", 28, 7).unwrap();
        bytes[2] = 0x81;
        bytes[3] = 0x80;
        bytes[7] = 1;
        bytes.extend([0xc0, 0x0c, 0, 28, 0, 1, 0, 0, 0, 9, 0, 16]);
        bytes.extend(
            "2409:8c4c:e00:204:3::24"
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets(),
        );
        assert_eq!(answer(&bytes, 7).unwrap().1, 9);
        assert!(answer(&bytes, 8).is_err());
        for length in 0..bytes.len() {
            assert!(answer(&bytes[..length], 7).is_err());
        }
    }
    #[test]
    fn dns_wire_rejects_negative_and_truncated_response_and_reads_ipv4() {
        let mut bytes = question("reading.snssdk.com", 1, 11).unwrap();
        bytes[2] = 0x81;
        bytes[3] = 0x80;
        assert!(answer(&bytes, 11).unwrap().0.is_empty());
        bytes[3] = 0x83;
        assert!(answer(&bytes, 11).is_err());
        bytes[3] = 0x80;
        bytes[2] = 0x83;
        assert!(answer(&bytes, 11).is_err());
        bytes[2] = 0x81;
        bytes[7] = 1;
        bytes.extend([0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 30, 0, 4, 111, 48, 177, 41]);
        let (ips, ttl) = answer(&bytes, 11).unwrap();
        assert_eq!(ips, vec!["111.48.177.41".parse::<IpAddr>().unwrap()]);
        assert_eq!(ttl, 30);
    }
}
