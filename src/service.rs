use crate::{
    engine::Engine,
    epoch_ms,
    network::{ConnectTo, Dns, NetworkConfig, RequestContext, Session, USER_AGENT},
    static_data::{origin, recognize, validate_url},
    Error, Result,
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::{atomic::Ordering, Arc, Mutex, Weak},
    time::{Duration, Instant},
};
struct Entry {
    session: Arc<Session>,
    touched: Instant,
    last_used_at_ms: u64,
}
#[derive(Default)]
struct Stats {
    total: u64,
    active: u64,
    success: u64,
    failed: u64,
    strict: bool,
    recent: VecDeque<Value>,
    history: VecDeque<Value>,
}
impl Stats {
    fn record(&mut self, record: Value) {
        self.recent.push_front(record.clone());
        self.recent.truncate(30);
        self.history.push_front(record);
        self.history.truncate(200);
    }
}
struct ActiveRequest<'a> {
    stats: &'a Mutex<Stats>,
    record: Value,
    started: Instant,
    completed: bool,
}
impl Drop for ActiveRequest<'_> {
    fn drop(&mut self) {
        let mut stats = self.stats.lock().unwrap();
        stats.active = stats.active.saturating_sub(1);
        if !self.completed {
            stats.failed += 1;
            self.record["ok"] = json!(false);
            self.record["code"] = json!("CANCELLED");
            self.record["totalMs"] = json!(self.started.elapsed().as_secs_f64() * 1000.);
            stats.record(self.record.clone());
        }
    }
}
pub struct Service {
    engine: Engine,
    network: NetworkConfig,
    dns: Dns,
    sessions: Mutex<HashMap<(String, String, Option<String>), Entry>>,
    cookie_contexts: Mutex<HashMap<(String, Option<String>), Weak<wreq::cookie::Jar>>>,
    stats: Mutex<Stats>,
    started: Instant,
}
fn bad(code: &str, message: &str) -> Error {
    let mut e = Error::new(code, message);
    e.status = 400;
    e
}
impl Service {
    pub fn new(engine: Engine) -> Result<Self> {
        Self::with_network(engine, NetworkConfig::default())
    }
    pub fn with_network(engine: Engine, network: NetworkConfig) -> Result<Self> {
        Ok(Self {
            engine,
            dns: Dns::new(&network)?,
            network,
            sessions: Mutex::default(),
            cookie_contexts: Mutex::default(),
            stats: Mutex::default(),
            started: Instant::now(),
        })
    }
    fn session(&self, url: &str, name: &str, proxy: Option<String>) -> Result<Arc<Session>> {
        let key = (origin(url)?, name.to_string(), proxy.clone());
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(entry) = sessions.get_mut(&key) {
            entry.touched = Instant::now();
            entry.last_used_at_ms = epoch_ms();
            return Ok(entry.session.clone());
        }
        if sessions.len() >= 32 {
            let oldest = sessions
                .iter()
                .filter(|(_, e)| Arc::strong_count(&e.session) == 1)
                .min_by_key(|(_, e)| e.touched)
                .map(|(k, _)| k.clone());
            if let Some(key) = oldest {
                sessions.remove(&key);
            } else {
                let mut e = Error::new("BUSY", "会话繁忙，请稍后再试");
                e.status = 429;
                return Err(e);
            }
        }
        // One logical client has a browser-style cookie jar. The jar enforces
        // Domain/Path/Secure; connection and verification state stay per origin.
        let mut contexts = self.cookie_contexts.lock().unwrap();
        contexts.retain(|_, jar| jar.strong_count() > 0);
        let context = (name.to_string(), proxy.clone());
        let cookies = contexts
            .get(&context)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let jar = Arc::new(wreq::cookie::Jar::default());
                contexts.insert(context, Arc::downgrade(&jar));
                jar
            });
        let session = Arc::new(Session::with_cookies(self.dns.clone(), proxy, cookies)?);
        sessions.insert(
            key,
            Entry {
                session: session.clone(),
                touched: Instant::now(),
                last_used_at_ms: epoch_ms(),
            },
        );
        Ok(session)
    }
    pub fn status(&self) -> Value {
        let mut session_details: Vec<_> = self.sessions.lock().unwrap().iter().map(|((origin, name, proxy), entry)| {
            json!({"session":name,"origin":origin,"hasProxy":proxy.is_some(),"verified":entry.session.verified.load(Ordering::Acquire),"busy":Arc::strong_count(&entry.session)>1,"lastUsedAtMs":entry.last_used_at_ms})
        }).collect();
        session_details.sort_by(|a, b| {
            b["lastUsedAtMs"]
                .as_u64()
                .cmp(&a["lastUsedAtMs"].as_u64())
                .then_with(|| a["session"].as_str().cmp(&b["session"].as_str()))
                .then_with(|| a["origin"].as_str().cmp(&b["origin"].as_str()))
                .then_with(|| a["hasProxy"].as_bool().cmp(&b["hasProxy"].as_bool()))
        });
        let sessions = session_details.len();
        let stats = self.stats.lock().unwrap();
        let mut system = sysinfo::System::new();
        let pid = sysinfo::Pid::from_u32(std::process::id());
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        let rss = system.process(pid).map(|p| p.memory());
        json!({"service":"cf-gateway-rust","version":crate::version::VERSION,"buildId":crate::version::build_id(),"mode":"pure-algorithm","precomputedEnvironmentProfile":false,"strictPureAlgorithmAcceptance":stats.strict,"uptimeSeconds":self.started.elapsed().as_secs(),"requests":stats.total,"active":stats.active,"success":stats.success,"failed":stats.failed,"runtime":{"language":"Rust","python":null,"rustEnabled":true,"rssBytes":rss,"sessions":sessions,"verifiedReadConcurrency":4,"responseCache":false,"defaultMode":"relay","networkMode":self.network.mode,"dnsMode":self.network.dns_mode(),"dohEnabled":self.network.doh,"dohUrl":if self.network.doh {Some(self.network.doh_url.as_str())}else{None}},"recent":stats.recent,"sessionDetails":session_details,"capabilities":{"nativeHttp":true,"managedChallenge":"native-rust-protocol","turnstile":"native-finite-recipes","jsdOneshot":"experimental"}})
    }
    pub fn history(&self) -> Value {
        let stats = self.stats.lock().unwrap();
        json!({"items":stats.history,"limit":200,"requests":stats.total,"persistent":false})
    }
    pub async fn fetch(&self, options: Value, analyze: bool, export: bool) -> Result<Value> {
        self.execute(options, analyze, export, false)
            .await
            .map(|(v, _)| v)
    }
    pub async fn image(&self, options: Value) -> Result<(Value, Vec<u8>)> {
        self.execute(options, false, false, true).await
    }
    async fn execute(
        &self,
        options: Value,
        analyze: bool,
        export: bool,
        image: bool,
    ) -> Result<(Value, Vec<u8>)> {
        let (network, connect_to) = self.network.request(&options)?;
        let dns = if network == self.network {
            self.dns.clone()
        } else {
            Dns::new(&network)?
        };
        crate::network::with_context(
            RequestContext::configured(connect_to.clone(), dns),
            self.execute_inner(options, analyze, export, image, network, connect_to),
        )
        .await
    }
    async fn execute_inner(
        &self,
        options: Value,
        analyze: bool,
        export: bool,
        image: bool,
        network: NetworkConfig,
        connect_to: ConnectTo,
    ) -> Result<(Value, Vec<u8>)> {
        let object = options
            .as_object()
            .ok_or_else(|| bad("INVALID_REQUEST", "请求必须为 JSON 对象"))?;
        let url = options["url"]
            .as_str()
            .ok_or_else(|| bad("INVALID_URL", "缺少网址"))?;
        let parsed = validate_url(url)?;
        let name = object
            .get("session")
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| bad("INVALID_SESSION", "会话名称必须是文字"))
            })
            .transpose()?
            .unwrap_or("default");
        if name.is_empty() || name.chars().count() > 128 {
            return Err(bad("INVALID_SESSION", "session 必须为 1～128 个字符"));
        }
        let timeout = object
            .get("timeoutMs")
            .map(|v| {
                v.as_f64()
                    .ok_or_else(|| bad("INVALID_TIMEOUT", "timeoutMs 必须为数字"))
            })
            .transpose()?
            .unwrap_or(90000.);
        if !(1000.0..=120000.0).contains(&timeout) {
            return Err(bad("INVALID_TIMEOUT", "timeoutMs 范围为 1000～120000"));
        }
        let proxy = network.proxy.clone();
        let session = self.session(url, name, proxy.clone())?;
        let start = Instant::now();
        let deadline = start + Duration::from_secs_f64(timeout / 1000.);
        let operation = if image {
            "image"
        } else if analyze {
            "analyze"
        } else if export {
            "solve"
        } else {
            "fetch"
        };
        let mut replay = json!({"url":url,"session":name,"timeoutMs":timeout});
        let credentials_omitted = proxy.as_ref().is_some_and(|p| {
            url::Url::parse(p).is_ok_and(|p| !p.username().is_empty() || p.password().is_some())
        });
        let mut replay_network =
            json!({"proxy":proxy,"doh":{"enabled":network.doh,"url":network.doh_url}});
        if credentials_omitted {
            replay_network.as_object_mut().unwrap().remove("proxy");
        }
        if !connect_to.is_empty() {
            replay_network["mapping"] = serde_json::to_value(&connect_to)?;
        }
        replay["network"] = replay_network;
        let mut record = json!({"id":uuid::Uuid::new_v4().to_string(),"url":url,"host":parsed.host_str(),"path":parsed.path(),"time":epoch_ms()/1000,"operation":operation,"networkMode":network.mode,"dnsMode":network.dns_mode(),"dohEnabled":network.doh,"dohUrl":if network.doh {Some(network.doh_url.as_str())}else{None},"request":replay,"proxyCredentialsOmitted":credentials_omitted});
        {
            let mut stats = self.stats.lock().unwrap();
            stats.total += 1;
            stats.active += 1;
        }
        let mut active = ActiveRequest {
            stats: &self.stats,
            record: record.clone(),
            started: start,
            completed: false,
        };
        let mut retry_first_error = None;
        let outcome = tokio::time::timeout_at(deadline.into(), async {
            let mut cached = None;
            loop {
                active.record["stage"] = json!("waitingForVerification");
                if session.verified.load(Ordering::Acquire) && !analyze && !export {
                    let read = session.gate.read().await;
                    if !session.verified.load(Ordering::Acquire) {
                        drop(read);
                        continue;
                    }
                    active.record["stage"] = json!("waitingForReadSlot");
                    let permit = session
                        .reads
                        .acquire()
                        .await
                        .map_err(|_| Error::new("BUSY", "会话已关闭"))?;
                    let queue_ms = start.elapsed().as_secs_f64() * 1000.;
                    let generation = session.generation.load(Ordering::Acquire);
                    active.record["stage"] = json!("fetchingUpstream");
                    let response = crate::engine::target(&session, url, deadline).await?;
                    let challenge = recognize(
                        &response.url,
                        response.status,
                        &response.headers,
                        &response.text,
                    )?;
                    if !matches!(
                        challenge.kind.as_str(),
                        "managed" | "blocked" | "jsd" | "embedded-turnstile"
                    ) && !(response.status == 403 && response.headers.contains_key("cf-ray"))
                    {
                        let outcome = crate::engine::Outcome {
                            response,
                            details: challenge.summary(),
                            solved: false,
                        };
                        return Ok((outcome, queue_ms));
                    }
                    cached = Some((response, generation));
                    drop(permit);
                    drop(read);
                }
                active.record["stage"] = json!("waitingForVerification");
                let exclusive = session.gate.write().await;
                // A previous verifier may have completed while this request
                // waited. Re-enter through the shared read path in that case.
                if session.verified.load(Ordering::Acquire)
                    && !analyze
                    && !export
                    && cached
                        .as_ref()
                        .is_none_or(|(_, g)| *g != session.generation.load(Ordering::Acquire))
                {
                    cached = None;
                    drop(exclusive);
                    continue;
                }
                let queue_ms = start.elapsed().as_secs_f64() * 1000.;
                let response = match cached.take() {
                    Some((r, g)) if g == session.generation.load(Ordering::Acquire) => Some(r),
                    _ => None,
                };
                session.generation.fetch_add(1, Ordering::Release);
                if !analyze {
                    session.verified.store(false, Ordering::Release);
                }
                active.record["stage"] = json!("verifyingAndFetching");
                let mut result = self
                    .engine
                    .run(session.clone(), url, deadline, analyze, response)
                    .await;
                // Never replay rejected protocol data. Allow one fresh round
                // only for the two observed rejection shapes, under
                // the original total deadline and exclusive gate.
                if !analyze
                    && (retry_continuation(&result, deadline)
                        || retry_final_target_get_403(&result, deadline))
                {
                    let first = result.as_ref().err().unwrap().reply();
                    retry_first_error = Some(first.clone());
                    result = self
                        .engine
                        .run(session.clone(), url, deadline, false, None)
                        .await;
                    match &mut result {
                        Ok(outcome) => {
                            outcome.details["freshRoundRetry"] =
                                json!({"attempts":2,"firstError":first})
                        }
                        Err(error) => {
                            if !error.details.is_object() {
                                error.details = json!({});
                            }
                            error.details["freshRoundRetry"] =
                                json!({"attempts":2,"firstError":first});
                        }
                    }
                }
                if !analyze {
                    session.verified.store(result.is_ok(), Ordering::Release);
                }
                return result.map(|r| (r, queue_ms));
            }
        })
        .await
        .unwrap_or_else(|_| {
            Err(total_deadline_error(
                &active.record["stage"],
                retry_first_error.as_ref(),
            ))
        });
        let outcome = outcome.and_then(|(outcome, queue)| {
            if image {
                image_type(&outcome.response)?;
            }
            Ok((outcome, queue))
        });
        let total_ms = start.elapsed().as_secs_f64() * 1000.;
        let result = match outcome {
            Ok((outcome, queue_ms)) => {
                record["ok"] = json!(true);
                record["status"] = json!(outcome.response.status);
                record["challenge"] = outcome.details["type"].clone();
                if let Some(retry) = outcome.details.get("freshRoundRetry") {
                    record["freshRoundRetry"] = retry.clone();
                }
                record["queueMs"] = json!(queue_ms);
                record["bytes"] = json!(outcome.response.content.len());
                record["route"] = json!(if proxy.is_some() { "proxy" } else { "direct" });
                let image_kind = if image {
                    Some(image_type(&outcome.response).unwrap())
                } else {
                    None
                };
                let bytes = if image {
                    outcome.response.content
                } else {
                    Vec::new()
                };
                let mut value = json!({"ok":true,"status":outcome.response.status,"url":outcome.response.url,"headers":outcome.response.headers,"body":outcome.response.text,"userAgent":USER_AGENT,"challenge":outcome.details,"contentCached":false,"precomputedEnvironmentProfile":false,"strictPureAlgorithmAcceptance":outcome.solved,"analyzed":analyze,"timings":{"queueMs":queue_ms,"totalMs":total_ms}});
                if let Some(kind) = image_kind {
                    value.as_object_mut().unwrap().remove("body");
                    value["imageType"] = json!(kind);
                    value["bytes"] = json!(bytes.len());
                }
                if export {
                    value.as_object_mut().unwrap().remove("body");
                    value["credentials"] = credentials(&session, value["url"].as_str().unwrap())?;
                }
                let mut stats = self.stats.lock().unwrap();
                stats.success += 1;
                stats.strict |= outcome.solved;
                Ok((value, bytes))
            }
            Err(error) => {
                let error = annotate_timeout(error, &active.record["stage"]);
                record["ok"] = json!(false);
                record["code"] = json!(error.code);
                record["error"] = json!(error.message);
                if serde_json::to_vec(&error.details).is_ok_and(|v| v.len() <= 8192) {
                    record["details"] = error.details.clone();
                }
                self.stats.lock().unwrap().failed += 1;
                let _ = self.engine.capture(
                    &serde_json::to_vec(&error.reply()).unwrap_or_default(),
                    ".error.json",
                );
                Err(error)
            }
        };
        record["totalMs"] = json!(total_ms);
        let mut stats = self.stats.lock().unwrap();
        stats.record(record);
        active.completed = true;
        result
    }
}
fn retry_continuation(result: &Result<crate::engine::Outcome>, deadline: Instant) -> bool {
    Instant::now() < deadline
        && result.as_ref().err().is_some_and(|e| {
            e.code == "MANAGED_CONTINUATION_REJECTED"
                && e.details["status"] == 400
                && matches!(
                    e.details["stage"].as_str(),
                    Some("frame-tasks" | "frame-work")
                )
        })
}
fn retry_final_target_get_403(result: &Result<crate::engine::Outcome>, deadline: Instant) -> bool {
    Instant::now() < deadline
        && result.as_ref().err().is_some_and(|e| {
            e.code == "NATIVE_CHALLENGE_NOT_SOLVED"
                && e.details["stage"] == "final-target"
                && e.details["status"] == 403
                && e.details["challenge"] == "managed"
                && e.details["details"]["firstFlow"]["finalNavigation"]["method"] == "GET"
        })
}
fn total_deadline_error(stage: &Value, first_error: Option<&Value>) -> Error {
    let mut error = Error::new("TIMEOUT", "请求总时限已用完");
    error.status = 504;
    error.details = json!({"stage":stage});
    if let Some(first) = first_error {
        error.details["freshRoundRetry"] = json!({"attempts":2,"firstError":first});
    }
    error
}
fn annotate_timeout(mut error: Error, stage: &Value) -> Error {
    if error.code == "TIMEOUT" && error.details.is_null() {
        error.details = json!({"stage":stage,"source":"network"});
    }
    error
}
fn image_type(response: &crate::network::HttpResponse) -> Result<&'static str> {
    let b = &response.content;
    let kind = if b.starts_with(b"\x89PNG\r\n\x1a\n") {
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
    };
    if response.status == 200 {
        if let Some(kind) = kind {
            return Ok(kind);
        }
    }
    let mut error = Error::new(
        if response.status == 404 {
            "IMAGE_NOT_FOUND"
        } else {
            "IMAGE_UPSTREAM_ERROR"
        },
        if response.status == 404 {
            "网站返回 404，未提供该封面"
        } else {
            "未取得有效的 PNG/JPEG/GIF/WebP/AVIF 图片"
        },
    );
    error.status = if response.status == 404 { 404 } else { 502 };
    error.details =
        json!({"status":response.status,"contentType":response.headers.get("content-type")});
    Err(error)
}
fn credentials(s: &Session, url: &str) -> Result<Value> {
    let parsed = validate_url(url)?;
    let cookies: Vec<_> = s.cookies.matches(url).collect();
    let header = cookies
        .iter()
        .map(|c| format!("{}={}", c.name(), c.value()))
        .collect::<Vec<_>>()
        .join("; ");
    let values:Vec<_>=cookies.iter().map(|c|json!({"name":c.name(),"value":c.value(),"domain":c.domain().unwrap_or(parsed.host_str().unwrap()),"hostOnly":c.domain().is_none(),"path":c.path().unwrap_or("/"),"secure":c.secure(),"httpOnly":c.http_only(),"expiresAt":c.expires().map(|v|v.duration_since(std::time::UNIX_EPOCH).map(|d|d.as_secs()).unwrap_or(0))})).collect();
    Ok(
        json!({"url":url,"origin":origin(url)?,"exportedAt":epoch_ms()/1000,"userAgent":USER_AGENT,"headers":{"user-agent":USER_AGENT,"Cookie":header},"cookieHeader":header,"cookies":values,"clearancePresent":cookies.iter().any(|c|c.name()=="cf_clearance"),"clientCompatibilityVerified":false}),
    )
}
#[cfg(test)]
mod regression {
    use super::*;
    #[test]
    fn fresh_round_retry_is_limited_to_the_observed_continuation_rejection_and_deadline() {
        let future = Instant::now() + Duration::from_secs(1);
        let make = |code: &str, status: u16, stage: &str| -> Result<crate::engine::Outcome> {
            let mut e = Error::new(code, "fixture");
            e.details = json!({"status":status,"stage":stage});
            Err(e)
        };
        assert!(retry_continuation(
            &make("MANAGED_CONTINUATION_REJECTED", 400, "frame-tasks"),
            future
        ));
        assert!(retry_continuation(
            &make("MANAGED_CONTINUATION_REJECTED", 400, "frame-work"),
            future
        ));
        assert!(!retry_continuation(
            &make("MANAGED_CONTINUATION_REJECTED", 400, "frame-first"),
            future
        ));
        assert!(!retry_continuation(
            &make("MANAGED_CONTINUATION_REJECTED", 429, "frame-work"),
            future
        ));
        assert!(!retry_continuation(
            &make("CHILD_CHALLENGE_REJECTED", 400, "frame-work"),
            future
        ));
        assert!(!retry_continuation(
            &make("MANAGED_CONTINUATION_REJECTED", 400, "frame-tasks"),
            Instant::now()
        ));
        assert!(!retry_continuation(
            &make("MANAGED_CONTINUATION_REJECTED", 403, "frame-tasks"),
            future
        ));
        assert!(!retry_continuation(
            &make("MANAGED_CONTINUATION_REJECTED", 400, "parent"),
            future
        ));
        assert!(!retry_continuation(
            &make("UPSTREAM_ERROR", 400, "frame-tasks"),
            future
        ));
    }
    #[test]
    fn outer_deadline_preserves_the_first_continuation_rejection() {
        let first = json!({"code":"MANAGED_CONTINUATION_REJECTED","details":{"stage":"frame-work","status":400}});
        let error = total_deadline_error(&json!("verifyingAndFetching"), Some(&first));
        assert_eq!(error.code, "TIMEOUT");
        assert_eq!(error.status, 504);
        assert_eq!(error.details["stage"], "verifyingAndFetching");
        assert_eq!(error.details["freshRoundRetry"]["attempts"], 2);
        assert_eq!(error.details["freshRoundRetry"]["firstError"], first);
        let waiting = total_deadline_error(&json!("waitingForReadSlot"), None);
        assert_eq!(waiting.details["stage"], "waitingForReadSlot");
        assert!(waiting.details.get("freshRoundRetry").is_none());
    }
    #[test]
    fn network_timeout_preserves_the_active_stage() {
        let error = annotate_timeout(
            Error::new("TIMEOUT", "fixture"),
            &json!("verifyingAndFetching"),
        );
        assert_eq!(error.details["stage"], "verifyingAndFetching");
        assert_eq!(error.details["source"], "network");
        let mut existing = Error::new("TIMEOUT", "fixture");
        existing.details = json!({"stage":"frame-work"});
        let existing = annotate_timeout(existing, &json!("verifyingAndFetching"));
        assert_eq!(existing.details["stage"], "frame-work");
    }
    #[test]
    fn final_target_retry_requires_managed_get_fallback_403() {
        let make = |code: &str, status: u16, challenge: &str, method: &str| {
            let mut e = Error::new(code, "fixture");
            e.details = json!({
                "stage":"final-target",
                "status":status,
                "challenge":challenge,
                "details":{"firstFlow":{"finalNavigation":{"method":method}}}
            });
            Err(e)
        };
        let future = Instant::now() + Duration::from_secs(1);
        assert!(retry_final_target_get_403(
            &make("NATIVE_CHALLENGE_NOT_SOLVED", 403, "managed", "GET"),
            future
        ));
        assert!(!retry_final_target_get_403(
            &make("NATIVE_CHALLENGE_NOT_SOLVED", 403, "managed", "POST"),
            future
        ));
        assert!(!retry_final_target_get_403(
            &make("NATIVE_CHALLENGE_NOT_SOLVED", 403, "blocked", "GET"),
            future
        ));
        assert!(!retry_final_target_get_403(
            &make("NATIVE_CHALLENGE_NOT_SOLVED", 429, "managed", "GET"),
            future
        ));
        assert!(!retry_final_target_get_403(
            &make("NATIVE_CHALLENGE_NOT_SOLVED", 403, "managed", "GET"),
            Instant::now()
        ));
    }
    #[test]
    fn history_and_overview_have_independent_bounds() {
        let mut stats = Stats::default();
        for i in 0..250 {
            stats.record(json!({"id":i}));
        }
        assert_eq!(stats.history.len(), 200);
        assert_eq!(stats.recent.len(), 30);
        assert_eq!(stats.history[0]["id"], 249);
        assert_eq!(stats.history[199]["id"], 50);
        assert_eq!(stats.recent[29]["id"], 220);
    }
    fn service() -> Service {
        Service::new(Engine::default()).unwrap()
    }
    #[test]
    fn credentials_enforce_domain_path_secure_expiry_and_host_only_scope() {
        let s = Session::new(Dns::new(&NetworkConfig::default()).unwrap(), None).unwrap();
        let url = "https://example.org/";
        for cookie in [
            "cf_clearance=domain; Domain=.example.org; Path=/; Secure; HttpOnly",
            "host=value; Path=/",
            "chapter=value; Domain=.example.org; Path=/chapter",
            "expired=value; Max-Age=0; Path=/",
        ] {
            s.cookies.add(cookie, url);
        }
        s.cookies.add(
            "foreign=value; Path=/",
            "https://challenges.cloudflare.com/",
        );
        let names = |url| {
            credentials(&s, url).unwrap()["cookies"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["name"].as_str().unwrap().to_string())
                .collect::<std::collections::HashSet<_>>()
        };
        assert_eq!(
            names("https://example.org/chapter/1"),
            ["cf_clearance", "host", "chapter"].map(String::from).into()
        );
        assert_eq!(
            names("https://sub.example.org/"),
            ["cf_clearance"].map(String::from).into()
        );
        assert_eq!(
            names("http://example.org/chapterish"),
            ["host"].map(String::from).into()
        );
        assert!(names("https://example.net/").is_empty());
        let a = credentials(&s, url).unwrap();
        let b = credentials(&s, url).unwrap();
        assert_eq!(a["cookieHeader"], b["cookieHeader"]);
        assert_eq!(a["headers"]["Cookie"], a["cookieHeader"]);
        assert_eq!(a["clientCompatibilityVerified"], false);
        assert_eq!(
            a["cookies"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == "host")
                .unwrap()["hostOnly"],
            true
        );
    }
    #[test]
    fn cookie_context_shares_valid_domain_cookies_but_keeps_origins_and_clients_isolated() {
        let s = service();
        let main = s
            .session("https://www.example.org/", "reader", None)
            .unwrap();
        let image = s
            .session("https://image.example.org/", "reader", None)
            .unwrap();
        let unrelated = s.session("https://example.net/", "reader", None).unwrap();
        assert!(Arc::ptr_eq(&main.cookies, &image.cookies));
        assert!(!Arc::ptr_eq(&main, &image));
        main.cookies.add(
            "parent=shared; Domain=.example.org; Path=/; Secure",
            "https://www.example.org/",
        );
        main.cookies
            .add("host=private; Path=/; Secure", "https://www.example.org/");
        main.cookies.add(
            "path=restricted; Domain=.example.org; Path=/account; Secure",
            "https://www.example.org/account",
        );
        let names = |url: &str| {
            main.cookies
                .matches(url)
                .map(|c| c.name().to_string())
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(
            names("https://image.example.org/"),
            ["parent".to_string()].into()
        );
        assert!(names("https://example.net/").is_empty());
        assert!(names("http://image.example.org/").is_empty());
        assert!(Arc::ptr_eq(&main.cookies, &unrelated.cookies));
        main.verified.store(true, Ordering::Release);
        main.generation.store(3, Ordering::Release);
        assert!(!image.verified.load(Ordering::Acquire));
        assert_eq!(image.generation.load(Ordering::Acquire), 0);
        let other = s
            .session("https://image.example.org/", "other", None)
            .unwrap();
        let proxy = s
            .session(
                "https://image.example.org/",
                "reader",
                Some("http://127.0.0.1:9999".into()),
            )
            .unwrap();
        assert!(!Arc::ptr_eq(&main.cookies, &other.cookies));
        assert!(!Arc::ptr_eq(&main.cookies, &proxy.cookies));
        assert!(other
            .cookies
            .matches("https://image.example.org/")
            .next()
            .is_none());
    }
    #[test]
    fn inventory_reports_live_origin_sessions_without_cookies_or_proxy_credentials() {
        let s = service();
        assert_eq!(s.status()["sessionDetails"], json!([]));
        let main = s
            .session("https://www.example.org/", "phone-A", None)
            .unwrap();
        let image = s
            .session("https://image.example.org/", "phone-A", None)
            .unwrap();
        main.verified.store(true, Ordering::Release);
        main.cookies.add(
            "login=private-cookie-value; Path=/",
            "https://www.example.org/",
        );
        s.session(
            "https://example.org/",
            "phone-B",
            Some("http://proxy-user:proxy-password@127.0.0.1:9999".into()),
        )
        .unwrap();
        let status = s.status();
        let rows = status["sessionDetails"].as_array().unwrap();
        assert_eq!(status["runtime"]["sessions"], rows.len());
        assert_eq!(rows.len(), 3);
        let item = rows
            .iter()
            .find(|v| v["origin"] == "https://www.example.org")
            .unwrap();
        assert_eq!(item["session"], "phone-A");
        assert_eq!(item["verified"], true);
        assert_eq!(item["busy"], true);
        let timestamp = item["lastUsedAtMs"].as_u64().unwrap();
        assert!(timestamp <= epoch_ms() && epoch_ms() - timestamp < 5000);
        drop(main);
        drop(image);
        assert!(s.status()["sessionDetails"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["busy"] == false));
        assert!(rows
            .iter()
            .any(|v| v["session"] == "phone-B" && v["hasProxy"] == true));
        let text = status.to_string();
        for secret in [
            "private-cookie-value",
            "proxy-user",
            "proxy-password",
            "9999",
        ] {
            assert!(!text.contains(secret));
        }
    }
    #[test]
    fn session_identity_capacity_and_active_eviction_are_bounded() {
        let s = service();
        let a = s
            .session("https://example.org/first", "reader", None)
            .unwrap();
        assert!(Arc::ptr_eq(
            &a,
            &s.session("https://example.org/second", "reader", None)
                .unwrap()
        ));
        assert!(!Arc::ptr_eq(
            &a,
            &s.session("https://example.org/", "other", None).unwrap()
        ));
        assert!(!Arc::ptr_eq(
            &a,
            &s.session("https://other.example/", "reader", None).unwrap()
        ));
        drop(a);
        let all = (0..32)
            .map(|i| {
                s.session("https://example.org/", &format!("active-{i}"), None)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let e = s
            .session("https://example.org/", "excess", None)
            .err()
            .unwrap();
        assert_eq!(e.status, 429);
        assert_eq!(s.sessions.lock().unwrap().len(), 32);
        drop(all);
        assert!(s
            .session("https://example.org/", "after-release", None)
            .is_ok());
        assert_eq!(s.sessions.lock().unwrap().len(), 32);
    }
    #[tokio::test]
    async fn cancelling_renewal_invalidates_the_generation_and_releases_exclusive_access() {
        use axum::{body::Body, extract::Request, response::IntoResponse, Router};
        use std::sync::atomic::AtomicBool;
        let challenge = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(tokio::sync::Notify::new());
        let flag = challenge.clone();
        let notify = entered.clone();
        let router=Router::new().fallback(move|request:Request|{let flag=flag.clone();let notify=notify.clone();async move{
            if request.uri().path().starts_with("/cdn-cgi/"){notify.notify_one();std::future::pending::<()>().await;}
            if flag.load(Ordering::Acquire){(axum::http::StatusCode::FORBIDDEN,[("content-type","text/html")],Body::from("<html><body><script>window.__CF$cv$params={r:'0123456789abcdef',t:'MTc5MjQ4MDAwMA=='};</script></body></html>")).into_response()}
            else{(axum::http::StatusCode::OK,[("content-type","text/html")],Body::from("<html><body>Fresh document</body></html>")).into_response()}
        }});
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/page", listener.local_addr().unwrap());
        struct Stop(tokio::task::JoinHandle<()>);
        impl Drop for Stop {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _server = Stop(tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap()
        }));
        let s = Arc::new(service());
        s.fetch(json!({"url":url}), false, false).await.unwrap();
        let session = s.session(&url, "default", None).unwrap();
        assert!(session.verified.load(Ordering::Acquire));
        let generation = session.generation.load(Ordering::Acquire);
        challenge.store(true, Ordering::Release);
        let other = s.clone();
        let target = url.clone();
        let task =
            tokio::spawn(async move { other.fetch(json!({"url":target}), false, false).await });
        tokio::time::timeout(Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        task.abort();
        let _ = task.await;
        assert!(!session.verified.load(Ordering::Acquire));
        assert!(session.generation.load(Ordering::Acquire) > generation);
        assert!(session.gate.try_write().is_ok());
        assert_eq!(s.status()["active"], 0);
        challenge.store(false, Ordering::Release);
        assert!(s.fetch(json!({"url":url}), false, false).await.is_ok());
        assert!(session.verified.load(Ordering::Acquire));
    }
}
