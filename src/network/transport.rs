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
}
struct Bridge {
    task: JoinHandle<()>,
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Transport {
    pub async fn new(
        dns: Dns,
        cookies: Arc<wreq::cookie::Jar>,
        proxy: Option<String>,
        connect_to: ConnectTo,
    ) -> Result<Self> {
        if proxy.is_none() && connect_to.is_empty() {
            return Ok(Self {
                client: client(dns, cookies, None)?,
                _bridge: None,
            });
        }
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let local = format!("socks5h://{}", listener.local_addr()?);
        let client = client(dns.clone(), cookies, Some(&local))?;
        let task = tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    connection=listener.accept()=>{
                        let Ok((stream,_))=connection else{break};
                        let Ok(permit)=dns.connections.clone().try_acquire_owned() else{drop(stream);continue};
                        let dns=dns.clone();let proxy=proxy.clone();let connect_to=connect_to.clone();
                        tasks.spawn(async move {let _permit=permit;let _=serve(stream,dns,proxy,connect_to).await;});
                    }
                    _=tasks.join_next(),if !tasks.is_empty()=>{}
                }
            }
        });
        Ok(Self {
            client,
            _bridge: Some(Bridge { task }),
        })
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
async fn serve(
    mut inbound: TcpStream,
    dns: Dns,
    proxy: Option<String>,
    connect_to: ConnectTo,
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
        let destinations = if let Some(target) = connect_to.target(&host, port) {
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
        };
        let mut last = failed("No target addresses");
        for destination in destinations {
            match connect(&destination, proxy.as_deref()).await {
                Ok(stream) => return Ok(stream),
                Err(error) => last = error,
            }
        }
        Err(last)
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
