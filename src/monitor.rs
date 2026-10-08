//! Bounded diagnostic history, independent of the fresh upstream data path.
use crate::{api, api::Metrics};
use axum::{extract::connect_info::Connected, serve::IncomingStream};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
};

const HISTORY_LIMIT: usize = 200;
const DETAIL_BUDGET: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct Monitor(Arc<Mutex<Counters>>);
struct Counters {
    started_ms: u64,
    tcp_total: u64,
    tcp_active: u64,
    api_total: u64,
    api_active: u64,
    succeeded: u64,
    failed: u64,
    elapsed_ms: u64,
    upstream_total: u64,
    response_bytes: u64,
    detail_bytes: usize,
    operations: HashMap<String, u64>,
    requests: VecDeque<RequestRecord>,
    connections: VecDeque<ConnectionRecord>,
    minutes: VecDeque<(u64, u64, u64)>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionRecord {
    pub id: u64,
    pub peer: String,
    pub opened_ms: u64,
    pub closed_ms: Option<u64>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRecord {
    pub id: u64,
    pub connection_id: u64,
    pub peer: String,
    pub client: String,
    pub started_ms: u64,
    pub operation: String,
    pub summary: String,
    pub status: u16,
    pub elapsed_ms: u64,
    pub upstream_count: usize,
    pub response_bytes: usize,
    pub has_detail: bool,
    pub error: Option<String>,
    #[serde(skip)]
    pub detail: Option<Arc<RequestDetail>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDetail {
    pub request: Value,
    pub response: String,
    pub metrics: Metrics,
    pub upstream: Vec<api::UpstreamTrace>,
}
impl RequestDetail {
    fn bytes(&self) -> usize {
        self.request.to_string().len()
            + self.response.len()
            + self.upstream.iter().map(|t| t.body.len()).sum::<usize>()
    }
}
#[derive(Clone, Debug)]
pub struct Peer {
    pub address: SocketAddr,
    pub connection_id: u64,
}
pub struct RequestGuard {
    monitor: Monitor,
    record: RequestRecord,
    start: Instant,
    done: bool,
}
impl Monitor {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Counters {
            started_ms: api::now(),
            tcp_total: 0,
            tcp_active: 0,
            api_total: 0,
            api_active: 0,
            succeeded: 0,
            failed: 0,
            elapsed_ms: 0,
            upstream_total: 0,
            response_bytes: 0,
            detail_bytes: 0,
            operations: HashMap::new(),
            requests: VecDeque::new(),
            connections: VecDeque::new(),
            minutes: VecDeque::new(),
        })))
    }
    fn opened(&self, peer: SocketAddr) -> u64 {
        let mut c = self.0.lock().unwrap();
        c.tcp_total += 1;
        c.tcp_active += 1;
        let id = c.tcp_total;
        c.connections.push_front(ConnectionRecord {
            id,
            peer: peer.to_string(),
            opened_ms: api::now(),
            closed_ms: None,
        });
        c.connections.truncate(40);
        id
    }
    fn closed(&self, id: u64) {
        let mut c = self.0.lock().unwrap();
        c.tcp_active = c.tcp_active.saturating_sub(1);
        if let Some(r) = c.connections.iter_mut().find(|r| r.id == id) {
            r.closed_ms = Some(api::now());
        }
    }
    pub fn begin(&self, peer: Peer, client: String) -> RequestGuard {
        let mut c = self.0.lock().unwrap();
        c.api_total += 1;
        c.api_active += 1;
        RequestGuard {
            monitor: self.clone(),
            start: Instant::now(),
            done: false,
            record: RequestRecord {
                id: c.api_total,
                connection_id: peer.connection_id,
                peer: peer.address.to_string(),
                client,
                started_ms: api::now(),
                operation: "invalid".into(),
                summary: String::new(),
                status: 0,
                elapsed_ms: 0,
                upstream_count: 0,
                response_bytes: 0,
                has_detail: false,
                error: None,
                detail: None,
            },
        }
    }
    fn complete(&self, mut record: RequestRecord) {
        let mut c = self.0.lock().unwrap();
        c.api_active = c.api_active.saturating_sub(1);
        let success = (200..300).contains(&record.status);
        if success {
            c.succeeded += 1;
        } else {
            c.failed += 1;
        }
        c.elapsed_ms += record.elapsed_ms;
        c.upstream_total += record.upstream_count as u64;
        c.response_bytes += record.response_bytes as u64;
        *c.operations.entry(record.operation.clone()).or_default() += 1;
        let minute = record.started_ms / 60_000 * 60_000;
        if let Some(bucket) = c.minutes.iter_mut().find(|b| b.0 == minute) {
            bucket.1 += 1;
            bucket.2 += u64::from(!success);
        } else {
            c.minutes.push_back((minute, 1, u64::from(!success)));
        }
        c.minutes.retain(|b| b.0 + 60 * 60_000 > api::now());
        let bytes = record.detail.as_ref().map(|d| d.bytes()).unwrap_or(0);
        if bytes > DETAIL_BUDGET {
            record.detail = None;
            record.has_detail = false;
        } else {
            c.detail_bytes += bytes;
        }
        c.requests.push_front(record);
        while c.detail_bytes > DETAIL_BUDGET {
            if let Some(r) = c.requests.iter_mut().rev().find(|r| r.detail.is_some()) {
                c.detail_bytes -= r.detail.take().unwrap().bytes();
            } else {
                break;
            }
        }
        while c.requests.len() > HISTORY_LIMIT {
            if let Some(r) = c.requests.pop_back() {
                c.detail_bytes -= r.detail.as_ref().map(|d| d.bytes()).unwrap_or(0);
            }
        }
        for r in &mut c.requests {
            r.has_detail = r.detail.is_some();
        }
    }
    pub fn snapshot(&self) -> Value {
        let c = self.0.lock().unwrap();
        let completed = c.succeeded + c.failed;
        let mut recent_ms: Vec<_> = c.requests.iter().map(|r| r.elapsed_ms).collect();
        recent_ms.sort_unstable();
        let p50 = recent_ms.get(recent_ms.len() / 2).copied().unwrap_or(0);
        let p95 = recent_ms
            .get((recent_ms.len() * 95).div_ceil(100).saturating_sub(1))
            .copied()
            .unwrap_or(0);
        let now = api::now();
        let minute = now / 60_000 * 60_000;
        let trend: Vec<_> = (0..15).rev().map(|i| {
            let time = minute.saturating_sub(i*60_000);
            let b = c.minutes.iter().find(|b| b.0 == time);
            json!({"time":time,"count":b.map(|b|b.1).unwrap_or(0),"failed":b.map(|b|b.2).unwrap_or(0)})
        }).collect();
        json!({"startedMs":c.started_ms,"uptimeMs":now-c.started_ms,"tcpTotal":c.tcp_total,
            "tcpActive":c.tcp_active,"apiTotal":c.api_total,"apiActive":c.api_active,
            "succeeded":c.succeeded,"failed":c.failed,"successRate":if completed==0 {100.0} else {c.succeeded as f64/completed as f64*100.0},
            "averageMs":if completed==0 {0} else {c.elapsed_ms/completed},"p50Ms":p50,"p95Ms":p95,
            "upstreamTotal":c.upstream_total,"responseBytes":c.response_bytes,"detailBytes":c.detail_bytes,
            "operations":c.operations,"requests":c.requests,"connections":c.connections,"trend":trend,
            "historyLimit":HISTORY_LIMIT,"detailBudget":DETAIL_BUDGET})
    }
    pub fn detail(&self, id: u64) -> Option<(RequestRecord, Arc<RequestDetail>)> {
        let c = self.0.lock().unwrap();
        let r = c.requests.iter().find(|r| r.id == id)?;
        Some((r.clone(), r.detail.clone()?))
    }
}
impl RequestGuard {
    pub fn id(&self) -> u64 {
        self.record.id
    }
    pub fn finish(
        mut self,
        status: u16,
        operation: String,
        summary: String,
        error: Option<String>,
        detail: Option<Arc<RequestDetail>>,
    ) {
        self.record.status = status;
        self.record.operation = operation;
        self.record.summary = summary;
        self.record.error = error;
        self.record.elapsed_ms = self.start.elapsed().as_millis() as u64;
        if let Some(d) = &detail {
            self.record.upstream_count = d.metrics.requests.len();
            self.record.response_bytes = d.response.len();
        }
        self.record.detail = detail;
        self.done = true;
        self.monitor.complete(self.record.clone());
    }
}
impl Drop for RequestGuard {
    fn drop(&mut self) {
        if !self.done {
            self.record.status = 499;
            self.record.error = Some("客户端取消请求".into());
            self.record.elapsed_ms = self.start.elapsed().as_millis() as u64;
            self.monitor.complete(self.record.clone());
        }
    }
}

