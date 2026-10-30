//! Separate, explicitly experimental adapter for the recognized JSD envelope.
use crate::{
    network::{Session, USER_AGENT},
    protocol::wire,
    re, require, string,
    syntax::static_data::{origin, unquote, validate_url},
    Error, Result,
};
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use serde_json::{json, Value};
use std::{collections::HashSet, time::Instant};
pub fn encode(value: &Value, alphabet: &str) -> Result<String> {
    encode_text(&serde_json::to_string(value)?, alphabet)
}
pub(crate) fn encode_text(text: &str, alphabet: &str) -> Result<String> {
    let mut symbols = alphabet.bytes().collect::<Vec<_>>();
    symbols.sort();
    let mut expected =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-$".to_vec();
    expected.sort();
    require(symbols == expected, "JSD alphabet")?;
    let bytes = text.as_bytes().to_vec();
    let compressed = if bytes.len() >= 128 {
        wire::deflate(&bytes)?
    } else {
        vec![]
    };
    let use_compressed = !compressed.is_empty() && compressed.len() < bytes.len();
    let mut data = vec![253, 1, u8::from(use_compressed)];
    data.extend(if use_compressed { compressed } else { bytes });
    let mut state = alphabet
        .bytes()
        .fold(2166136261u32, |a, b| (a ^ b as u32).wrapping_mul(16777619));
    if state == 0 {
        state = 2779062077
    }
    let a = alphabet.as_bytes();
    for (i, v) in data.iter_mut().enumerate() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *v ^= (state >> 24) as u8 ^ a[i % 64];
    }
    let base = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    Ok(STANDARD_NO_PAD
        .encode(data)
        .bytes()
        .map(|b| a[base.iter().position(|c| *c == b).unwrap()] as char)
        .collect())
}
pub async fn submit(
    s: &Session,
    page_url: &str,
    config: &Value,
    source: &crate::syntax::html::Document,
    deadline: Instant,
) -> Result<Value> {
    let source = source.clone();
    let info = crate::cpu::run(deadline, move || inspect(&source)).await?;
    require(
        info["codec"] == "deflate-xorshift-v1"
            && info["alphabet"].is_string()
            && info["version"].is_string()
            && info["oneshot_path"].is_string(),
        "Unknown JSD wire parameters",
    )?;
    let payload = payload_with_agent(config, page_url, USER_AGENT)?;
    let ray = string(&config["r"])?;
    let endpoint = format!(
        "{}/cdn-cgi/challenge-platform/h/{}{}{ray}",
        origin(page_url)?,
        string(&info["version"])?,
        string(&info["oneshot_path"])?
    );
    let response = s
        .request(
            "POST",
            &endpoint,
            Some(encode(&payload, string(&info["alphabet"])?)?.into_bytes()),
            &[
                ("content-type", "text/plain;charset=UTF-8".into()),
                ("origin", origin(page_url)?),
                ("referer", page_url.into()),
                ("accept", "*/*".into()),
                ("sec-fetch-site", "same-origin".into()),
                ("sec-fetch-mode", "cors".into()),
                ("sec-fetch-dest", "empty".into()),
            ],
            deadline,
        )
        .await?;
    require(
        (200..300).contains(&response.status),
        "JSD submission rejected",
    )?;
    Ok(
        json!({"status":response.status,"experimental":true,"cookieIssued":s.cookies.get_all().any(|c|c.name()=="cf_clearance")}),
    )
}

pub(crate) fn inspect(source: &str) -> Result<Value> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .map_err(|e| Error::new("PARSER_ERROR", e.to_string()))?;
    let tree = crate::cpu::parse(&mut parser, source)?;
    let mut alphabets = HashSet::new();
    let mut paths = HashSet::new();
    for node in crate::syntax::constants::walk(tree.root_node(), true) {
        crate::cpu::check()?;
        if !matches!(node.kind(), "string" | "template_string") {
            continue;
        }
        if let Ok(value) = unquote(&source[node.byte_range()]) {
            for candidate in value.split([';', ',']) {
                if candidate.len() == 65 {
                    let mut symbols = candidate.bytes().collect::<Vec<_>>();
                    symbols.sort();
                    let mut expected =
                        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-$"
                            .to_vec();
                    expected.sort();
                    if symbols == expected {
                        alphabets.insert(candidate.to_string());
                    }
                }
            }
            if value.starts_with("/jsd/oneshot/") && value.ends_with('/') {
                paths.insert(value);
            }
        }
    }
    let cfg = crate::syntax::static_data::object_data(source, "_cf_chl_opt")?
        .unwrap_or_else(|| json!({}));
    let versions: HashSet<_> = cfg
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_str)
        .filter(|v| matches!(*v, "b" | "g"))
        .collect();
    let unique = |set: HashSet<String>| {
        if set.len() == 1 {
            set.into_iter().next()
        } else {
            None
        }
    };
    let version = if versions.len() == 1 {
        versions.into_iter().next().map(String::from)
    } else {
        None
    };
    Ok(
        json!({"sha256":crate::tasks::models::sha(source),"alphabet":unique(alphabets),"version":version,"oneshot_path":unique(paths),"codec":if ["2166136261","16777619","2779062077","Uint8Array"].iter().all(|v|source.contains(v)){Some("deflate-xorshift-v1")}else{None}}),
    )
}

pub(crate) fn payload_with_agent(
    config: &Value,
    page_url: &str,
    user_agent: &str,
) -> Result<Value> {
    let ray = string(&config["r"])?;
    require(re(r"^[a-fA-F0-9]{16,32}$").is_match(ray), "JSD ray")?;
    let timestamp = string(&config["t"])?;
    require(timestamp.len() <= 64, "JSD timestamp length")?;
    let raw = STANDARD.decode(timestamp)?;
    let timestamp = std::str::from_utf8(&raw)
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n >= 0.)
        .ok_or_else(|| Error::new("INVALID_JSD_CONFIG", "时间戳无效"))?;
    let mut profile: Value =
        serde_json::from_str(include_str!("../../profiles/jsd-chrome150.json"))?;
    for (value, fields) in [
        (origin(page_url)?, vec!["origin"]),
        (user_agent.into(), vec!["n.userAgent"]),
        (
            user_agent.strip_prefix("Mozilla/").unwrap().into(),
            vec!["n.appVersion"],
        ),
        (
            validate_url(page_url)?.host_str().unwrap().into(),
            vec!["d.domain"],
        ),
        (page_url.into(), vec!["d.referrer", "d.baseURI"]),
        (
            chrono::Local::now().format("%m/%d/%Y %H:%M:%S").to_string(),
            vec!["d.lastModified"],
        ),
    ] {
        profile
            .as_object_mut()
            .unwrap()
            .entry(value)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap()
            .extend(fields.into_iter().map(|v| json!(v)));
    }
    Ok(
        json!({"t":timestamp.floor()as u64,"lhr":"about:blank","api":config["api"].as_bool().unwrap_or(false),"c":true,"payload":profile}),
    )
}
