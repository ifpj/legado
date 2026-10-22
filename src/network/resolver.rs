use super::{config::NetworkConfig, dns_wire, error, profile};
use crate::Result;
use base64::Engine;
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use wreq::{
    dns::{Addrs, Name, Resolve, Resolving},
    Client,
};
#[derive(Clone)]
struct Record {
    addresses: Vec<SocketAddr>,
    expires: Instant,
}
#[derive(Clone)]
pub struct Dns {
    pub(super) config: Arc<NetworkConfig>,
    client: Client,
    entries: Arc<Mutex<HashMap<String, Record>>>,
    pending: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    pub(super) connections: Arc<tokio::sync::Semaphore>,
}
impl Dns {
    pub(super) fn for_proxy(&self, proxy: Option<String>) -> Result<Self> {
        if proxy == self.config.proxy {
            return Ok(self.clone());
        }
        let mut config = (*self.config).clone();
        config.mode = if proxy.is_some() {
            super::NetworkMode::Proxy
        } else {
            super::NetworkMode::Direct
        };
        config.proxy = proxy;
        Self::new(&config)
    }
    pub fn new(config: &NetworkConfig) -> Result<Self> {
        config.validate()?;
        let mut builder = Client::builder()
            .emulation(profile())
            .no_proxy()
            .retry(wreq::retry::Policy::never())
            .redirect(wreq::redirect::Policy::none());
        if let Some(proxy) = &config.proxy {
            let mut bootstrap = url::Url::parse(proxy).unwrap();
            if bootstrap.scheme() == "socks5" {
                bootstrap.set_scheme("socks5h").unwrap();
            }
            builder = builder.proxy(wreq::Proxy::all(bootstrap.as_str()).map_err(error)?);
        }
        Ok(Self {
            config: Arc::new(config.clone()),
            client: builder.build().map_err(error)?,
            entries: Arc::default(),
            pending: Arc::default(),
            connections: Arc::new(tokio::sync::Semaphore::new(128)),
        })
    }
    pub(super) async fn lookup(
        &self,
        host: &str,
    ) -> std::result::Result<Vec<SocketAddr>, Box<dyn std::error::Error + Send + Sync>> {
        let host = host
            .trim_matches(['[', ']'])
            .trim_end_matches('.')
            .to_ascii_lowercase();
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(vec![SocketAddr::new(ip, 0)]);
        }
        if !self.config.doh {
            return Ok(tokio::net::lookup_host((host.as_str(), 0)).await?.collect());
        }
        let gate = {
            let mut pending = self.pending.lock().unwrap();
            pending.retain(|_, v| Arc::strong_count(v) > 1);
            pending.entry(host.clone()).or_default().clone()
        };
        let _lookup = gate.lock().await;
        if let Some(entry) = self
            .entries
            .lock()
            .unwrap()
            .get(&host)
            .filter(|e| e.expires > Instant::now())
            .cloned()
        {
            return Ok(entry.addresses);
        }
        let name = host.as_str();
        let query = |kind| async move {
            let packet = dns_wire::query(name, kind)?;
            let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(packet);
            let response = self
                .client
                .get(&self.config.doh_url)
                .query(&[("dns", encoded)])
                .header("accept", "application/dns-message")
                .timeout(Duration::from_secs(3))
                .send()
                .await?
                .error_for_status()?;
            if !response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| {
                    v.split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .eq_ignore_ascii_case("application/dns-message")
                })
            {
                return Err::<_, Box<dyn std::error::Error + Send + Sync>>(
                    "DoH returned an unsupported media type".into(),
                );
            }
            let mut stream = response.bytes_stream();
            let mut bytes = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                if bytes.len() + chunk.len() > 65535 {
                    return Err("DoH response too large".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(dns_wire::answer(&bytes, name, kind)?)
        };
        let (aaaa, a) = tokio::join!(query(28), query(1));
        let mut addresses = Vec::new();
        let mut ttl = 300;
        for (ips, lifetime) in [aaaa, a].into_iter().flatten() {
            ttl = ttl.min(lifetime);
            for ip in ips {
                let address = SocketAddr::new(ip, 0);
                if !addresses.contains(&address) {
                    addresses.push(address);
                }
            }
        }
        if addresses.is_empty() {
            return Err("DoH did not return usable addresses".into());
        }
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, e| e.expires > Instant::now());
        if entries.len() < 256 {
            entries.insert(
                host,
                Record {
                    addresses: addresses.clone(),
                    expires: Instant::now() + Duration::from_secs(ttl),
                },
            );
        }
        Ok(addresses)
    }
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
