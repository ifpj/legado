//! End-to-end native HTTP protocol, with independent target-page verification.
use crate::{
    array,
    catalog::Catalog,
    completion::{self, Navigation},
    data::{PendingSealed, Plan, Response},
    epoch_ms,
    html::Document,
    models::{self, Context, Results},
    network::{HttpResponse, Session},
    re, require,
    static_data::{self, origin},
    string, uint,
    wire::{self, Cipher},
    Error, Result,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
#[derive(Clone, Default)]
pub struct Engine {
    pub capture_dir: Option<PathBuf>,
}
pub struct Outcome {
    pub response: HttpResponse,
    pub details: Value,
    pub solved: bool,
}
fn merge(target: &mut Value, source: &Value) {
    target
        .as_object_mut()
        .unwrap()
        .extend(source.as_object().unwrap().clone());
}
fn copy(target: &mut Value, source: &Value, names: &[&str]) {
    for name in names {
        target[*name] = source[*name].clone();
    }
}
fn remove(target: &mut Value, names: &[&str]) {
    for name in names {
        target.as_object_mut().unwrap().remove(*name);
    }
}
fn protocol_error(code: &str, message: &str, status: u16, details: Value) -> Error {
    Error {
        code: code.into(),
        message: message.into(),
        status,
        details,
    }
}
pub fn initial(config: &Value, locale: &str) -> Value {
    json!({"TawNq4":config["cType"],"QZAb2":config["cvId"],"UqwKl5":0,"gRYZ0":0,"Pzgs5":"OoGu5","URVD0":0,"shUar7":0,"ghqno0":0,"pKaqh3":1,"RUtA1":config["cITimeS"],"MsSIK5":config["md"],"YVDgD5":config["cTplC"]==1,"nTlR0":{"frLR7":0,"WxIBU1":0,"rRfi2":0,"XWXu1":0,"Mfpwg0":0,"GCgUo2":0,"GkCb5":0,"gmGY7":0},"CtUv5":false,"TKcX8":false,"iWlWL9":"iDQN3","CAHKN0":"","vvfHL3":"[]","bkOl5":0,"gfOgY8":"tQth1","Bxtcq6":locale,"lffN9":"qCZq4","BuWQ0":{"LQfqz0":"chejv1"}})
}
/// The managed component is always loaded from this origin.
const FRAME_ORIGIN: &str = "https://challenges.cloudflare.com/";
/// Component task hosts seen in earlier verifications. They are connection
/// hints only: every task URL is still read from the current program, and an
/// unused early connection is dropped without sending TLS or HTTP data.
static TASK_HOSTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
fn learn_task_hosts(tasks: &[crate::http_tasks::Task]) {
    let mut hosts = TASK_HOSTS.lock().unwrap();
    for task in tasks {
        let Some(host) = url::Url::parse(&task.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
        else {
            continue;
        };
        if host != "challenges.cloudflare.com"
            && re(r"^[a-z0-9-]{1,63}\.challenges\.cloudflare\.com$").is_match(&host)
        {
            hosts.retain(|h| *h != host);
            hosts.insert(0, host);
            hosts.truncate(2);
        }
    }
}
fn learned_task_hosts() -> Vec<String> {
    TASK_HOSTS.lock().unwrap().clone()
}
async fn plan(
    source: Document,
    deadline: Instant,
    defer: bool,
) -> Result<(Arc<Plan>, Option<PendingSealed>)> {
    // Parse once, then read the current key from the original tree while
    // literals are decoded. The first response still waits for the check.
    let early = if defer && source.contains("atob") && source.contains("fromCharCode.apply") {
        let parsed = source.clone();
        crate::cpu::run(deadline, move || parsed.syntax().map(|_| ())).await?;
        let original = source.clone();
        Some(tokio::spawn(crate::cpu::run(deadline, move || {
            crate::compiler::current_sealed_original(&original, original.syntax()?)
        })))
    } else {
        None
    };
    let early = crate::data::EarlySealed::from(early);
    let planned = crate::cpu::run(deadline, move || {
        (if defer {
            let tree = if source.contains("atob") && source.contains("fromCharCode.apply") {
                Some(source.syntax()?)
            } else {
                None
            };
            Plan::prepare_parent(&source, tree)
        } else {
            Plan::analyze(&source, false).map(|p| (p, None))
        })
        .map(|(p, v)| (Arc::new(p), v))
    })
    .await?;
    Ok((
        planned.0,
        planned.1.map(|pending| pending.with_early(early)),
    ))
}
async fn validated_response<T>(
    request: impl std::future::Future<Output = Result<T>>,
    validate: impl std::future::Future<Output = Result<()>>,
) -> Result<T> {
    let (response, ()) = tokio::try_join!(request, validate)?;
    Ok(response)
}
pub(crate) async fn recognize(
    response: &HttpResponse,
    deadline: Instant,
) -> Result<static_data::Challenge> {
    let (url, status, headers, text) = (
        response.url.clone(),
        response.status,
        response.headers.clone(),
        response.text.clone(),
    );
    crate::cpu::run(deadline, move || {
        static_data::recognize_document(&url, status, &headers, &text)
    })
    .await
}

pub(crate) async fn target(s: &Session, url: &str, deadline: Instant) -> Result<HttpResponse> {
    if let Some(body) = crate::embedded::search_form(url)? {
        s.request(
            "POST",
            url,
            Some(body),
            &[
                ("content-type", "application/x-www-form-urlencoded".into()),
                ("referer", origin(url)? + "/"),
                ("sec-fetch-site", "same-origin".into()),
                ("sec-fetch-mode", "navigate".into()),
                ("sec-fetch-dest", "document".into()),
            ],
            deadline,
        )
        .await
    } else {
        s.request("GET", url, None, &[], deadline).await
    }
}
impl Engine {
    pub(crate) fn capture_generated(
        &self,
        suffix: &str,
        generate: impl FnOnce() -> Result<Vec<u8>>,
    ) -> Result<()> {
        if self.capture_dir.is_none() {
            return Ok(());
        }
        self.capture(&generate()?, suffix)
    }
    pub fn capture(&self, content: &[u8], suffix: &str) -> Result<()> {
        if let Some(dir) = &self.capture_dir {
            std::fs::create_dir_all(dir)?;
            let path = dir.join(format!("{}{}", models::sha(content), suffix));
            if !path.exists() {
                std::fs::write(path, content)?
            }
        }
        Ok(())
    }
    async fn submit(
        &self,
        s: &Session,
        config: &Value,
        p: &Arc<Plan>,
        c: &Cipher,
        payload: &Value,
        endpoint: &str,
        referer: &str,
        deadline: Instant,
        label: &str,
        unframed: bool,
        validation: Option<PendingSealed>,
    ) -> Result<Response> {
        self.capture_generated(&format!(".{label}.payload.json"), || {
            Ok(serde_json::to_vec(payload)?)
        })?;
        let packet = c.encode(payload)?.into_bytes();
        let request = async {
            let response = s
                .request(
                    "POST",
                    endpoint,
                    Some(packet),
                    &[
                        ("cf-chl", string(&config["token"])?.into()),
                        ("cf-chl-ra", "0".into()),
                        ("referer", referer.into()),
                        ("content-type", "text/plain;charset=UTF-8".into()),
                        ("accept", "*/*".into()),
                        ("sec-fetch-site", "same-origin".into()),
                        ("sec-fetch-mode", "cors".into()),
                        ("sec-fetch-dest", "empty".into()),
                    ],
                    deadline,
                )
                .await?;
            self.capture(&response.content, &format!(".{label}.response"))?;
            if response.status != 200 {
                self.capture_generated(&format!(".{label}.rejected-payload.json"), || {
                    Ok(serde_json::to_vec(payload)?)
                })?;
                return Err(protocol_error(
                    "MANAGED_CONTINUATION_REJECTED",
                    "网站拒绝挑战提交",
                    422,
                    json!({"stage":label,"status":response.status}),
                ));
            }
            Ok(response)
        };
        let validation = async move {
            if let Some(validation) = validation {
                validation.check(deadline).await?;
            }
            Ok(())
        };
        // The first request needs only the current wire parameters and the
        // exact-program layout. Never decode its response with an unchecked CV.
        let response = validated_response(request, validation).await?;
        let (p, ray, label, capture) = (
            p.clone(),
            string(&config["ray"])?.to_string(),
            label.to_string(),
            self.clone(),
        );
        crate::cpu::run(deadline, move || {
            let decoded = wire::flow(&response.text, &ray)?;
            capture.capture(&decoded, &format!(".{label}.program"))?;
            Response::read(
                std::str::from_utf8(&decoded)
                    .map_err(|_| Error::new("INVALID_DATA", "Protocol ASCII"))?,
                &p,
                unframed,
            )
        })
        .await
    }
    pub async fn run(
        &self,
        s: Arc<Session>,
        url: &str,
        deadline: Instant,
        analyze: bool,
        response: Option<HttpResponse>,
    ) -> Result<Outcome> {
        let mut current = if let Some(r) = response {
            r
        } else {
            target(&s, url, deadline).await?
        };
        let mut visited = std::collections::HashSet::new();
        let mut details_list: Vec<Value> = Vec::new();
        let mut solved = false;
        for _ in 0..4 {
            let challenge = recognize(&current, deadline).await?;
            let identity = format!(
                "{}|{}",
                origin(&current.url)?,
                challenge.config["cType"]
                    .as_str()
                    .unwrap_or(&challenge.kind)
            );
            require(visited.insert(identity), "Challenge redirect loop")?;
            let mut details = json!({"type":challenge.kind,"ray":challenge.config.get("cRay").or_else(||challenge.config.get("r")),"configFields":challenge.config.as_object().map(|o|o.keys().collect::<Vec<_>>()),"script":challenge.script.as_ref().and_then(|s|url::Url::parse(s).ok()).map(|u|u.path().to_string())});
            if challenge.kind == "blocked" {
                return Err(protocol_error(
                    "UPSTREAM_BLOCKED",
                    "网站返回拦截页，没有可处理的挑战",
                    502,
                    details,
                ));
            }
            if challenge.kind == "embedded-turnstile" {
                if analyze {
                    return Ok(Outcome {
                        response: current,
                        details,
                        solved: false,
                    });
                }
                let (widget_url, widget_text) = (current.url.clone(), current.text.clone());
                let widget = crate::cpu::run(deadline, move || {
                    crate::embedded::recognize_document(&widget_url, &widget_text)
                })
                .await?
                .ok_or_else(|| {
                    Error::new("EMBEDDED_FORMAT_UNSUPPORTED", "嵌入式验证声明发生变化")
                })?;
                let (next, summary) = self
                    .embedded(s.clone(), &current, &widget, deadline)
                    .await?;
                details["embeddedVerification"] = summary;
                let check = recognize(&next, deadline).await?;
                if check.kind == "embedded-turnstile" {
                    return Err(protocol_error(
                        "EMBEDDED_NOT_SOLVED",
                        "验证后仍返回站内验证页",
                        422,
                        json!({"status":next.status,"details":details}),
                    ));
                }
                if check.kind == "none" || check.kind == "jsd-background" {
                    require(
                        next.status == 200,
                        "Embedded verification target HTTP status",
                    )?;
                    details["embeddedVerification"]["strictPureAlgorithmAcceptance"] = json!(true);
                    details["embeddedVerification"]["solved"] = json!(true);
                }
                details_list.push(details);
                current = next;
                solved = true;
                continue;
            }
            if challenge.kind != "managed" {
                if challenge.kind == "jsd" && !analyze {
                    let script_url = challenge
                        .script
                        .as_ref()
                        .ok_or_else(|| Error::new("JSD_FORMAT_UNSUPPORTED", "缺少 JSD 脚本"))?;
                    require(
                        origin(script_url)? == origin(&current.url)?,
                        "Foreign JSD script",
                    )?;
                    let script = s
                        .request(
                            "GET",
                            script_url,
                            None,
                            &[("referer", current.url.clone()), ("accept", "*/*".into())],
                            deadline,
                        )
                        .await?;
                    details["submission"] = crate::jsd::submit(
                        &s,
                        &current.url,
                        &challenge.config,
                        &script.text,
                        deadline,
                    )
                    .await?;
                    current = target(&s, url, deadline).await?;
                    let check = recognize(&current, deadline).await?;
                    if current.status == 403
                        || matches!(check.kind.as_str(), "managed" | "blocked" | "jsd")
                    {
                        return Err(protocol_error(
                            "CHALLENGE_NOT_SOLVED",
                            "JSD 提交后仍未取得页面",
                            422,
                            details,
                        ));
                    }
                    return Ok(Outcome {
                        response: current,
                        details,
                        solved: false,
                    });
                }
                if let Some(first) = details_list.first() {
                    details = first.clone();
                    details["redirectChallenges"] =
                        json!(details_list.iter().skip(1).collect::<Vec<_>>());
                }
                return Ok(Outcome {
                    response: current,
                    details,
                    solved,
                });
            }
            self.capture(&current.content, ".html")?;
            if challenge.config["cType"] != "precursor_interstitial" {
                // The managed flow next loads its component from this fixed
                // origin. Open only that transport while the script is read.
                s.prewarm(FRAME_ORIGIN).await;
            }
            let script_url = challenge
                .script
                .as_ref()
                .ok_or_else(|| Error::new("CHALLENGE_SCRIPT_MISSING", "验证页面缺少脚本地址"))?;
            require(
                origin(script_url)? == origin(&current.url)?,
                "Foreign challenge script",
            )?;
            let script = s
                .request(
                    "GET",
                    script_url,
                    None,
                    &[
                        ("referer", current.url.clone()),
                        ("accept", "*/*".into()),
                        ("sec-fetch-site", "same-origin".into()),
                        ("sec-fetch-mode", "no-cors".into()),
                        ("sec-fetch-dest", "script".into()),
                    ],
                    deadline,
                )
                .await?;
            require(script.status == 200, "Challenge script HTTP status")?;
            self.capture(&script.content, ".js")?;
            let analysis = Instant::now();
            let (p, validation) = plan(script.text.clone(), deadline, !analyze).await?;
            details["managedProtocol"] = json!({"build":p.meta["build"],"nativeRust":true,"analysisMs":analysis.elapsed().as_millis() as u64,"compiledLayoutReused":p.meta["compiledLayoutReused"]==true});
            if analyze {
                return Ok(Outcome {
                    response: current,
                    details,
                    solved: false,
                });
            }
            let (next, flow_details) = self
                .managed(
                    s.clone(),
                    &current,
                    &challenge.config,
                    &script.text,
                    &p,
                    deadline,
                    validation,
                )
                .await?;
            details["firstFlow"] = flow_details;
            details["stage"] = json!("native-parent-submitted");
            details_list.push(details.clone());
            let checked = recognize(&next, deadline).await?;
            let received = (200..300).contains(&next.status)
                || (next.status == 404
                    && matches!(checked.kind.as_str(), "none" | "jsd-background")
                    && next.text.len() >= 256);
            if received
                && !matches!(
                    checked.kind.as_str(),
                    "managed" | "blocked" | "jsd" | "embedded-turnstile"
                )
            {
                details["firstFlow"]["frameInitialization"]["strictPureAlgorithmAcceptance"] =
                    json!(true);
                details["firstFlow"]["frameInitialization"]["solved"] = json!(true);
                details["firstFlow"]["verification"] = json!({"status":next.status,"challengeType":checked.kind,"siteResponseReceived":true,"pageAccessible":(200..300).contains(&next.status)});
                if details_list.len() > 1 {
                    let mut first = details_list[0].clone();
                    first["redirectChallenges"] =
                        json!(details_list.iter().skip(1).collect::<Vec<_>>());
                    details = first;
                }
                return Ok(Outcome {
                    response: next,
                    details,
                    solved: true,
                });
            }
            if checked.kind == "embedded-turnstile"
                || checked.kind == "managed"
                    && (origin(&next.url)? != origin(&current.url)?
                        || (challenge.config["cType"] != "precursor_interstitial"
                            && checked.config["cType"] == "precursor_interstitial"))
            {
                current = next;
                solved = true;
                continue;
            }
            return Err(protocol_error(
                "NATIVE_CHALLENGE_NOT_SOLVED",
                "提交后独立复查仍未取得目标页面",
                422,
                json!({"stage":"final-target","status":next.status,"challenge":checked.kind,"details":details}),
            ));
        }
        Err(Error::new("CHALLENGE_REDIRECT_LIMIT", "网站验证跳转过多"))
    }
    async fn managed(
        &self,
        s: Arc<Session>,
        page: &HttpResponse,
        config: &Value,
        source: &Document,
        p: &Arc<Plan>,
        deadline: Instant,
        validation: Option<PendingSealed>,
    ) -> Result<(HttpResponse, Value)> {
        let mut phase_ms = json!({});
        let first_started = Instant::now();
        for name in ["cType", "cvId", "cITimeS", "md", "cRay", "cH", "cFPWv"] {
            string(&config[name])?;
        }
        require(
            matches!(config["cFPWv"].as_str(), Some("b" | "g"))
                && re(r"^[a-fA-F0-9]{16,32}$").is_match(string(&config["cRay"])?)
                && re(r"^[A-Za-z0-9_.-]{20,512}$").is_match(string(&config["cH"])?),
            "Managed path parameters",
        )?;
        let born = epoch_ms();
        let source = source.clone();
        let new_protocol = p.meta["build"] == "c787ce48cf6b";
        let locale = crate::cpu::run(deadline, move || {
            static_data::assignment_document(
                &source,
                if new_protocol {
                    "_cf_chl_opt.TglCI7"
                } else {
                    "_cf_chl_opt.bwDXz7"
                },
            )
        })
        .await?
        .and_then(|v| {
            v[if new_protocol { "LgdCU7" } else { "wgqFb1" }]
                .as_str()
                .map(String::from)
        })
        .unwrap_or_else(|| "en-us".into());
        let mut payload = initial(config, &locale);
        if p.meta["build"] == "c787ce48cf6b" {
            payload = json!({"gOeQ5":config["cType"],"mTMKo9":config["cvId"],"XNWt6":0,"zJHo8":0,"xAiT8":"Edqb9","LJfS7":0,"XLyWr5":0,"BVMeN7":0,"WqCj2":1,"wIgH5":config["cITimeS"],"RdXW8":config["md"],"xxPdJ4":config["cTplC"]==1,"nYPYj0":{"uXra5":0,"DkMeO5":0,"yVVkz8":0,"Tkjy2":0,"HszO7":0,"vXvkl0":0,"DPFYO4":0,"CjIiz8":0},"nYuU0":false,"BhiO6":false,"tgVOo4":"zZdzq6","kNTL3":"","hbRVt7":"[]","yGdKz7":0,"ZMiod6":"YqOw6","FvhZ6":locale});
            merge(&mut payload, &p.meta["bootstrapFlags"]);
        }
        let cipher = Cipher::new(&p.modulus, &p.alphabet, p.mask()?, None)?;
        if p.meta["build"] != "c787ce48cf6b" {
            payload["mHsm4"] = json!(epoch_ms() - born);
        } else {
            payload["KHnWz9"] = json!(epoch_ms() - born);
        }
        let endpoint = format!(
            "{}/cdn-cgi/challenge-platform/h/{}{}{}/{}",
            origin(&page.url)?,
            string(&config["cFPWv"])?,
            p.flow,
            string(&config["cRay"])?,
            string(&config["cH"])?
        );
        let overlapped = validation.is_some();
        let data = self
            .submit(
                &s,
                &json!({"ray":config["cRay"],"token":config["cH"]}),
                p,
                &cipher,
                &payload,
                &endpoint,
                &page.url,
                deadline,
                "managed-first",
                false,
                validation,
            )
            .await?;
        let entered = epoch_ms();
        phase_ms["firstSubmission"] = json!(first_started.elapsed().as_millis() as u64);
        let nav;
        let summary;
        if config["cType"] == "precursor_interstitial" {
            let o = crate::cpu::run(deadline, move || data.precursor()).await?;
            require(
                p.meta["build"] == "d2d3316b729e"
                    && config
                        .get("pi")
                        .is_some_and(|v| v != false && v != 0 && v != "")
                    && o["script"]
                        == format!("/scripts/precursor/{}/main.js", string(&p.meta["build"])?),
                "Precursor version",
            )?;
            let now = epoch_ms();
            let mut continuation = payload.clone();
            remove(&mut continuation, &["YVDgD5", "bkOl5", "gfOgY8"]);
            copy(&mut continuation, &o, &["vaZBm3", "yVlbA2", "nidg0"]);
            merge(
                &mut continuation,
                &json!({"UORq8":config["cRay"],"UqwKl5":1,"gRYZ0":0,"qrKd7":[],"Hoqd1":0,"CAHKN0":"[]","TYrU7":0,"RxXRQ8":null,"jLEVv0":[],"ejOxj7":"","xWFy3":completion::current_page_token(config)?,"vvfHL3":[],"mHsm4":now-born,"1":{"hmwT6":o["hmwT6"],"nuvy8":o["nuvy8"],"pTaf7":3,"YwQJz9":"","gHYEt0":{"cb":{"cC":0,"xC":0,"pC":0,"sC":0,"cL":[]},"f":{"t":[]},"k":{"t":[]},"n":{"t":[]},"p":{"pt":0,"l":[],"tr":false},"v":{"vH":0,"vV":now-entered,"vC":0,"fO":-1,"fI":0},"_to":born,"_ti":uuid::Uuid::new_v4().to_string(),"_tsrc":"unavailable"},"CyYT8":0,"uGYqH4":entered,"dLrXX1":now,"NEMH8":now-entered}}),
            );
            let target = format!(
                "{}/cdn-cgi/challenge-platform/h/{}{}",
                origin(&page.url)?,
                string(&config["cFPWv"])?,
                string(&o["flow"])?
            );
            let parent_started = Instant::now();
            let complete = self
                .submit(
                    &s,
                    &json!({"ray":config["cRay"],"token":config["cH"]}),
                    p,
                    &cipher,
                    &continuation,
                    &target,
                    &page.url,
                    deadline,
                    "precursor-completion",
                    false,
                    None,
                )
                .await?;
            nav = completion::navigation(&complete, p, config, &page.url)?;
            phase_ms["parentCompletion"] = json!(parent_started.elapsed().as_millis() as u64);
            summary = json!({"taskCount":1,"nativeComputedCount":1,"nativeTaskKinds":["native-precursor-telemetry"]});
        } else {
            let root_plan = p.clone();
            let parameters = crate::cpu::run(deadline, move || data.root(&root_plan)).await?;
            let (completion, frame_summary) = self
                .frame(
                    s.clone(),
                    page,
                    config,
                    &parameters,
                    string(&p.meta["build"])?,
                    deadline,
                    None,
                )
                .await?;
            summary = frame_summary;
            let mut continuation = payload.clone();
            remove(&mut continuation, &["YVDgD5", "bkOl5", "gfOgY8"]);
            copy(
                &mut continuation,
                &parameters,
                &["vaZBm3", "yVlbA2", "nidg0"],
            );
            merge(
                &mut continuation,
                &json!({"UORq8":config["cRay"],"UqwKl5":1,"qrKd7":[],"Hoqd1":0,"CAHKN0":"[]","TYrU7":0,"RxXRQ8":null,"jLEVv0":[],"ejOxj7":completion["parentState"],"xWFy3":completion::current_page_token(config)?,"CtUv5":false,"TKcX8":false,"vvfHL3":[],"1":{"hmwT6":parameters["hmwT6"],"nuvy8":parameters["nuvy8"],"pTaf7":3,"YwQJz9":"","XXao4":completion["widgetToken"],"NAmDV6":"","uGYqH4":entered,"dLrXX1":epoch_ms(),"NEMH8":epoch_ms()-entered},"mHsm4":epoch_ms()-born}),
            );
            if p.meta["build"] == "c787ce48cf6b" {
                continuation = payload.clone();
                remove(&mut continuation, &["xxPdJ4", "yGdKz7", "ZMiod6"]);
                merge(
                    &mut continuation,
                    &json!({"dXXrN6":config["cRay"],"XNWt6":1,"TwoO6":[],"cWAH0":0,"kNTL3":"[]","LoBqb5":0,"FQob1":null,"WdGLU4":{},"NglZ3":[],"sJhNC9":completion["parentState"],"slWOZ8":completion::current_page_token(config)?,"nYuU0":false,"BhiO6":false,"hbRVt7":[],"rXsMy3":parameters["vaZBm3"],"pAFSo0":parameters["yVlbA2"],"pnUb9":parameters["nidg0"],"1":{"tlwi7":parameters["hmwT6"],"rhUgi8":parameters["nuvy8"],"kYpkN9":3,"zzVN3":"","XHpVt4":completion["widgetToken"],"sSxR2":"","roTuJ3":entered,"DJme8":epoch_ms(),"helyQ4":epoch_ms()-entered},"KHnWz9":epoch_ms()-born}),
                );
            }
            let target = format!(
                "{}/cdn-cgi/challenge-platform/h/{}{}",
                origin(&page.url)?,
                string(&config["cFPWv"])?,
                string(&parameters["flow"])?
            );
            let parent_started = Instant::now();
            let complete = self
                .submit(
                    &s,
                    &json!({"ray":config["cRay"],"token":config["cH"]}),
                    p,
                    &cipher,
                    &continuation,
                    &target,
                    &page.url,
                    deadline,
                    "parent-completion",
                    false,
                    None,
                )
                .await?;
            nav = completion::navigation(&complete, p, config, &page.url)?;
            phase_ms["parentCompletion"] = json!(parent_started.elapsed().as_millis() as u64);
        }
        let navigation_started = Instant::now();
        let navigation_method = if nav.is_some() { "POST" } else { "GET" };
        let checked = if let Some(Navigation { url, body }) = nav {
            s.request(
                "POST",
                &url,
                Some(body),
                &[
                    ("content-type", "application/x-www-form-urlencoded".into()),
                    ("referer", page.url.clone()),
                    ("sec-fetch-site", "same-origin".into()),
                    ("sec-fetch-mode", "navigate".into()),
                    ("sec-fetch-dest", "document".into()),
                ],
                deadline,
            )
            .await?
        } else {
            let target = completion::issued_url(&page.url, config)?;
            s.request("GET", &target, None, &[], deadline).await?
        };
        let checked = if crate::embedded::search_form(&page.url)?.is_some() {
            target(&s, &page.url, deadline).await?
        } else {
            checked
        };
        let checked_status = checked.status;
        self.capture(&checked.content, ".verification.html")?;
        phase_ms["navigation"] = json!(navigation_started.elapsed().as_millis() as u64);
        Ok((
            checked,
            json!({"nextStage":"native-parent-submitted","frameInitialization":summary,"phaseMs":phase_ms,"layoutValidationOverlapped":overlapped,"finalNavigation":{"method":navigation_method,"status":checked_status}}),
        ))
    }
    async fn frame(
        &self,
        s: Arc<Session>,
        page: &HttpResponse,
        root: &Value,
        parameters: &Value,
        build: &str,
        deadline: Instant,
        embedded: Option<&crate::embedded::Widget>,
    ) -> Result<(Value, Value)> {
        let mut phase_ms = json!({});
        let frame_started = Instant::now();
        let sitekey = string(&parameters["sitekey"])?;
        require(
            re(r"^[a-f0-9]{12}$").is_match(build)
                && re(r"^0x4[A-Za-z0-9_-]{15,40}$").is_match(sitekey),
            "Widget build/sitekey",
        )?;
        let instance: String = (0..5)
            .map(|_| {
                b"abcdefghijklmnopqrstuvwxyz0123456789"[(rand::random::<u32>() % 36) as usize]
                    as char
            })
            .collect();
        let theme = if embedded.is_some() { "auto" } else { "light" };
        let url=format!("https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b/turnstile/f/av0/rch/{instance}/{sitekey}/{theme}/fbE/new/normal?lang=auto");
        let born = epoch_ms();
        let frame = s
            .request(
                "GET",
                &url,
                None,
                &[
                    ("referer", origin(&page.url)? + "/"),
                    ("sec-fetch-site", "cross-site".into()),
                    ("sec-fetch-mode", "navigate".into()),
                    ("sec-fetch-dest", "iframe".into()),
                ],
                deadline,
            )
            .await?;
        require(frame.status == 200, "Frame page HTTP status")?;
        self.capture(&frame.content, ".frame.html")?;
        let document = frame.text.clone();
        let script_index = crate::cpu::run(deadline, move || {
            document
                .facts()?
                .script_texts
                .iter()
                .enumerate()
                .max_by_key(|(_, s)| s.len())
                .map(|(index, _)| index)
                .ok_or_else(|| Error::new("FRAME_SCRIPT_MISSING", "验证组件缺少脚本"))
        })
        .await?;
        // Configuration and layout read the same current component script
        // independently. Analyse them concurrently; both must succeed.
        let (document, capture) = (frame.text.clone(), self.clone());
        let configuration = crate::cpu::run(deadline, move || {
            let source = &document.facts()?.script_texts[script_index];
            capture.capture(source.as_bytes(), ".frame.js")?;
            static_data::object_data(source, "_cf_chl_opt")?
                .ok_or_else(|| Error::new("FRAME_CONFIG_UNSUPPORTED", "验证组件缺少配置"))
        });
        let document = frame.text.clone();
        let layout = async move {
            let analysis = Instant::now();
            let p = crate::cpu::run(deadline, move || {
                Plan::analyze(&document.facts()?.script_texts[script_index], true).map(Arc::new)
            })
            .await?;
            Ok::<_, Error>((p, analysis.elapsed().as_millis() as u64))
        };
        let (config, (p, analysis_ms)) = tokio::try_join!(configuration, layout)?;
        phase_ms["pageAndConfig"] = json!(frame_started.elapsed().as_millis() as u64);
        for name in [
            "buiX8", "QZAb2", "RUtA1", "MsSIK5", "yoFh0", "DstX3", "cYUbN2", "DNCSh4", "RIQr4",
            "MrbDg2", "UORq8", "ueNyz9",
        ] {
            string(&config[name])?;
        }
        require(
            config.get("TYrU7").is_some()
                && config["uzyK3"] == "b"
                && re(r"^[a-fA-F0-9]{16,32}$").is_match(string(&config["UORq8"])?)
                && re(r"^[A-Za-z0-9_.-]{20,512}$").is_match(string(&config["ueNyz9"])?),
            "Frame request configuration",
        )?;
        let mut payload = initial(
            &json!({"cType":config["buiX8"],"cvId":config["QZAb2"],"cITimeS":config["RUtA1"],"md":config["MsSIK5"],"cTplC":config.get("fpwv7").unwrap_or(&Value::Null)}),
            "en-us",
        );
        remove(&mut payload, &["CtUv5", "TKcX8"]);
        copy(
            &mut payload,
            &config,
            &["yoFh0", "DstX3", "TYrU7", "cYUbN2", "DNCSh4"],
        );
        let elapsed = epoch_ms() - born;
        let mode = if embedded.is_some() {
            string(&config["RIQr4"])?
        } else {
            string(&root["cType"])?
        }
        .to_string();
        require(
            matches!(mode.as_str(), "managed" | "interactive" | "non-interactive"),
            "Frame parent mode",
        )?;
        merge(
            &mut payload,
            &json!({"NOqLU5":root["cType"],"xmKtH5":root["cRay"],"Dtviy4":parameters["pageData"],"PHYjM2":build,"iiWo7":format!("https://challenges.cloudflare.com/turnstile/v0/b/{build}/api.js?onload=DraY9&render=explicit"),"jXAmp8":page.url,"dMNbG1":origin(&page.url)?,"tmhp0":elapsed,"ECvL0":elapsed,"wWyY1":0,"vlScl0":0,"jWijM3":0,"lVgt8":0,"fuhL4":elapsed,"ccKqy6":0,"jrshV1":0,"RxXRQ8":0,"nulf3":0,"rSaL2":config["MrbDg2"],"OtYa2":[],"jLEVv0":[],"mHsm4":elapsed,"URVD0":analysis_ms,"shUar7":analysis_ms,"ghqno0":analysis_ms}),
        );
        if let Some(widget) = embedded {
            // action, cData and chlPageData are absent in this site's render call.
            // JSON.stringify omits undefined members; sending null or a managed
            // page's ray here changes the standalone widget protocol.
            remove(&mut payload, &["NOqLU5", "xmKtH5", "Dtviy4"]);
            payload["iiWo7"] = json!(widget.api_url);
        }
        let cipher = Cipher::new(&p.modulus, &p.alphabet, p.mask()?, None)?;
        let endpoint = format!(
            "https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b{}{}/{}",
            p.flow,
            string(&config["UORq8"])?,
            string(&config["ueNyz9"])?
        );
        let wire_config = json!({"ray":config["UORq8"],"token":config["ueNyz9"]});
        for host in learned_task_hosts() {
            s.prewarm(&format!("https://{host}/")).await;
        }
        let phase = Instant::now();
        let data = self
            .submit(
                &s,
                &wire_config,
                &p,
                &cipher,
                &payload,
                &endpoint,
                &url,
                deadline,
                "frame-first",
                false,
                None,
            )
            .await?;
        phase_ms["firstSubmission"] = json!(phase.elapsed().as_millis() as u64);
        let phase = Instant::now();
        let context_data = (
            data,
            p.clone(),
            frame.text.clone(),
            page.text.clone(),
            url.clone(),
            page.url.clone(),
            mode,
        );
        let (context_data, config, http_tasks) = crate::cpu::run(deadline, move || {
            let (mut data, p, h, ph, fu, pu, mode) = context_data;
            let catalog = Catalog::read(&mut data, &p)?;
            let context = Context {
                response: &data,
                plan: &p,
                catalog: &catalog,
                html: &h,
                parent_html: &ph,
                frame_url: &fu,
                page_url: &pu,
                mode: &mode,
            };
            let http_tasks = crate::http_tasks::tasks(&context, &config)?;
            Ok(((data, p, catalog, h, ph, fu, pu, mode), config, http_tasks))
        })
        .await?;
        phase_ms["catalog"] = json!(phase.elapsed().as_millis() as u64);
        learn_task_hosts(&http_tasks);
        let mut pending = tokio::task::JoinSet::new();
        for task in http_tasks {
            pending.spawn(crate::network::inherit_context(crate::http_tasks::run(
                task,
                s.clone(),
                url.clone(),
                deadline,
            )));
        }
        let phase = Instant::now();
        let (data, catalog, mut computed) = crate::cpu::run(deadline, move || {
            let (d, p, c, h, ph, fu, pu, m) = context_data;
            let computed = calculate_all(&Context {
                response: &d,
                plan: &p,
                catalog: &c,
                html: &h,
                parent_html: &ph,
                frame_url: &fu,
                page_url: &pu,
                mode: &m,
            })?;
            Ok((d, c, computed))
        })
        .await?;
        phase_ms["localTasks"] = json!(phase.elapsed().as_millis() as u64);
        let phase = Instant::now();
        let mut records = Vec::new();
        while let Some(task) = pending.join_next().await {
            let (task, fields, entered, finished, record) =
                task.map_err(|e| Error::new("TASK_ERROR", e.to_string()))??;
            computed.insert(&task.id, &task.kind, fields, entered);
            computed.timings.insert(task.id, (entered, finished));
            records.push(record);
        }
        phase_ms["remainingHttpTasks"] = json!(phase.elapsed().as_millis() as u64);
        let missing: Vec<_> = catalog
            .modules
            .iter()
            .filter(|m| !computed.fields.contains_key(&m.id))
            .map(|m| m.label.clone())
            .collect();
        let task_durations = computed
            .timings
            .values()
            .map(|(start, end)| end.saturating_sub(*start))
            .collect::<Vec<_>>();
        let mut summary = json!({"status":200,"taskCount":catalog.modules.len(),"nativeComputedCount":computed.fields.len(),"nativeTaskKinds":computed.kinds,"nativeHttpTasks":records,"uncomputedTaskModules":missing,"allTaskResultsComputed":missing.is_empty(),"compiledLayoutReused":p.meta["compiledLayoutReused"]==true,"analysisMs":analysis_ms,"strictPureAlgorithmAcceptance":false,"solved":false});
        summary["taskDurationsMs"] = json!(task_durations);
        if !missing.is_empty() {
            return Err(protocol_error(
                "FRAME_DATA_UNSUPPORTED",
                "部分验证任务尚未支持",
                422,
                summary,
            ));
        }
        remove(&mut payload, &["YVDgD5", "bkOl5"]);
        copy(
            &mut payload,
            &catalog.parameters,
            &["vaZBm3", "yVlbA2", "nidg0"],
        );
        merge(&mut payload, &crate::native_input::root());
        merge(
            &mut payload,
            &json!({"UORq8":config["UORq8"],"UqwKl5":catalog.order.len(),"qrKd7":[],"Hoqd1":0,"gfOgY8":0}),
        );
        let mut selectors = Vec::new();
        for (_, v) in &data.packed {
            if v.starts_with('{') && v.contains("\"n\":") {
                selectors.extend(models::ancestry(&serde_json::from_str(v)?)?.1);
            }
        }
        selectors.push("script[nonce]".into());
        payload["vvfHL3"] = json!(selectors);
        let phase = Instant::now();
        let envelopes = array(&catalog.parameters["tasks"])?;
        for (index, id) in catalog.order.iter().enumerate() {
            let mut fields = computed.fields.remove(id).unwrap();
            if fields.get("SgRiW9").is_some() {
                fields["SgRiW9"]["HiXK5"] = json!(frame.content.len());
                fields["SgRiW9"]["bIWk0"] = json!(frame.download_size);
            }
            let entered = computed.timings[id].0;
            let wait = Duration::from_millis((entered + 125).saturating_sub(epoch_ms()));
            if !wait.is_zero() {
                tokio::time::sleep(wait).await
            }
            let finished = epoch_ms();
            let envelope = envelopes.iter().find(|v| v["hmwT6"] == *id).unwrap();
            let mut task = envelope.clone();
            merge(&mut task, &fields);
            merge(
                &mut task,
                &json!({"pTaf7":3,"YwQJz9":"","uGYqH4":entered,"dLrXX1":finished,"NEMH8":finished-entered}),
            );
            payload[(index + 1).to_string()] = task;
        }
        payload["mHsm4"] = json!(epoch_ms() - born);
        phase_ms["taskEnvelopes"] = json!(phase.elapsed().as_millis() as u64);
        let endpoint = format!(
            "https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b{}",
            string(&catalog.parameters["flow"])?
        );
        let phase = Instant::now();
        // Proof facts read only the current component document. Prepare them
        // while the task submission is in flight; they are used only when the
        // response requests work, and a preparation error surfaces only then.
        let document = frame.text.clone();
        let proof = crate::cpu::run(deadline, move || {
            crate::native_input::proof_facts(document.facts()?)
        });
        let (followup, proof) = tokio::join!(
            self.submit(
                &s,
                &wire_config,
                &p,
                &cipher,
                &payload,
                &endpoint,
                &url,
                deadline,
                "frame-tasks",
                true,
                None,
            ),
            proof
        );
        let followup = followup?;
        phase_ms["taskSubmission"] = json!(phase.elapsed().as_millis() as u64);
        let o = match completion::next_for(&followup, embedded.is_some())? {
            completion::Next::Done(done) => {
                summary["workTaskRequested"] = json!(false);
                summary["phaseMs"] = phase_ms;
                return Ok((done, summary));
            }
            completion::Next::Work(operands) => operands,
        };
        require(
            uint(&o["previousCount"])? == catalog.order.len(),
            "Proof task count mismatch",
        )?;
        let entered = epoch_ms();
        tokio::time::sleep(Duration::from_millis(2)).await;
        let started = epoch_ms();
        tokio::time::sleep(Duration::from_millis(6)).await;
        let initialized = epoch_ms();
        require(
            deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                > uint(&o["budgetMs"])? as u128 + 20,
            "Proof deadline insufficient",
        )?;
        let clicked = epoch_ms();
        tokio::time::sleep(Duration::from_millis(2)).await;
        let prefix = format!(
            "{}|{}|{}|",
            string(&config["UORq8"])?,
            epoch_ms(),
            uint(&o["mode"])?
        );
        let work_start = Instant::now();
        let suffix = string(&o["suffix"])?.to_string();
        let budget = uint(&o["budgetMs"])? as u64;
        let work_prefix = prefix.clone();
        let work = crate::cpu::group(deadline, move |i, count| {
            let count = count as u64;
            let first = (rand::random::<u64>() % (10000 / count)) * count + i as u64;
            wire::work(&work_prefix, &suffix, budget, first, count)
        })
        .await?;
        let worker_count = work.len();
        let elapsed = work_start.elapsed().as_secs_f64() * 1000.;
        let finished = epoch_ms();
        let best = work.iter().max_by_key(|r| r[2]).unwrap();
        let attempts: u64 = work.iter().map(|r| r[1]).sum();
        let mut task = proof?;
        copy(&mut task, &o, &["hmwT6", "nuvy8"]);
        merge(
            &mut task,
            &json!({"GEWq5":o["suffix"],"qsgV2":o["budgetMs"],"bsSd2":o["mode"],"pTaf7":3,"YwQJz9":"","fPnBP8":started,"EyyWl0":initialized,"qTNw9":clicked,"evtm8":worker_count,"clAR9":format!("{prefix}{}",best[0]),"QaoMZ6":attempts,"dtQiW5":worker_count,"BvSgL3":elapsed,"swsJd4":finished,"uGYqH4":entered,"dLrXX1":finished,"NEMH8":finished-entered}),
        );
        task["YBguh4"]["HuPfz3"] = json!((clicked - born).to_string());
        let count = catalog.order.len();
        payload[(count + 1).to_string()] = task;
        copy(&mut payload, &o, &["vaZBm3", "yVlbA2", "nidg0"]);
        merge(
            &mut payload,
            &json!({"UqwKl5":count+1,"gRYZ0":count,"ejOxj7":"","mHsm4":finished-born}),
        );
        let endpoint = format!(
            "https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b{}",
            string(&o["flow"])?
        );
        let complete = self
            .submit(
                &s,
                &wire_config,
                &p,
                &cipher,
                &payload,
                &endpoint,
                &url,
                deadline,
                "frame-work",
                true,
                None,
            )
            .await?;
        summary["workTaskRequested"] = json!(true);
        summary["proofAttempts"] = json!(attempts);
        phase_ms["work"] = json!(elapsed);
        summary["phaseMs"] = phase_ms;
        Ok((
            if embedded.is_some() {
                complete.widget_completion()?
            } else {
                complete.completion()?
            },
            summary,
        ))
    }
    async fn embedded(
        &self,
        s: Arc<Session>,
        page: &HttpResponse,
        widget: &crate::embedded::Widget,
        deadline: Instant,
    ) -> Result<(HttpResponse, Value)> {
        self.capture(&page.content, ".embedded.html")?;
        let api = s
            .request(
                "GET",
                &widget.api_url,
                None,
                &[
                    ("referer", page.url.clone()),
                    ("sec-fetch-dest", "script".into()),
                    ("sec-fetch-mode", "no-cors".into()),
                    ("sec-fetch-site", "cross-site".into()),
                ],
                deadline,
            )
            .await?;
        require(api.status == 200, "Embedded API HTTP status")?;
        let build = crate::embedded::api_build(&api.url)?;
        let (done, mut summary) = self
            .frame(
                s.clone(),
                page,
                &json!({}),
                &json!({"sitekey":widget.sitekey}),
                &build,
                deadline,
                Some(widget),
            )
            .await?;
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("verify_token", string(&done["widgetToken"])?)
            .finish()
            .into_bytes();
        let response = s
            .request(
                "POST",
                &widget.verify_url,
                Some(body),
                &[
                    (
                        "content-type",
                        "application/x-www-form-urlencoded; charset=UTF-8".into(),
                    ),
                    ("x-requested-with", "XMLHttpRequest".into()),
                    ("referer", page.url.clone()),
                    (
                        "accept",
                        "application/json, text/javascript, */*; q=0.01".into(),
                    ),
                ],
                deadline,
            )
            .await?;
        require(
            response.status == 200 && origin(&response.url)? == origin(&page.url)?,
            "Site verification HTTP status",
        )?;
        self.capture(&response.content, ".embedded-verification.json")?;
        let cookie = crate::embedded::response_cookie(&serde_json::from_str(&response.text)?)?;
        s.cookies.add(cookie, &page.url);
        let checked = target(&s, &page.url, deadline).await?;
        self.capture(&checked.content, ".embedded-search.html")?;
        summary["siteVerificationStatus"] = json!(response.status);
        summary["verificationStatus"] = json!(checked.status);
        summary["adapter"] = json!("69shuba-search");
        Ok((checked, summary))
    }
}
#[cfg(test)]
mod capture_tests;
#[cfg(test)]
mod validation_tests;
pub fn calculate_all(c: &Context<'_>) -> Result<Results> {
    crate::cpu::check()?;
    let mut result = models::calculate(c)?;
    crate::model_geometry::calculate(c, &mut result)?;
    crate::surface::calculate(c, &mut result)?;
    crate::locale::calculate(c, &mut result)?;
    crate::semantics::calculate(c, &mut result)?;
    crate::benchmark::calculate(c, &mut result)?;
    crate::semantics::callbacks(c, &mut result)?;
    crate::cpu::check()?;
    Ok(result)
}