pub fn redact(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                let key = k.to_ascii_lowercase().replace('_', "").replace('-', "");
                if matches!(
                    key.as_str(),
                    "cookie"
                        | "sessioncookie"
                        | "sessionkey"
                        | "sessionid"
                        | "sessionidss"
                        | "sidguard"
                        | "tttoken"
                        | "token"
                        | "authorization"
                        | "password"
                        | "sessionsign"
                        | "key"
                ) {
                    *v = json!("[已隐藏凭证]");
                } else {
                    redact(v);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(redact),
        _ => (),
    }
}

pub struct TrackedListener {
    pub listener: TcpListener,
    pub monitor: Monitor,
}
pub struct TrackedStream {
    stream: TcpStream,
    monitor: Monitor,
    id: u64,
}
impl axum::serve::Listener for TrackedListener {
    type Io = TrackedStream;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.listener.accept().await {
                Ok((stream, address)) => {
                    let _ = stream.set_nodelay(true);
                    let id = self.monitor.opened(address);
                    return (
                        TrackedStream {
                            stream,
                            monitor: self.monitor.clone(),
                            id,
                        },
                        address,
                    );
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    }
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }
}
impl Connected<IncomingStream<'_, TrackedListener>> for Peer {
    fn connect_info(stream: IncomingStream<'_, TrackedListener>) -> Self {
        Self {
            address: *stream.remote_addr(),
            connection_id: stream.io().id,
        }
    }
}
impl Drop for TrackedStream {
    fn drop(&mut self) {
        self.monitor.closed(self.id);
    }
}
impl AsyncRead for TrackedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}
impl AsyncWrite for TrackedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
    fn is_write_vectored(&self) -> bool {
        self.stream.is_write_vectored()
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write_vectored(cx, bufs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn telemetry_counts_failures_and_redacts_credentials() {
        let m = Monitor::new();
        let peer = Peer {
            address: "127.0.0.1:1".parse().unwrap(),
            connection_id: 1,
        };
        m.begin(peer.clone(), "Web".into())
            .finish(200, "detail".into(), String::new(), None, None);
        drop(m.begin(peer, "Web".into()));
        let s = m.snapshot();
        assert_eq!(s["apiTotal"], 2);
        assert_eq!(s["apiActive"], 0);
        assert_eq!(s["failed"], 1);
        let mut v = json!({"account":{"sessionCookie":"secret","ttToken":"secret","uid":"123"},"book":{"title":"a","chapter_word_number":100}});
        redact(&mut v);
        assert_eq!(v["account"]["sessionCookie"], "[已隐藏凭证]");
        assert_eq!(v["book"]["chapter_word_number"], 100);
    }
}
