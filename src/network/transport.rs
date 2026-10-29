//! A loopback SOCKS tunnel keeps URL/Host/SNI intact when overriding a port,
//! and makes HTTP and SOCKS upstream proxies obey the same DNS policy.
use super::{
    client,
    config::{authority, proxy_url},
    ConnectTo, Dns,
};
use crate::Result;
use base64::Engine;
use std::{
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};
trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
type Tunnel = Box<dyn Stream>;
pub(super) struct Transport {
    pub client: wreq::Client,
    _bridge: Option<Bridge>,
    predial: Arc<PreDial>,
    /// Direct clients normally connect by themselves. A loopback route is
    /// started only after a host has been pre-dialed for this transport.
    direct: Option<DirectRoute>,
    bridged: Option<(Dns, Option<String>, ConnectTo)>,
}
struct DirectRoute {
    dns: Dns,
    bridge: std::sync::OnceLock<std::result::Result<(String, Bridge), String>>,
}
struct Bridge {
    task: JoinHandle<()>,
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}
/// A pre-dialed connection carries only TCP (or the configured proxy tunnel).
/// TLS, HTTP and cookies still start when the protocol makes its request.
/// Short enough that an idle upstream socket has not been closed for lack of
/// a TLS handshake; an older slot is discarded and the request dials anew.
const PREDIAL_MAX_AGE: Duration = Duration::from_secs(5);
const PREDIAL_LIMIT: usize = 8;
#[derive(Default)]
struct PreDial {
    slots: std::sync::Mutex<std::collections::HashMap<(String, u16), Slot>>,
    routed: std::sync::Mutex<std::collections::HashSet<(String, u16)>>,
}
struct Slot {
    born: std::time::Instant,
    task: Option<JoinHandle<io::Result<Tunnel>>>,
}
impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
impl PreDial {
    fn take(&self, host: &str, port: u16) -> Option<JoinHandle<io::Result<Tunnel>>> {
        let mut slots = self.slots.lock().unwrap();
        let mut slot = slots.remove(&(host.to_ascii_lowercase(), port))?;
        if slot.born.elapsed() > PREDIAL_MAX_AGE {
            return None;
        }
        slot.task.take()
    }
}
fn bridge(
    listener: TcpListener,
    dns: Dns,
    proxy: Option<String>,
    connect_to: ConnectTo,
    predial: Arc<PreDial>,
) -> Bridge {
    let task = tokio::spawn(async move {
        let mut tasks = JoinSet::new();
        loop {
            tokio::select! {
                connection=listener.accept()=>{
                    let Ok((stream,_))=connection else{break};
                    let Ok(permit)=dns.connections.clone().try_acquire_owned() else{drop(stream);continue};
                    let dns=dns.clone();let proxy=proxy.clone();let connect_to=connect_to.clone();let predial=predial.clone();
                    tasks.spawn(async move {let _permit=permit;let _=serve(stream,dns,proxy,connect_to,predial).await;});
                }
                _=tasks.join_next(),if !tasks.is_empty()=>{}
            }
        }
    });
    Bridge { task }
}
impl Transport {
    pub async fn new(
        dns: Dns,
        cookies: Arc<wreq::cookie::Jar>,
        proxy: Option<String>,
        connect_to: ConnectTo,
    ) -> Result<Self> {
        let predial = Arc::new(PreDial::default());
        if proxy.is_none() && connect_to.is_empty() {
            return Ok(Self {
                client: client(dns.clone(), cookies, None)?,
                _bridge: None,
                predial,
                direct: Some(DirectRoute {
                    dns,
                    bridge: std::sync::OnceLock::new(),
                }),
                bridged: None,
            });
        }
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let local = format!("socks5h://{}", listener.local_addr()?);
        let client = client(dns.clone(), cookies, Some(&local))?;
        Ok(Self {
            client,
            _bridge: Some(bridge(
                listener,
                dns.clone(),
                proxy.clone(),
                connect_to.clone(),
                predial.clone(),
            )),
            predial,
            direct: None,
            bridged: Some((dns, proxy, connect_to)),
        })
    }
    /// Starts the transport connection for a host the current protocol will
    /// contact next. Failures are ignored; the request then dials normally.
    pub fn prewarm(&self, host: &str, port: u16) {
        let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
        if host.is_empty() || port == 0 {
            return;
        }
        let (dns, proxy, connect_to) = if let Some(direct) = &self.direct {
            let started = direct.bridge.get_or_init(|| {
                let listener = std::net::TcpListener::bind("127.0.0.1:0")
                    .and_then(|l| l.set_nonblocking(true).map(|_| l))
                    .and_then(TcpListener::from_std)
                    .map_err(|e| e.to_string())?;
                let local = format!(
                    "socks5h://{}",
                    listener.local_addr().map_err(|e| e.to_string())?
                );
                Ok((
                    local,
                    bridge(
                        listener,
                        direct.dns.clone(),
                        None,
                        ConnectTo::default(),
                        self.predial.clone(),
                    ),
                ))
            });
            if started.is_err() {
                return;
            }
            (direct.dns.clone(), None, ConnectTo::default())
        } else if let Some((dns, proxy, connect_to)) = &self.bridged {
            (dns.clone(), proxy.clone(), connect_to.clone())
        } else {
            return;
        };
        if self.direct.is_some() {
            self.predial
                .routed
                .lock()
                .unwrap()
                .insert((host.clone(), port));
        }
        self.spawn_predial(host, port, dns, proxy, connect_to);
    }
    fn spawn_predial(
        &self,
        host: String,
        port: u16,
        dns: Dns,
        proxy: Option<String>,
        connect_to: ConnectTo,
    ) {
        let mut slots = self.predial.slots.lock().unwrap();
        slots.retain(|_, slot| slot.born.elapsed() <= PREDIAL_MAX_AGE);
        let key = (host.clone(), port);
        if slots.contains_key(&key) || slots.len() >= PREDIAL_LIMIT {
            return;
        }
        let task = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(10), async move {
                let destinations =
                    destinations(host, port, &dns, proxy.as_deref(), &connect_to).await?;
                dial(destinations, proxy.as_deref()).await
            })
            .await
            .map_err(|_| failed("Predial timed out"))?
        });
        slots.insert(
            key,
            Slot {
                born: std::time::Instant::now(),
                task: Some(task),
            },
        );
    }
    /// Direct requests to a pre-dialed host use the transport's loopback
    /// route for the whole session, so its HTTP/2 pool key stays stable.
    pub fn route(&self, url: &url::Url) -> Option<wreq::Proxy> {
        let direct = self.direct.as_ref()?;
        let host = url
            .host_str()?
            .trim_matches(['[', ']'])
            .to_ascii_lowercase();
        let port = url.port_or_known_default()?;
        if !self.predial.routed.lock().unwrap().contains(&(host, port)) {
            return None;
        }
        let (local, _) = direct.bridge.get()?.as_ref().ok()?;
        wreq::Proxy::all(local.as_str()).ok()
    }
}
enum Destination {
    Socket(SocketAddr),
    Name(String, u16),
}
impl Destination {
    fn authority(&self) -> String {
        match self {
            Self::Socket(s) => s.to_string(),
            Self::Name(host, port) => authority(host, *port),
        }
    }
}
fn failed(message: impl std::fmt::Display) -> io::Error {
    io::Error::other(message.to_string())
}
async fn destinations(
    host: String,
    port: u16,
    dns: &Dns,
    proxy: Option<&str>,
    connect_to: &ConnectTo,
) -> io::Result<Vec<Destination>> {
    Ok(if let Some(target) = connect_to.target(&host, port) {
        vec![Destination::Socket(target)]
    } else if let Ok(ip) = host.parse::<IpAddr>() {
        vec![Destination::Socket(SocketAddr::new(ip, port))]
    } else if proxy.is_some() && !dns.config.doh {
        vec![Destination::Name(host, port)]
    } else {
        dns.lookup(&host)
            .await
            .map_err(failed)?
            .into_iter()
            .map(|mut ip| {
                ip.set_port(port);
                Destination::Socket(ip)
            })
            .collect()
    })
}
/// Proxies are single endpoints and keep their sequential behaviour. Direct
/// addresses use a short stagger so one stalled address family cannot hold
/// the whole connection, matching the client connector's fallback timing.
async fn dial(destinations: Vec<Destination>, proxy: Option<&str>) -> io::Result<Tunnel> {
    let mut last = failed("No target addresses");
    if proxy.is_some() || destinations.len() <= 1 {
        for destination in destinations {
            match connect(&destination, proxy).await {
                Ok(stream) => return Ok(stream),
                Err(error) => last = error,
            }
        }
        return Err(last);
    }
    let mut pending = JoinSet::new();
    let mut remaining = destinations.into_iter();
    loop {
        if let Some(destination) = remaining.next() {
            pending.spawn(async move { connect(&destination, None).await });
        }
        let stagger = tokio::time::sleep(Duration::from_millis(300));
        tokio::pin!(stagger);
        loop {
            tokio::select! {
                finished = pending.join_next(), if !pending.is_empty() => {
                    match finished {
                        Some(Ok(Ok(stream))) => return Ok(stream),
                        Some(Ok(Err(error))) => last = error,
                        Some(Err(error)) => last = failed(error),
                        None => {}
                    }
                    if pending.is_empty() {
                        break;
                    }
                }
                _ = &mut stagger => break,
            }
        }
        if pending.is_empty() && remaining.len() == 0 {
            return Err(last);
        }
    }
}
async fn serve(
    mut inbound: TcpStream,
    dns: Dns,
    proxy: Option<String>,
    connect_to: ConnectTo,
    predial: Arc<PreDial>,
) -> io::Result<()> {
    let established = tokio::time::timeout(Duration::from_secs(10), async {
        if inbound.read_u8().await? != 5 {
            return Err(failed("SOCKS version"));
        }
        let count = inbound.read_u8().await? as usize;
        let mut methods = vec![0; count];
        inbound.read_exact(&mut methods).await?;
        if !methods.contains(&0) {
            inbound.write_all(&[5, 255]).await?;
            return Err(failed("SOCKS authentication"));
        }
        inbound.write_all(&[5, 0]).await?;
        let mut header = [0; 4];
        inbound.read_exact(&mut header).await?;
        if header[..3] != [5, 1, 0] {
            return Err(failed("SOCKS command"));
        }
        let host = match header[3] {
            1 => {
                let mut ip = [0; 4];
                inbound.read_exact(&mut ip).await?;
                IpAddr::from(ip).to_string()
            }
            4 => {
                let mut ip = [0; 16];
                inbound.read_exact(&mut ip).await?;
                IpAddr::from(ip).to_string()
            }
            3 => {
                let len = inbound.read_u8().await? as usize;
                let mut value = vec![0; len];
                inbound.read_exact(&mut value).await?;
                String::from_utf8(value).map_err(failed)?
            }
            _ => return Err(failed("SOCKS address")),
        };
        let port = inbound.read_u16().await?;
        if host.is_empty()
            || host
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '/' | '\\' | '@' | '?' | '#'))
            || port == 0
        {
            return Err(failed("SOCKS target"));
        }
        if let Some(ready) = predial.take(&host, port) {
            if let Ok(Ok(stream)) = ready.await {
                return Ok(stream);
            }
        }
        let destinations = destinations(host, port, &dns, proxy.as_deref(), &connect_to).await?;
        dial(destinations, proxy.as_deref()).await
    })
    .await;
    let mut upstream = match established {
        Ok(Ok(stream)) => stream,
        _ => {
            let _ = inbound.write_all(&[5, 4, 0, 1, 0, 0, 0, 0, 0, 0]).await;
            return Err(failed("Upstream tunnel failed"));
        }
    };
    inbound.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).await?;
    tokio::io::copy_bidirectional(&mut inbound, &mut upstream).await?;
    Ok(())
}
fn credentials(url: &url::Url) -> io::Result<(String, String)> {
    let decode = |value: &str| {
        percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .map(|s| s.into_owned())
            .map_err(failed)
    };
    Ok((
        decode(url.username())?,
        decode(url.password().unwrap_or(""))?,
    ))
}
async fn connect(destination: &Destination, proxy: Option<&str>) -> io::Result<Tunnel> {
    let Some(proxy) = proxy else {
        return match destination {
            Destination::Socket(address) => Ok(Box::new(TcpStream::connect(address).await?)),
            Destination::Name(_, _) => Err(failed("Unresolved direct target")),
        };
    };
    let proxy = proxy_url(proxy).map_err(failed)?;
    let host = proxy.host_str().unwrap().trim_matches(['[', ']']);
    let port = proxy.port_or_known_default().unwrap();
    let socket = TcpStream::connect((host, port)).await?;
    if matches!(proxy.scheme(), "socks5" | "socks5h") {
        let target = match destination {
            Destination::Socket(address) => tokio_socks::TargetAddr::Ip(*address),
            Destination::Name(host, port) => {
                tokio_socks::TargetAddr::Domain(host.clone().into(), *port)
            }
        };
        let stream = if proxy.username().is_empty() && proxy.password().is_none() {
            tokio_socks::tcp::Socks5Stream::connect_with_socket(socket, target)
                .await
                .map_err(failed)?
        } else {
            let (username, password) = credentials(&proxy)?;
            tokio_socks::tcp::Socks5Stream::connect_with_password_and_socket(
                socket, target, &username, &password,
            )
            .await
            .map_err(failed)?
        };
        return Ok(Box::new(stream.into_inner()));
    }
    let mut stream: Tunnel = if proxy.scheme() == "https" {
        let mut builder =
            btls::ssl::SslConnector::bare_builder(btls::ssl::SslMethod::tls()).map_err(failed)?;
        let mut roots = btls::x509::store::X509StoreBuilder::new().map_err(failed)?;
        for root in webpki_root_certs::TLS_SERVER_ROOT_CERTS {
            roots
                .add_cert(btls::x509::X509::from_der(root.as_ref()).map_err(failed)?)
                .map_err(failed)?;
        }
        builder.set_cert_store(roots.build());
        builder.set_verify(btls::ssl::SslVerifyMode::PEER);
        let connector = builder.build();
        let ssl = connector
            .configure()
            .map_err(failed)?
            .into_ssl(host)
            .map_err(failed)?;
        let mut stream = tokio_btls::SslStream::new(ssl, socket).map_err(failed)?;
        Pin::new(&mut stream).connect().await.map_err(failed)?;
        Box::new(stream)
    } else {
        Box::new(socket)
    };
    let target = destination.authority();
    let mut header = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
    if !proxy.username().is_empty() || proxy.password().is_some() {
        let (username, password) = credentials(&proxy)?;
        header.push_str(&format!(
            "Proxy-Authorization: Basic {}\r\n",
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
        ));
    }
    header.push_str("\r\n");
    stream.write_all(header.as_bytes()).await?;
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        if response.len() >= 16384 {
            return Err(failed("Proxy response too large"));
        }
        response.push(stream.read_u8().await?);
    }
    let first = std::str::from_utf8(&response)
        .map_err(failed)?
        .lines()
        .next()
        .unwrap_or("");
    if !matches!(first.split_whitespace().nth(1), Some("200")) {
        return Err(failed("Proxy rejected CONNECT"));
    }
    Ok(stream)
}
