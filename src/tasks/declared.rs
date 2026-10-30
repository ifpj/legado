//! Explicit inputs for the finite software model. These are not browser measurements.
use crate::{array, re, require, string, tasks::math, tasks::models::sha, uint, Error, Result};
use chrono::{Datelike, Offset, TimeZone};
use serde_json::{json, Map, Value};
use std::{collections::HashSet, sync::OnceLock};

pub fn allocator(profile: &Value, program_bytes: usize, texts: &[String]) -> Result<Vec<[u64; 3]>> {
    let defaults = [
        ("base_bytes", 32 * 1048576),
        ("object_header", 64),
        ("temporary_bytes", 64 * 1048576),
        ("reserve_bytes", 16 * 1048576),
        ("reclaimed_reserve", 8 * 1048576),
        ("tail_bytes", 524288),
        ("old_generation_bytes", 768 * 1048576),
        ("semispace_bytes", 8 * 1048576),
    ];
    let mut fields = std::collections::HashMap::new();
    for (name, default) in defaults {
        let n = profile.get(name).map(uint).transpose()?.unwrap_or(default);
        require(n <= 1024 * 1048576, "Allocator profile bound")?;
        fields.insert(name, n as u64);
    }
    require(
        (1..=1048576).contains(&program_bytes)
            && texts.len() <= 10000
            && texts.iter().all(|s| s.chars().count() <= 65536),
        "Allocator input bound",
    )?;
    let live = fields["base_bytes"]
        + program_bytes as u64
        + texts
            .iter()
            .map(|s| fields["object_header"] + 2 * s.encode_utf16().count() as u64)
            .sum::<u64>();
    let peak = live + fields["temporary_bytes"];
    let total = peak.div_ceil(1048576) * 1048576 + fields["reserve_bytes"];
    let limit = fields["old_generation_bytes"] + 3 * fields["semispace_bytes"];
    require(
        total + fields["reclaimed_reserve"] < limit,
        "Allocator capacity exceeded",
    )?;
    let mut out = vec![[total, peak, limit]; 4];
    out.extend([
        [total + fields["reclaimed_reserve"], live, limit],
        [
            total + fields["reclaimed_reserve"],
            live + fields["tail_bytes"],
            limit,
        ],
    ]);
    Ok(out)
}
pub fn preferences(profile: &Value) -> Result<Value> {
    let mut out = Map::new();
    for (name, key) in [
        "dark",
        "forced_colors",
        "contrast",
        "reduced_motion",
        "inverted_colors",
        "reduced_data",
        "reduced_transparency",
    ]
    .into_iter()
    .zip([
        "scji4", "EEDTe0", "rOlH2", "xXps5", "oUSFk5", "qdpwl2", "EuVBe7",
    ]) {
        let value = profile
            .get(name)
            .map(|v| {
                v.as_bool()
                    .ok_or_else(|| Error::new("INVALID_DATA", "Preference must be boolean"))
            })
            .transpose()?
            .unwrap_or(false);
        out.insert(key.into(), json!(value));
    }
    Ok(json!(out))
}
pub fn preference_fields(values: &HashSet<&str>, page: &str, profile: &Value) -> Result<Value> {
    let expected = HashSet::from([
        "(prefers-color-scheme: dark)",
        "(forced-colors: active)",
        "(prefers-contrast: more)",
        "(prefers-contrast: less)",
        "(prefers-contrast: custom)",
        "(prefers-reduced-motion: reduce)",
        "(inverted-colors: inverted)",
        "(prefers-reduced-data: reduce)",
        "(prefers-reduced-transparency: reduce)",
    ]);
    let actual = values
        .iter()
        .copied()
        .filter(|s| s.starts_with('(') && s.ends_with(')'))
        .collect::<HashSet<_>>();
    require(
        actual == expected && values.contains("geolocationAnotificationsAcameraAmicrophone"),
        "Unsupported preference query or permission",
    )?;
    let mut out = preferences(profile)?;
    let permissions = ["geolocation", "notifications", "camera", "microphone"]
        .map(|name| json!({"name":name,"state":"denied"}));
    out.as_object_mut().unwrap().extend(json!({"UJhTk5":"navigate","dZVkx0":page,"GMvA7":[null,null,null,null],"YSycG6":false,"YWjJ7":serde_json::to_string(&permissions)?}).as_object().unwrap().clone());
    Ok(out)
}
pub fn connection(profile: &Value) -> Result<Value> {
    let down = profile
        .get("downlink_mbps")
        .map(|v| {
            v.as_f64()
                .filter(|n| n.is_finite() && *n >= 0. && *n <= 10000.)
                .ok_or_else(|| Error::new("INVALID_DATA", "Connection downlink"))
        })
        .transpose()?
        .unwrap_or(10.);
    let rtt = profile.get("rtt_ms").map(uint).transpose()?.unwrap_or(50);
    require(rtt <= 60000, "Connection RTT")?;
    let effective = profile
        .get("effective_type")
        .map(string)
        .transpose()?
        .unwrap_or("4g");
    require(
        ["slow-2g", "2g", "3g", "4g"].contains(&effective),
        "Connection type",
    )?;
    let save = profile
        .get("save_data")
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| Error::new("INVALID_DATA", "Connection saveData"))
        })
        .transpose()?
        .unwrap_or(false);
    Ok(json!({"iOxd5":down,"QmaFT8":null,"gKCe1":effective,"ZYYjL6":rtt,"ddSA0":save,"dVmv6":null}))
}
pub fn style_digest(width: usize, height: usize, profile: Option<&Value>) -> Result<Value> {
    require(width <= 16 && height <= 16, "Unsupported initial geometry")?;
    static PROFILE: OnceLock<Value> = OnceLock::new();
    let p = profile.unwrap_or_else(|| {
        PROFILE.get_or_init(|| {
            serde_json::from_str(include_str!("../../profiles/initial-css-values-v1.json"))
                .expect("initial CSS profile")
        })
    });
    require(
        p["version"] == 1
            && array(&p["indexedProperties"])?.len() == 475
            && array(&p["defaultProperties"])?.len() == 736,
        "Initial CSS profile format",
    )?;
    let mut out = Map::new();
    for (i, name) in array(&p["indexedProperties"])?.iter().enumerate() {
        out.insert(i.to_string(), name.clone());
    }
    for pair in array(&p["defaultProperties"])? {
        let value = if pair[1].is_object() {
            require(
                pair[1].as_object().unwrap().len() == 1,
                "Initial geometry resolver",
            )?;
            json!(match string(&pair[1]["resolve"])? {
                "origin" => "0px 0px",
                "width" | "height" => "0px",
                _ => return Err(Error::new("INVALID_DATA", "Initial geometry resolver")),
            })
        } else {
            pair[1].clone()
        };
        out.insert(string(&pair[0])?.into(), value);
    }
    Ok(json!({"AXwP6":sha(serde_json::to_vec(&out)?)[..32]}))
}
pub fn calendar(years: &Value, name: &str, now: chrono::DateTime<chrono::Utc>) -> Result<Value> {
    let names = ["gPhvu9", "HIxfx6", "OZHY7", "FSbs2", "LWwMX3"];
    require(
        years
            .as_object()
            .is_some_and(|v| v.len() == 5 && names.iter().all(|k| v.contains_key(*k)))
            && name.len() <= 128,
        "Calendar operands",
    )?;
    let zone = name
        .parse::<chrono_tz::Tz>()
        .map_err(|_| Error::new("INVALID_DATA", "Unknown timezone"))?;
    let mut out = Map::new();
    for key in names {
        let year = string(&years[key])?;
        require(
            re(r"^[1-9][0-9]{2,3}$").is_match(year),
            "Calendar year syntax",
        )?;
        let date = zone
            .with_ymd_and_hms(year.parse().unwrap(), 12, 1, 0, 0, 0)
            .earliest()
            .ok_or_else(|| Error::new("INVALID_DATA", "Calendar instant"))?;
        out.insert(
            key.into(),
            json!(if year.len() == 4 {
                date.offset().fix().local_minus_utc() / 60
            } else {
                0
            }),
        );
    }
    let local = now.with_timezone(&zone);
    let midnight = zone
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .earliest()
        .ok_or_else(|| Error::new("INVALID_DATA", "Midnight"))?;
    out.insert(
        "Lkgd1".into(),
        json!(-midnight.offset().fix().local_minus_utc() / 60),
    );
    out.insert(
        "UCXp5".into(),
        json!(-local.offset().fix().local_minus_utc() / 60),
    );
    out.insert("cMggP6".into(), json!(name));
    Ok(json!(out))
}
pub fn console(methods: &[String]) -> Result<Value> {
    let allowed = HashSet::from([
        "OvQPy9", "rxMM2", "RCFl4", "elYp1", "OVdg4", "jZNT8", "aUpc1", "XuJJ1", "HRNDa0",
    ]);
    require(
        !methods.is_empty()
            && methods.len() <= 9
            && methods.iter().all(|v| allowed.contains(v.as_str()))
            && methods.iter().collect::<HashSet<_>>().len() == methods.len(),
        "Console method declaration",
    )?;
    let mut trace = vec![
        json!("fAhw2"),
        json!("OROhR9"),
        json!("RmGBF2"),
        json!("dzUp7"),
        json!("nJkL5"),
        json!("YCge9"),
        json!("Hsul6"),
        json!(21),
        json!("hipz3"),
        json!("hipz3"),
        json!("mvyF4"),
        json!("mvyF4"),
    ];
    for m in methods {
        for _ in 0..2 {
            trace.extend([json!("OROhR9"), json!(m), json!("RmGBF2"), json!(m)]);
        }
    }
    trace.extend([
        json!("OROhR9"),
        json!("dbLIw8"),
        json!("RmGBF2"),
        json!("dbLIw8"),
        json!("dzUp7"),
        json!("dbLIw8"),
    ]);
    Ok(json!({"iJDrV5":trace,"ZOif6":"YPqu7"}))
}
pub fn clock(samples: usize, mut now: impl FnMut() -> f64) -> Result<Value> {
    require((1..=5000).contains(&samples), "Clock sampling bounds")?;
    let (mut first, mut second) = (1., 1.);
    for _ in 0..samples {
        let a = now();
        let b = now();
        if a < b {
            let delta = b - a;
            if first < delta && delta < second {
                second = delta
            } else if delta < first {
                second = first;
                first = delta;
            }
        }
    }
    Ok(json!({"clAR9":math::number(first)}))
}
