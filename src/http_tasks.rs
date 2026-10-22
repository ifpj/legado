//! Finite network tasks with authenticated ownership and measured timing.
use crate::{
    epoch_ms,
    models::{has, Context},
    network::Session,
    re, require, string, Error, Result,
};
use serde_json::{json, Map, Value};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};
#[derive(Clone)]
pub struct Task {
    pub id: String,
    pub kind: String,
    pub url: String,
    pub timestamp: String,
    pub digest: String,
}
pub fn tasks(c: &Context<'_>, config: &Value) -> Result<Vec<Task>> {
    let ray = string(&config["UORq8"])?;
    require(re(r"^[a-fA-F0-9]{16,32}$").is_match(ray), "Frame ray")?;
    let mut result = Vec::new();
    let pattern = re(&format!(
        r"^/(ci|pat)/{}/([0-9]{{13}})/(?:([a-f0-9]{{64}})/)?$",
        regex::escape(ray)
    ));
    for (p, value) in &c.response.packed {
        if !value.starts_with("/ci/") && !value.starts_with("/pat/") {
            continue;
        }
        let cap = pattern
            .captures(value)
            .ok_or_else(|| Error::new("INVALID_DATA", "Foreign HTTP task path"))?;
        let kind = &cap[1];
        require(
            (kind == "pat") == cap.get(3).is_some(),
            "HTTP task kind mismatch",
        )?;
        let owners: Vec<_> = c
            .catalog
            .modules
            .iter()
            .filter(|m| m.start <= *p && *p < m.end)
            .collect();
        require(
            owners.len() == 1
                && owners[0].data.len() <= 16384
                && crate::data::token(&owners[0].data),
            "HTTP task ownership",
        )?;
        let owner = owners[0];
        result.push(Task {
            id: owner.id.clone(),
            kind: kind.into(),
            url: format!(
                "https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b{value}{}",
                owner.data
            ),
            timestamp: cap[2].into(),
            digest: cap.get(3).map_or("", |m| m.as_str()).into(),
        });
    }
    for m in &c.catalog.modules {
        let values: HashSet<_> = c
            .response
            .between(m.start, m.end)
            .iter()
            .map(|l| l.value.as_str())
            .collect();
        if !has(
            &values,
            &["hkqe2", "fetch_error", "no-store", "omit", "/i/"],
        ) {
            continue;
        }
        let prefixes:Vec<_>=values.into_iter().filter(|s|re(r"^https://[a-z0-9-]{1,63}\.challenges\.cloudflare\.com/cdn-cgi/challenge-platform/h/$").is_match(s)).collect();
        require(
            prefixes.len() == 1 && matches!(config["uzyK3"].as_str(), Some("b" | "g")),
            "Integrity path",
        )?;
        let token = string(&config["ueNyz9"])?;
        require(
            re(r"^[A-Za-z0-9_.-]{20,512}$").is_match(token),
            "Integrity token",
        )?;
        result.push(Task {
            id: m.id.clone(),
            kind: "integrity".into(),
            url: format!(
                "{}{}/i/{ray}/{token}",
                prefixes[0],
                string(&config["uzyK3"])?
            ),
            timestamp: String::new(),
            digest: String::new(),
        });
    }
    require(
        result
            .iter()
            .map(|t| t.kind.as_str())
            .collect::<HashSet<_>>()
            .len()
            == result.len(),
        "Duplicate HTTP task",
    )?;
    Ok(result)
}
pub fn integrity(status: u16, headers: &Map<String, Value>) -> Value {
    let h = |n: &str| headers.get(n).and_then(Value::as_str).unwrap_or("");
    let value = if !matches!(
        h("access-control-allow-origin"),
        "*" | "https://challenges.cloudflare.com"
    ) {
        "fetch_error".into()
    } else if !(200..300).contains(&status) {
        format!("http_error:{status}")
    } else {
        let visible = h("access-control-expose-headers")
            .split(',')
            .any(|s| matches!(s.trim().to_lowercase().as_str(), "*" | "cf-chl-peek"));
        if visible && !h("cf-chl-peek").trim().is_empty() {
            "ok".into()
        } else {
            "no_token".into()
        }
    };
    json!({"hkqe2":value})
}
pub fn png(content: &[u8], allow_raw: bool) -> Result<(u32, u32)> {
    require(
        (33..=4 * 1024 * 1024).contains(&content.len())
            && content.starts_with(b"\x89PNG\r\n\x1a\n"),
        "PNG size/signature",
    )?;
    let mut position = 8;
    let (mut width, mut height, mut row, mut expected) = (0u32, 0u32, 0usize, 0usize);
    let (mut ended, mut idat_ended, mut marker) = (false, false, false);
    let mut compressed = Vec::new();
    let u32be = |b: &[u8]| u32::from_be_bytes(b.try_into().unwrap());
    while position < content.len() {
        require(position + 12 <= content.len(), "PNG chunk header")?;
        let size = u32be(&content[position..position + 4]) as usize;
        let end = position
            .checked_add(size + 12)
            .ok_or_else(|| Error::new("INVALID_DATA", "PNG length"))?;
        require(end <= content.len(), "PNG chunk length")?;
        let kind = &content[position + 4..position + 8];
        let data = &content[position + 8..end - 4];
        require(
            crc32fast::hash(&content[position + 4..end - 4]) == u32be(&content[end - 4..end]),
            "PNG checksum",
        )?;
        require(position != 8 || kind == b"IHDR", "PNG first chunk")?;
        match kind {
            b"IHDR" => {
                require(width == 0 && size == 13, "PNG header")?;
                width = u32be(&data[..4]);
                height = u32be(&data[4..8]);
                require(
                    width > 0
                        && width <= 16384
                        && height > 0
                        && height <= 16384
                        && u64::from(width) * u64::from(height) <= 16 * 1024 * 1024
                        && data[8] == 8
                        && matches!(data[9], 0 | 2 | 4 | 6)
                        && data[10..] == [0, 0, 0],
                    "PNG format",
                )?;
                let channels = match data[9] {
                    0 => 1,
                    2 => 3,
                    4 => 2,
                    _ => 4,
                };
                row = width as usize * channels + 1;
                expected = row * height as usize;
            }
            b"IDAT" => {
                require(width > 0 && !idat_ended, "PNG data order")?;
                compressed.extend(data);
            }
            b"IEND" => {
                require(
                    size == 0 && !compressed.is_empty() && end == content.len(),
                    "PNG end",
                )?;
                ended = true;
            }
            b"tEXt" => {
                if size == 6 && data.starts_with(b"r\0") {
                    marker = true
                }
            }
            b"PLTE" => {}
            _ => require(!kind[0].is_ascii_uppercase(), "PNG critical chunk")?,
        }
        if !compressed.is_empty() && kind != b"IDAT" {
            idat_ended = true
        }
        position = end;
    }
    require(ended && width > 0, "Incomplete PNG")?;
    let inflate = |zlib: bool| -> Result<Vec<u8>> {
        let mut decoder = flate2::Decompress::new(zlib);
        let mut output = vec![0u8; expected + 1];
        let status = decoder
            .decompress(&compressed, &mut output, flate2::FlushDecompress::Finish)
            .map_err(|e| Error::new("INVALID_DATA", e.to_string()))?;
        require(
            status == flate2::Status::StreamEnd
                && decoder.total_in() == compressed.len() as u64
                && decoder.total_out() == expected as u64,
            "PNG compressed size",
        )?;
        output.truncate(expected);
        Ok(output)
    };
    let data = match inflate(true) {
        Ok(data) => data,
        Err(e) => {
            if allow_raw && marker {
                inflate(false)?
            } else {
                return Err(e);
            }
        }
    };
    require(
        data.iter().step_by(row).all(|b| *b <= 4),
        "PNG scanline filter",
    )?;
    Ok((width, height))
}
pub async fn run(
    task: Task,
    session: Arc<Session>,
    frame_url: String,
    deadline: Instant,
) -> Result<(Task, Value, u64, u64, Value)> {
    let entered = epoch_ms();
    let start = Instant::now();
    let mut headers = vec![("referer", frame_url), ("accept", "*/*".into())];
    let task_deadline = if task.kind == "integrity" {
        headers.extend([
            ("origin", "https://challenges.cloudflare.com".into()),
            ("cookie", "".into()),
            ("sec-fetch-site", "same-site".into()),
            ("sec-fetch-mode", "cors".into()),
            ("sec-fetch-dest", "empty".into()),
            ("cache-control", "no-cache".into()),
            ("pragma", "no-cache".into()),
        ]);
        deadline.min(Instant::now() + Duration::from_secs(2))
    } else {
        if task.kind == "pat" {
            headers.push(("cache-control", "max-age=0".into()))
        }
        deadline
    };
    let response = session
        .request("GET", &task.url, None, &headers, task_deadline)
        .await;
    let duration = start.elapsed().as_millis() as u64;
    let finished = epoch_ms();
    let (response, fields) = match response {
        Ok(response) => {
            let fields = match task.kind.as_str() {
                "integrity" => integrity(response.status, &response.headers),
                "pat" => task.pat_fields(response.status, duration)?,
                "ci" => {
                    require(response.status == 200, "CI HTTP status")?;
                    task.ci_fields(&response.content, "4g")?
                }
                _ => return Err(Error::new("INVALID_DATA", "Unknown network task kind")),
            };
            (Some(response), fields)
        }
        Err(e) => {
            if task.kind != "integrity" || Instant::now() >= deadline {
                return Err(e);
            }
            (
                None,
                json!({"hkqe2":if e.code=="TIMEOUT"{"timeout"}else{"fetch_error"}}),
            )
        }
    };
    let record = json!({"kind":task.kind,"status":response.as_ref().map(|r|r.status),"bytes":response.as_ref().map(|r|r.content.len()),"elapsedMs":duration});
    Ok((task, fields, entered, finished, record))
}

impl Task {
    pub(crate) fn pat_fields(&self, status: u16, elapsed: u64) -> Result<Value> {
        require(
            self.kind == "pat" && (100..=599).contains(&status),
            "Invalid PAT measurements",
        )?;
        Ok(
            json!({"clAR9":format!("status_{status}"),"Vagc5":self.digest,"YaIrl5":self.timestamp,"vMoo7":elapsed}),
        )
    }
    pub(crate) fn ci_fields(&self, image: &[u8], connection_type: &str) -> Result<Value> {
        require(
            self.kind == "ci" && ["slow-2g", "2g", "3g", "4g"].contains(&connection_type),
            "Invalid CI connection profile",
        )?;
        let (w, h) = png(image, true)?;
        Ok(
            json!({"YbGSY2":w.to_string(),"UFJmW4":h.to_string(),"cbpN0":connection_type,"Xzwu7":"true","IiLN9":"true","GbXDz5":w,"pfRZ9":self.timestamp}),
        )
    }
}
