//! Explicit HTTP identity shared by transport and finite task calculations.
use crate::{
    epoch_ms,
    program::catalog::Catalog,
    program::data::Response,
    re, require,
    tasks::models::{has, Results},
    Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub user_agent: String,
    pub version: String,
    pub platform: String,
    pub worker_platform: String,
    pub languages: Vec<String>,
    pub brands: Vec<(String, String)>,
    pub cores: usize,
    pub memory_gib: usize,
}
impl Identity {
    pub fn from_headers(headers: &Map<String, Value>) -> Result<Self> {
        let h = headers
            .iter()
            .map(|(k, v)| Ok((k.to_ascii_lowercase(), crate::string(v)?.to_string())))
            .collect::<Result<HashMap<_, _>>>()?;
        let ua = h.get("user-agent").map(String::as_str).unwrap_or("");
        require(ua.len() <= 512, "Native identity length")?;
        let pattern = re(r"\bChrome/([0-9]{2,3}\.[0-9]+\.[0-9]+\.[0-9]+)\b");
        let capture = pattern
            .captures(ua)
            .ok_or_else(|| crate::Error::new("INVALID_DATA", "Unsupported native identity"))?;
        let version = capture[1].to_string();
        let (platform, worker) = if ua.contains("Windows NT ") {
            ("Windows", "Win32")
        } else if ua.contains("X11; Linux x86_64") {
            ("Linux", "Linux x86_64")
        } else {
            return Err(crate::Error::new(
                "INVALID_DATA",
                "Unsupported desktop platform",
            ));
        };
        require(
            h.get("sec-ch-ua-platform")
                .is_none_or(|v| *v == format!("\"{platform}\"")),
            "Identity platform contradicts headers",
        )?;
        let mut languages = Vec::new();
        for s in h
            .get("accept-language")
            .map(String::as_str)
            .unwrap_or("en-US,en;q=0.9")
            .split(',')
        {
            let s = s.split(';').next().unwrap().trim();
            require(
                re(r"^[A-Za-z]{2,3}(?:-[A-Za-z0-9]{2,8})*$").is_match(s),
                "Invalid native language",
            )?;
            if !languages.iter().any(|v| v == s) {
                languages.push(s.to_string());
            }
        }
        require(
            (1..=8).contains(&languages.len()),
            "Identity languages bound",
        )?;
        let brands = re(r#""([^"\\]{1,40})"\s*;\s*v="([0-9]{1,3})""#)
            .captures_iter(h.get("sec-ch-ua").map(String::as_str).unwrap_or(""))
            .map(|c| (c[1].into(), c[2].into()))
            .collect::<Vec<(String, String)>>();
        require(
            !brands.is_empty()
                && brands.iter().all(|(brand, v)| {
                    !matches!(brand.as_str(), "Chromium" | "Google Chrome")
                        || v == version.split('.').next().unwrap()
                }),
            "Identity version contradicts headers",
        )?;
        Ok(Self {
            user_agent: ua.into(),
            version,
            platform: platform.into(),
            worker_platform: worker.into(),
            languages,
            brands,
            cores: 8,
            memory_gib: 8,
        })
    }
    pub fn transport() -> Self {
        Self::from_headers(
            &crate::network::IDENTITY_HEADERS
                .iter()
                .map(|(k, v)| (k.to_string(), json!(v)))
                .collect(),
        )
        .expect("internal HTTP identity")
    }
    pub fn navigator_fields(&self) -> Value {
        json!({"wKFQX4":0,"tuTSa2":{"WDRmZ7":self.worker_platform,"pjey6":self.languages,"XVUCi3":self.cores,"ipnj0":self.memory_gib,"wZXOJ3":self.user_agent}})
    }
    pub fn client_hint_fields(&self) -> Value {
        json!({"QxBk9":{"architecture":"x86","bitness":"64","brands":self.brands.iter().map(|(b,v)|json!({"brand":b,"version":v})).collect::<Vec<_>>(),"fullVersionList":self.brands.iter().map(|(b,v)|json!({"brand":b,"version":if matches!(b.as_str(),"Chromium"|"Google Chrome"){self.version.clone()}else{format!("{v}.0.0.0")}})).collect::<Vec<_>>(),"mobile":false,"model":"","platform":self.platform,"platformVersion":""}})
    }
    pub fn keyboard_fields(&self) -> Value {
        let mut keys = Map::new();
        for c in 'A'..='Z' {
            keys.insert(format!("Key{c}"), json!(c.to_ascii_lowercase().to_string()));
        }
        for c in '0'..='9' {
            keys.insert(format!("Digit{c}"), json!(c.to_string()));
        }
        for (key, value) in [
            "Backquote",
            "Minus",
            "Equal",
            "BracketLeft",
            "BracketRight",
            "Backslash",
            "Semicolon",
            "Quote",
            "Comma",
            "Period",
            "Slash",
            "IntlBackslash",
        ]
        .into_iter()
        .zip(["`", "-", "=", "[", "]", "\\", ";", "'", ",", ".", "/", "<"])
        {
            keys.insert(key.into(), json!(value));
        }
        json!({"XQwu5":"en-us","tGnD8":keys})
    }
    pub fn calculate(&self, r: &Response, c: &Catalog, out: &mut Results) -> Result<()> {
        let mut used = HashSet::new();
        for m in &c.modules {
            let values = r
                .between(m.start, m.end)
                .iter()
                .map(|l| l.value.as_str())
                .collect::<HashSet<_>>();
            let matches = [
                has(&values, &["wKFQX4", "WDRmZ7", "XVUCi3"]),
                values.contains("QxBk9"),
                has(&values, &["XQwu5", "tGnD8"]),
            ];
            require(
                matches.iter().filter(|v| **v).count() <= 1,
                "Native identity task ownership is ambiguous",
            )?;
            if let Some(i) = matches.iter().position(|v| *v) {
                require(used.insert(i), "Duplicate native identity task")?;
                let kind = ["navigator", "client-hints", "keyboard"][i];
                out.insert(
                    &m.id,
                    kind,
                    match i {
                        0 => self.navigator_fields(),
                        1 => self.client_hint_fields(),
                        _ => self.keyboard_fields(),
                    },
                    epoch_ms(),
                );
            }
        }
        Ok(())
    }
}
