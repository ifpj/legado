//! End-to-end native HTTP protocol, with independent target-page verification.
use crate::{
    array,
    catalog::Catalog,
    completion::{self, Navigation},
    data::{Plan, Response},
    epoch_ms,
    html::Facts,
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
async fn plan(source: String, frame: bool) -> Result<Plan> {
    tokio::task::spawn_blocking(move || Plan::analyze(&source, frame))
        .await
        .map_err(|e| Error::new("COMPILER_ERROR", e.to_string()))?
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
        p: &Plan,
        c: &Cipher,
        payload: &Value,
        endpoint: &str,
        referer: &str,
        deadline: Instant,
        label: &str,
        unframed: bool,
    ) -> Result<Response> {
        self.capture(
            &serde_json::to_vec(payload)?,
            &format!(".{label}.payload.json"),
        )?;
        let packet = c.encode(payload)?.into_bytes();
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
            self.capture(
                &serde_json::to_vec(payload)?,
                &format!(".{label}.rejected-payload.json"),
            )?;
            return Err(protocol_error(
                "MANAGED_CONTINUATION_REJECTED",
                "网站拒绝挑战提交",
                422,
                json!({"stage":label,"status":response.status}),
            ));
        }
        let decoded = wire::flow(&response.text, string(&config["ray"])?)?;
        self.capture(&decoded, &format!(".{label}.program"))?;
        Response::read(
            std::str::from_utf8(&decoded)
                .map_err(|_| Error::new("INVALID_DATA", "Protocol ASCII"))?,
            p,
            unframed,
        )
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
            let challenge = static_data::recognize(
                &current.url,
                current.status,
                &current.headers,
                &current.text,
            )?;
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
                let widget =
                    crate::embedded::recognize(&current.url, &current.text)?.ok_or_else(|| {
                        Error::new("EMBEDDED_FORMAT_UNSUPPORTED", "嵌入式验证声明发生变化")
                    })?;
                let (next, summary) = self
                    .embedded(s.clone(), &current, &widget, deadline)
                    .await?;
                details["embeddedVerification"] = summary;
                let check =
                    static_data::recognize(&next.url, next.status, &next.headers, &next.text)?;
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
                    let check = static_data::recognize(
                        &current.url,
                        current.status,
                        &current.headers,
                        &current.text,
                    )?;
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
            let p = plan(script.text.clone(), false).await?;
            details["managedProtocol"] = json!({"build":p.meta["build"],"nativeRust":true});
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
                )
                .await?;
            details["firstFlow"] = flow_details;
            details["stage"] = json!("native-parent-submitted");
            details_list.push(details.clone());
            let checked =
                static_data::recognize(&next.url, next.status, &next.headers, &next.text)?;
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
                json!({"status":next.status,"challenge":checked.kind,"details":details}),
            ));
        }
        Err(Error::new("CHALLENGE_REDIRECT_LIMIT", "网站验证跳转过多"))
    }
    async fn managed(
        &self,
        s: Arc<Session>,
        page: &HttpResponse,
        config: &Value,
        source: &str,
        p: &Plan,
        deadline: Instant,
    ) -> Result<(HttpResponse, Value)> {
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
        let locale = static_data::assignment(source, "_cf_chl_opt.bwDXz7")?
            .and_then(|v| v["wgqFb1"].as_str().map(String::from))
            .unwrap_or_else(|| "en-us".into());
        let mut payload = initial(config, &locale);
        let cipher = Cipher::new(&p.modulus, &p.alphabet, p.mask()?, None)?;
        payload["mHsm4"] = json!(epoch_ms() - born);
        let endpoint = format!(
            "{}/cdn-cgi/challenge-platform/h/{}{}{}/{}",
            origin(&page.url)?,
            string(&config["cFPWv"])?,
            p.flow,
            string(&config["cRay"])?,
            string(&config["cH"])?
        );
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
            )
            .await?;
        let entered = epoch_ms();
        let nav;
        let summary;
        if config["cType"] == "precursor_interstitial" {
            let o = data.precursor()?;
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
                )
                .await?;
            nav = completion::navigation(&complete, p, config, &page.url)?;
            summary = json!({"taskCount":1,"nativeComputedCount":1,"nativeTaskKinds":["native-precursor-telemetry"]});
        } else {
            let parameters = data.root()?;
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
            let target = format!(
                "{}/cdn-cgi/challenge-platform/h/{}{}",
                origin(&page.url)?,
                string(&config["cFPWv"])?,
                string(&parameters["flow"])?
            );
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
                )
                .await?;
            nav = completion::navigation(&complete, p, config, &page.url)?;
        }
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
        self.capture(&checked.content, ".verification.html")?;
        Ok((
            checked,
            json!({"nextStage":"native-parent-submitted","frameInitialization":summary}),
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
        let facts = Facts::read(&frame.text)?;
        let source = facts
            .script_texts
            .into_iter()
            .max_by_key(String::len)
            .ok_or_else(|| Error::new("FRAME_SCRIPT_MISSING", "验证组件缺少脚本"))?;
        self.capture(source.as_bytes(), ".frame.js")?;
        let config = static_data::object_data(&source, "_cf_chl_opt")?
            .ok_or_else(|| Error::new("FRAME_CONFIG_UNSUPPORTED", "验证组件缺少配置"))?;
        let analysis = Instant::now();
        let p = plan(source, true).await?;
        let analysis_ms = analysis.elapsed().as_millis() as u64;
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
        };
        require(
            matches!(mode, "managed" | "interactive" | "non-interactive"),
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
        let mut data = self
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
            )
            .await?;
        let catalog = Catalog::read(&mut data, &p)?;
        let context = Context {
            response: &data,
            plan: &p,
            catalog: &catalog,
            html: &frame.text,
            parent_html: &page.text,
            frame_url: &url,
            page_url: &page.url,
            mode,
        };
        let http_tasks = crate::http_tasks::tasks(&context, &config)?;
        let mut pending = Vec::new();
        for task in http_tasks {
            pending.push(tokio::spawn(crate::network::inherit_context(
                crate::http_tasks::run(task, s.clone(), url.clone(), deadline),
            )));
        }
        let context_data = (
            data.clone(),
            p.clone(),
            catalog.clone(),
            frame.text.clone(),
            page.text.clone(),
            url.clone(),
            page.url.clone(),
            mode.to_string(),
        );
        let mut computed = tokio::task::spawn_blocking(move || {
            let (d, p, c, h, ph, fu, pu, m) = context_data;
            calculate_all(&Context {
                response: &d,
                plan: &p,
                catalog: &c,
                html: &h,
                parent_html: &ph,
                frame_url: &fu,
                page_url: &pu,
                mode: &m,
            })
        })
        .await
        .map_err(|e| Error::new("TASK_ERROR", e.to_string()))??;
        let mut records = Vec::new();
        for task in pending {
            let (task, fields, entered, finished, record) = task
                .await
                .map_err(|e| Error::new("TASK_ERROR", e.to_string()))??;
            computed.insert(&task.id, &task.kind, fields, entered);
            computed.timings.insert(task.id, (entered, finished));
            records.push(record);
        }
        let missing: Vec<_> = catalog
            .modules
            .iter()
            .filter(|m| !computed.fields.contains_key(&m.id))
            .map(|m| m.label.clone())
            .collect();
        let mut summary = json!({"status":200,"taskCount":catalog.modules.len(),"nativeComputedCount":computed.fields.len(),"nativeTaskKinds":computed.kinds,"nativeHttpTasks":records,"uncomputedTaskModules":missing,"allTaskResultsComputed":missing.is_empty(),"compiledLayoutReused":p.meta["compiledLayoutReused"]==true,"analysisMs":analysis_ms,"strictPureAlgorithmAcceptance":false,"solved":false});
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
        let endpoint = format!(
            "https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b{}",
            string(&catalog.parameters["flow"])?
        );
        let followup = self
            .submit(
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
            )
            .await?;
        let o = match completion::next_for(&followup, embedded.is_some())? {
            completion::Next::Done(done) => {
                summary["workTaskRequested"] = json!(false);
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
        let mut workers = Vec::new();
        for i in 0..6 {
            let prefix = prefix.clone();
            let suffix = suffix.clone();
            let first = (rand::random::<u64>() % (10000 / 6)) * 6 + i;
            workers.push(tokio::task::spawn_blocking(move || {
                wire::work(&prefix, &suffix, budget, first, 6)
            }));
        }
        let mut work = Vec::new();
        for worker in workers {
            work.push(
                worker
                    .await
                    .map_err(|e| Error::new("WORK_ERROR", e.to_string()))??,
            )
        }
        let elapsed = work_start.elapsed().as_secs_f64() * 1000.;
        let finished = epoch_ms();
        let best = work.iter().max_by_key(|r| r[2]).unwrap();
        let attempts: u64 = work.iter().map(|r| r[1]).sum();
        let mut task = crate::native_input::proof(&frame.text)?;
        copy(&mut task, &o, &["hmwT6", "nuvy8"]);
        merge(
            &mut task,
            &json!({"GEWq5":o["suffix"],"qsgV2":o["budgetMs"],"bsSd2":o["mode"],"pTaf7":3,"YwQJz9":"","fPnBP8":started,"EyyWl0":initialized,"qTNw9":clicked,"evtm8":6,"clAR9":format!("{prefix}{}",best[0]),"QaoMZ6":attempts,"dtQiW5":6,"BvSgL3":elapsed,"swsJd4":finished,"uGYqH4":entered,"dLrXX1":finished,"NEMH8":finished-entered}),
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
            )
            .await?;
        summary["workTaskRequested"] = json!(true);
        summary["proofAttempts"] = json!(attempts);
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
pub fn calculate_all(c: &Context<'_>) -> Result<Results> {
    let mut result = models::calculate(c)?;
    crate::model_geometry::calculate(c, &mut result)?;
    crate::surface::calculate(c, &mut result)?;
    crate::locale::calculate(c, &mut result)?;
    crate::semantics::calculate(c, &mut result)?;
    crate::benchmark::calculate(c, &mut result)?;
    crate::semantics::callbacks(c, &mut result)?;
    Ok(result)
}
