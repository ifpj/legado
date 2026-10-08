use crate::{crypto, model};
use anyhow::{Context, Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use rand::RngCore;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use std::{
    io::{Read, Write},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub install_id: String,
    #[serde(default)]
    pub device_type: String,
    #[serde(default)]
    pub device_brand: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub key_timestamp: u64,
    #[serde(default)]
    pub key_version: u64,
    #[serde(default)]
    pub key_user_id: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    #[serde(default)]
    pub session_cookie: String,
    #[serde(default)]
    pub tt_token: String,
    #[serde(default)]
    pub session_sign: String,
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub method: String,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub queue_ms: u64,
    pub batch_wait_ms: u64,
    pub sign_us: u64,
    pub http_ms: u64,
    pub parse_us: u64,
    pub transform_us: u64,
    pub serialize_us: u64,
    pub total_ms: u64,
    pub requests: Vec<RequestMetric>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestMetric {
    pub path: String,
    pub status: u16,
    pub ms: u64,
    pub protocol: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamTrace {
    pub path: String,
    pub status: u16,
    pub body: String,
    pub bytes: usize,
    pub credentials_hidden: bool,
}
pub struct Api {
    pub client: Client,
    pub device: Device,
    pub account: Option<Account>,
    pub os_version: String,
    pub metrics: Metrics,
    pub traces: Vec<UpstreamTrace>,
}
pub fn native_batch(data: &mut Value) {
    if let Some(chapters) = data["chapters"].as_array_mut() {
        for chapter in chapters {
            if let Some(fields) = chapter.as_object_mut() {
                fields.remove("metadata");
            }
        }
    }
}
#[derive(Debug)]
struct EmptyUpstream(String);
impl std::fmt::Display for EmptyUpstream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "番茄接口返回空响应：{}", self.0)
    }
}
impl std::error::Error for EmptyUpstream {}
pub fn encode_query(params: &[(String, String)]) -> String {
    fn encode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
                out.push(b as char)
            } else {
                out.push_str(&format!("%{b:02X}"))
            }
        }
        out
    }
    params
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}
pub fn params(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
pub fn number(v: &Value) -> u64 {
    model::string(v).parse().unwrap_or(0)
}
impl Api {
    pub async fn ensure_device(&mut self) -> Result<()> {
        if self.device.device_id.is_empty() {
            self.register().await?
        }
        Ok(())
    }
    async fn register(&mut self) -> Result<()> {
        // The original source creates 64 hexadecimal characters, i.e. 32 bytes.
        // The TTEncrypt wire prefix therefore contains a 32-byte random seed.
        let mut random = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut random);
        let uuid = || {
            let mut b = [0; 16];
            rand::rngs::OsRng.fill_bytes(&mut b);
            b[6] = (b[6] & 15) | 64;
            b[8] = (b[8] & 63) | 128;
            let h = hex::encode(b);
            format!(
                "{}-{}-{}-{}-{}",
                &h[..8],
                &h[8..12],
                &h[12..16],
                &h[16..20],
                &h[20..]
            )
        };
        let payload = json!({"magic_tag":"ss_app_log","_gen_time":now(),"header":{"display_name":"番茄免费小说","aid":1967,"channel":"43536163a","package":"com.dragon.read","app_version":"7.3.9.32","version_code":73932,"update_version_code":73932,"sdk_version":"3.7.0-rc.25-fanqie-xiaoshuo","sdk_target_version":29,"git_hash":"711d1a7","device_platform":"android","os":"Android","os_version":self.os_version,"os_api":33,"device_model":self.device.device_type,"device_brand":self.device.device_brand,"device_manufacturer":self.device.device_brand,"cpu_abi":"arm64-v8a","density_dpi":240,"display_density":"hdpi","resolution":"720x1280","language":"zh","timezone":8,"access":"wifi","region":"CN","tz_name":"Asia/Shanghai","tz_offset":28800,"sim_serial_number":[],"oaid_may_support":false,"not_request_sender":0,"custom":{"host_bit":32,"dragon_device_type":0},"pre_installed_channel":"","is_system_app":0,"sdk_flavor":"china","guest_mode":0,"cdid":uuid(),"clientudid":uuid(),"req_id":uuid(),"openudid":uuid().replace('-',"")[..20],"sig_hash":"a4a27c2633195374c15651ffc3c4a497"}});
        let fixed=STANDARD.decode("TdTC5rgxYgkOUrPHpnM7pByyRiuCmrWKGWs521cXdST0m69/COjWjSanLjfBqVovHwWlGJKu8pSXMrYqOKrdWA==")?;
        let mut digest_input = Sha512::digest(random).to_vec();
        digest_input.extend(fixed);
        let key = Sha512::digest(digest_input);
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(payload.to_string().as_bytes())?;
        let gzip = gzip.finish()?;
        let mut content = Sha512::digest(&gzip).to_vec();
        content.extend(gzip);
        let mut bytes = hex::decode("746305100000")?;
        bytes.extend(random);
        bytes.extend(crypto::encrypt(&key[..16], &key[16..32], &content)?);
        let start = Instant::now();
        let response = self
            .client
            .post("https://log0-applog-lq.fqnovel.com/service/2/device_register/?tt_data=a")
            .header(header::USER_AGENT, "okhttp/4.10.0")
            .header(header::CONTENT_TYPE, "application/octet-stream; tt-data=a")
            .body(bytes)
            .send()
            .await?;
        let status = response.status().as_u16();
        let protocol = format!("{:?}", response.version());
        let raw = response.text().await?;
        self.record("/service/2/device_register/", status, start, protocol);
        self.capture("/service/2/device_register/", status, &raw);
        let mut v: Value = serde_json::from_str(&raw).context("设备注册未返回 JSON")?;
        model::protect_ids(&mut v);
        let id = model::s(&v, "device_id_str");
        let install = model::s(&v, "install_id_str");
        ensure!(
            !id.is_empty() && id != "0" && !install.is_empty() && install != "0",
            "番茄匿名设备注册失败（HTTP {status}，设备 ID 有效：{}，安装 ID 有效：{}）",
            !id.is_empty() && id != "0",
            !install.is_empty() && install != "0"
        );
        self.device.device_id = id;
        self.device.install_id = install;
        self.device.key = None;
        self.device.key_timestamp = 0;
        self.device.key_version = 0;
        self.device.key_user_id.clear();
        Ok(())
    }
    pub(crate) fn record(&mut self, path: &str, status: u16, start: Instant, protocol: String) {
        let ms = start.elapsed().as_millis() as u64;
        self.metrics.http_ms += ms;
        self.metrics.requests.push(RequestMetric {
            path: path.into(),
            status,
            ms,
            protocol,
        });
    }
    pub(crate) fn capture(&mut self, path: &str, status: u16, raw: &str) {
        let hidden = path.starts_with("/passport/")
            || path.contains("device_register")
            || path.contains("registerkey");
        self.traces.push(UpstreamTrace {
            path: path.into(),
            status,
            bytes: raw.len(),
            body: if path.contains("device_register") {
                let v: Value = serde_json::from_str(raw).unwrap_or_default();
                json!({"note":"身份值不写入诊断记录","deviceIdValid":!matches!(model::s(&v,"device_id_str").as_str(),""|"0"),"installIdValid":!matches!(model::s(&v,"install_id_str").as_str(),""|"0"),"code":v.get("code"),"message":v.get("message")}).to_string()
            } else if hidden {
                json!({"note":"身份、登录凭证与解密密钥不写入诊断记录"}).to_string()
            } else {
                raw.into()
            },
            credentials_hidden: hidden,
        });
    }
    fn url(&self, host: &str, path: &str, mut p: Vec<(String, String)>) -> String {
        p.extend(params(&[
            ("aid", "1967"),
            ("app_name", "novelapp"),
            ("version_code", "73932"),
            ("update_version_code", "73932"),
            ("version_name", "7.3.9.32"),
            ("device_platform", "android"),
            ("os", "android"),
            ("device_type", &self.device.device_type),
            ("device_brand", &self.device.device_brand),
            ("os_version", &self.os_version),
            ("channel", "43536163a"),
            ("device_id", &self.device.device_id),
            ("iid", &self.device.install_id),
            ("_rticket", &now().to_string()),
        ]));
        format!("{host}{path}?{}", encode_query(&p))
    }
    async fn request(
        &mut self,
        host: &str,
        path: &str,
        p: Vec<(String, String)>,
        body: Option<&str>,
        signed: bool,
    ) -> Result<(String, reqwest::header::HeaderMap, u16)> {
        let url = self.url(host, path, p);
        let mut request = if body.is_some() {
            self.client.post(&url)
        } else {
            self.client.get(&url)
        };
        if signed {
            let start = Instant::now();
            let headers = crypto::sign(url.split_once('?').unwrap().1, body, now())?;
            self.metrics.sign_us += start.elapsed().as_micros() as u64;
            for (k, v) in headers {
                request = request.header(
                    &k,
                    if k == "X-SS-STUB" {
                        v.to_uppercase()
                    } else {
                        v
                    },
                )
            }
        }
        request = request
            .header(header::ACCEPT, "application/json")
            .header(header::USER_AGENT, "com.dragon.read");
        if let Some(account) = &self.account {
            if !account.session_cookie.is_empty() {
                request = request.header(header::COOKIE, &account.session_cookie);
                for cookie in account.session_cookie.split(';') {
                    if let Some((name, value)) = cookie.trim().split_once('=') {
                        if name == "passport_csrf_token" || name == "passport_csrf_token_default" {
                            request = request.header("x-tt-passport-csrf-token", value);
                            break;
                        }
                    }
                }
            }
            if !account.tt_token.is_empty() {
                request = request.header("x-tt-token", &account.tt_token)
            }
            if !account.session_sign.is_empty() {
                request = request.header("x-tt-session-sign", &account.session_sign)
            }
        }
        if path.starts_with("/passport/") {
            request = request.header("passport-sdk-version", "5051452")
        }
        if let Some(body) = body {
            request = request
                .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
                .body(body.to_string())
        }
        let start = Instant::now();
        let response = request.send().await?;
        let status = response.status().as_u16();
        let protocol = format!("{:?}", response.version());
        let headers = response.headers().clone();
        let raw = response.text().await?;
        self.record(path, status, start, protocol);
        self.capture(path, status, &raw);
        Ok((raw, headers, status))
    }
    pub async fn get(&mut self, path: &str, p: Vec<(String, String)>) -> Result<Value> {
        self.api(path, p, None).await
    }
    pub async fn api(
        &mut self,
        path: &str,
        p: Vec<(String, String)>,
        body: Option<Value>,
    ) -> Result<Value> {
        let payload = body.map(|v| v.to_string());
        let optional = payload.is_none()
            && matches!(
                path,
                "/reading/bookapi/bookmall/homepage/v"
                    | "/reading/bookapi/bookshelf/list/v"
                    | "/reading/bookapi/read_history/list/v"
            );
        for retry in 0..2 {
            let (mut raw, _, mut status) = self
                .request(
                    "https://reading.snssdk.com",
                    path,
                    p.clone(),
                    payload.as_deref(),
                    !optional,
                )
                .await?;
            if optional {
                let accepted = status == 200
                    && serde_json::from_str::<Value>(&raw).ok().is_some_and(|v| {
                        v.get("code")
                            .is_some_and(|n| model::string(n).parse::<f64>().ok() == Some(0.0))
                    });
                if !accepted {
                    let response = self
                        .request(
                            "https://reading.snssdk.com",
                            path,
                            p.clone(),
                            payload.as_deref(),
                            true,
                        )
                        .await?;
                    raw = response.0;
                    status = response.2
                }
            }
            ensure!(status == 200, "番茄接口 HTTP {status}");
            if raw.is_empty() {
                // A reader response can be empty because its key was replaced,
                // or because the gateway is throttling. It is not proof that
                // the device is invalid. Recover the key in content(), where
                // the request can be rebuilt for the current device.
                if matches!(
                    path,
                    "/reading/reader/full/v"
                        | "/reading/reader/batch_full/v"
                        | "/reading/crypt/registerkey"
                ) || path.contains("/read_progress/")
                    || payload.is_some()
                    || path.contains("/new_category/")
                    || path.contains("/ugc/")
                {
                    return Err(EmptyUpstream(path.into()).into());
                }
                if retry == 0 {
                    self.register().await?;
                    continue;
                }
                return Err(EmptyUpstream(path.into()).into());
            }
            let start = Instant::now();
            let mut response: Value = serde_json::from_str(&raw)
                .with_context(|| format!("番茄接口未返回 JSON：{path}"))?;
            model::protect_ids(&mut response);
            self.metrics.parse_us += start.elapsed().as_micros() as u64;
            ensure!(
                response
                    .get("code")
                    .is_some_and(|n| model::string(n).parse::<f64>().ok() == Some(0.0)),
                "番茄接口 {}：{}",
                model::s(&response, "code"),
                model::s(&response, "message")
            );
            return Ok(response
                .get("data")
                .filter(|v| model::truth(v))
                .cloned()
                .unwrap_or(response));
        }
        Err(anyhow!("设备注册重试失败"))
    }
    pub async fn register_key(&mut self, force: bool) -> Result<()> {
        let uid = self
            .account
            .as_ref()
            .map(|a| a.uid.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "0".into());
        if !force
            && self.device.key.is_some()
            && self.device.key_version > 0
            && self.device.key_user_id == uid
        {
            return Ok(());
        }
        for retry in 0..2 {
            let id = self.device.device_id.clone();
            let mut data = id.parse::<u64>()?.to_le_bytes().to_vec();
            data.extend(uid.parse::<u64>()?.to_le_bytes());
            let mut iv = [0; 16];
            rand::rngs::OsRng.fill_bytes(&mut iv);
            let mut envelope = iv.to_vec();
            envelope.extend(crypto::encrypt(&crypto::MASTER_KEY, &iv, &data)?);
            let result = self
                .api(
                    "/reading/crypt/registerkey",
                    vec![],
                    Some(json!({"content":STANDARD.encode(envelope),"keyver":1})),
                )
                .await;
            if self.device.device_id != id && retry == 0 {
                continue;
            }
            let result = result?;
            let key = crypto::decrypt(&crypto::MASTER_KEY, &model::s(&result, "key"))?;
            ensure!(
                key.len() == 16 && number(&result["keyver"]) > 0,
                "番茄密钥注册失败"
            );
            self.device.key = Some(hex::encode(key));
            self.device.key_timestamp = number(&result["key_register_ts"]);
            self.device.key_version = number(&result["keyver"]);
            self.device.key_user_id = uid;
            return Ok(());
        }
        Err(anyhow!("密钥注册时设备重复失效"))
    }
    pub async fn content(&mut self, book: &Value, chapter: &Value) -> Result<String> {
        let id = chapter_id(chapter)?;
        let book_id = model::book_id(book)?;
        let mut renew_key = false;
        for attempt in 0..3 {
            if let Err(error) = self.register_key(renew_key).await {
                if error.is::<EmptyUpstream>() && attempt < 2 {
                    self.register().await?;
                    renew_key = true;
                    continue;
                }
                return Err(error);
            }
            let device_id = self.device.device_id.clone();
            let response = self
                .get(
                    "/reading/reader/full/v",
                    params(&[
                        ("book_id", &book_id),
                        ("item_id", &id),
                        ("key_register_ts", &self.device.key_timestamp.to_string()),
                        ("novel_text_type", "1"),
                    ]),
                )
                .await;
            let result = match response {
                Ok(v) => v,
                Err(error) if error.is::<EmptyUpstream>() && attempt < 2 => {
                    // Re-register a key first; replacing the device for every
                    // downloaded chapter creates an upstream registration storm.
                    if attempt == 1 {
                        self.register().await?;
                    }
                    renew_key = true;
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if self.device.device_id != device_id {
                renew_key = true;
                continue;
            }
            ensure!(
                model::first(&result, &["code_i32", "code"])
                    .is_none_or(|v| model::string(v).parse::<f64>().ok() == Some(0.0)),
                "番茄章节不可读"
            );
            let html = if number(&result["crypt_status"]) == 1 {
                model::s(&result, "content")
            } else {
                if !matching_key(&self.device, &result) && attempt < 2 {
                    renew_key = true;
                    continue;
                }
                ensure!(
                    matching_key(&self.device, &result),
                    "番茄章节密钥版本不匹配"
                );
                let bytes = crypto::decrypt(
                    &hex::decode(self.device.key.as_ref().context("番茄章节解密密钥缺失")?)?,
                    &model::s(&result, "content"),
                )?;
                if number(&result["compress_status"]) == 1 {
                    let mut text = String::new();
                    GzDecoder::new(&bytes[..]).read_to_string(&mut text)?;
                    text
                } else {
                    String::from_utf8(bytes)?
                }
            };
            let start = Instant::now();
            let output = model::content(&html, number(&result["paragraphs_num"]) as usize)?;
            self.metrics.transform_us += start.elapsed().as_micros() as u64;
            return Ok(output);
        }
        Err(anyhow!("番茄章节恢复失败，请稍后重试"))
    }
    pub async fn content_batch(&mut self, book: &Value, chapters: &[Value]) -> Result<Value> {
        ensure!(
            !chapters.is_empty() && chapters.len() <= 30,
            "章节批量数量应在 1 到 30 之间"
        );
        let ids = chapters
            .iter()
            .map(chapter_id)
            .collect::<Result<Vec<_>>>()?;
        let book_id = model::book_id(book)?;
        self.register_key(false).await?;
        let mut completed = std::collections::HashMap::new();
        let mut missing = Vec::new();
        let mut pending = ids.clone();
        for attempt in 0..2 {
            let data = self
                .get(
                    "/reading/reader/batch_full/v",
                    params(&[
                        ("book_id", &book_id),
                        ("item_ids", &pending.join(",")),
                        // APK ml6.v1 uses Download (0), in sequential batches of 30.
                        // This mode requires the common update_version_code.
                        ("req_type", "0"),
                        ("key_register_ts", &self.device.key_timestamp.to_string()),
                    ]),
                )
                .await?;
            if attempt == 0
                && ids.iter().any(|id| {
                    let item = &data[id];
                    item.is_object()
                        && readable(item)
                        && number(&item["crypt_status"]) != 1
                        && (number(&item["crypt_status"]) == 2 || !matching_key(&self.device, item))
                })
            {
                self.register_key(true).await?;
                continue;
            }
            let start = Instant::now();
            missing.clear();
            for (id, chapter) in ids.iter().zip(chapters) {
                if completed.contains_key(id) {
                    continue;
                }
                let item = &data[id];
                match self.decode_content(item) {
                    Ok(content) => { completed.insert(id.clone(),json!({"itemId":id,"url":model::s(chapter,"url"),"content":content,"metadata":item})); },
                    Err(error) => missing.push(json!({"itemId":id,"url":model::s(chapter,"url"),"error":format!("番茄批量正文未取得：{error}")})),
                }
            }
            self.metrics.transform_us += start.elapsed().as_micros() as u64;
            if missing.is_empty() {
                break;
            }
            pending = missing.iter().map(|v| model::s(v, "itemId")).collect();
        }
        let output: Vec<_> = ids
            .iter()
            .filter_map(|id| completed.get(id).cloned())
            .collect();
        Ok(
            json!({"requested":chapters.len(),"returned":output.len(),"chapters":output,"missing":missing,"fallback":"Legado retries missing chapters individually"}),
        )
    }
    fn decode_content(&self, item: &Value) -> Result<String> {
        ensure!(item.is_object(), "番茄批量响应缺少章节，将单章补取");
        ensure!(readable(item), "番茄章节不可读");
        let html = if number(&item["crypt_status"]) == 1 {
            model::s(item, "content")
        } else {
            ensure!(matching_key(&self.device, item), "番茄章节密钥版本不匹配");
            let bytes = crypto::decrypt(
                &hex::decode(self.device.key.as_ref().context("番茄章节解密密钥缺失")?)?,
                &model::s(item, "content"),
            )?;
            if number(&item["compress_status"]) == 1 {
                let mut text = String::new();
                GzDecoder::new(&bytes[..]).read_to_string(&mut text)?;
                text
            } else {
                String::from_utf8(bytes)?
            }
        };
        model::content(&html, number(&item["paragraphs_num"]) as usize)
    }
    pub async fn web_login(&mut self, cookie: &str) -> Result<Account> {
        self.account = Some(Account {
            session_cookie: cookie.into(),
            ..Default::default()
        });
        let (raw, headers, status) = self
            .request(
                "https://security.snssdk.com",
                "/passport/account/info/v2/",
                params(&[("passport-sdk-version", "5051452")]),
                None,
                true,
            )
            .await?;
        let mut response: Value =
            serde_json::from_str(&raw).context("官方登录接口未返回有效响应")?;
        model::protect_ids(&mut response);
        let data = &response["data"];
        ensure!(
            response["message"] == "success"
                && number(
                    model::first(data, &["error_code"])
                        .or_else(|| model::first(&response, &["error_code"]))
                        .unwrap_or(&Value::Null)
                ) == 0,
            "官方登录接口未接受会话（HTTP {status}）"
        );
        let mut cookies: Vec<(String, String)> = cookie
            .split(';')
            .filter_map(|v| v.trim().split_once('=').map(|(k, v)| (k.into(), v.into())))
            .collect();
        for c in headers.get_all(header::SET_COOKIE) {
            if let Ok(c) = c.to_str() {
                if let Some((key, value)) = c.split(';').next().unwrap_or("").split_once('=') {
                    if let Some(old) = cookies.iter_mut().find(|(k, _)| k == key) {
                        old.1 = value.into()
                    } else {
                        cookies.push((key.into(), value.into()))
                    }
                }
            }
        }
        if let Some(v) = model::first(data, &["session_key"]) {
            if !cookies.iter().any(|(k, _)| k == "sessionid") {
                cookies.push(("sessionid".into(), model::string(v)))
            }
        }
        let account = Account {
            session_cookie: cookies
                .into_iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; "),
            tt_token: headers
                .get("x-tt-token")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .into(),
            session_sign: headers
                .get("x-tt-session-sign")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .into(),
            uid: model::first(data, &["user_id_str", "user_id", "uid"])
                .map(model::string)
                .unwrap_or_default(),
            nickname: model::first(data, &["screen_name", "name", "nickname"])
                .map(model::string)
                .unwrap_or_default(),
            method: "web".into(),
        };
        ensure!(
            !account.uid.is_empty() && account.uid != "0",
            "网页登录没有返回客户端账号 ID"
        );
        self.account = Some(account.clone());
        self.get(
            "/reading/bookapi/bookshelf/list/v",
            params(&[("server_time", "0")]),
        )
        .await?;
        self.get(
            "/reading/bookapi/read_history/list/v",
            params(&[
                ("book_type", "0"),
                ("offset", "0"),
                ("limit", "1"),
                ("full_field", "true"),
                ("is_first_load", "true"),
            ]),
        )
        .await?;
        Ok(account)
    }
}
pub fn chapter_id(chapter: &Value) -> Result<String> {
    let url = model::s(chapter, "url");
    let id = if url.bytes().all(|c| c.is_ascii_digit()) {
        url.as_str()
    } else {
        url.split("/reader/")
            .nth(1)
            .unwrap_or("")
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap_or("")
    };
    ensure!(
        !id.is_empty() && id.len() <= 20 && id.parse::<u64>().ok().is_some_and(|n| n > 0),
        "章节地址无效"
    );
    Ok(id.into())
}
fn readable(item: &Value) -> bool {
    model::first(item, &["code_i32", "code"])
        .is_none_or(|v| model::string(v).parse::<f64>().ok() == Some(0.0))
}
fn matching_key(device: &Device, response: &Value) -> bool {
    device.key.as_ref().is_some_and(|k| k.len() == 32)
        && device.key_version > 0
        && number(&response["key_version"]) == device.key_version
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_batch_keeps_plaintext_order_and_missing_for_fallback() {
        let mut data = json!({"requested":3,"returned":2,"chapters":[
            {"itemId":"9000000000000000001","url":"one","content":"第一章正文","metadata":{"content":"encrypted"}},
            {"itemId":"9000000000000000003","url":"three","content":"第三章正文","metadata":{"title":"第三章"}}
        ],"missing":[{"url":"two","error":"not returned"}],"fallback":"single"});
        native_batch(&mut data);
        assert_eq!(
            data["chapters"],
            json!([
                {"itemId":"9000000000000000001","url":"one","content":"第一章正文"},
                {"itemId":"9000000000000000003","url":"three","content":"第三章正文"}
            ])
        );
        assert_eq!(
            data["missing"],
            json!([{"url":"two","error":"not returned"}])
        );
        assert_eq!(data["requested"], 3);
    }
    #[test]
    fn batch_decoder_rejects_missing_and_unreadable_and_decodes_gzip() {
        let api = Api {
            client: Client::new(),
            device: Device {
                key: Some("00".repeat(16)),
                key_version: 12,
                ..Default::default()
            },
            account: None,
            os_version: "13".into(),
            metrics: Default::default(),
            traces: vec![],
        };
        assert!(api.decode_content(&Value::Null).is_err());
        assert!(
            api.decode_content(&json!({"code":101009,"crypt_status":1,"content":"denied"}))
                .is_err()
        );
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all("<p>新鲜正文</p>".as_bytes()).unwrap();
        let mut bytes = vec![0u8; 16];
        bytes.extend(crypto::encrypt(&[0; 16], &[0; 16], &gzip.finish().unwrap()).unwrap());
        let mut item = json!({"code":0,"crypt_status":0,"compress_status":1,"key_version":12,"paragraphs_num":1,"content":STANDARD.encode(bytes)});
        assert_eq!(api.decode_content(&item).unwrap(), "新鲜正文");
        item["key_version"] = json!(13);
        assert!(api.decode_content(&item).is_err());
    }
    #[test]
    fn missing_keys_and_zero_versions_never_match() {
        assert!(!matching_key(&Device::default(), &json!({"key_version":0})));
        let device = Device {
            key: Some("00".repeat(16)),
            key_version: 12,
            ..Default::default()
        };
        assert!(matching_key(&device, &json!({"key_version":12})));
        assert!(!matching_key(&device, &json!({"key_version":13})));
    }
}
