//! Classify the declared WebIDL catalogue using fresh page and client facts.
use crate::{
    array, epoch_ms, require, string,
    syntax::static_data::{origin, validate_url},
    tasks::math::number,
    tasks::models::{has, Context, Results},
    Result,
};
use serde_json::{json, Map, Value};
use std::{collections::HashSet, sync::OnceLock};
fn kind(name: &str) -> Value {
    json!({"declarationKind":name})
}
fn declared(member: &Value) -> Result<Value> {
    if member["kind"] == "operation" {
        return Ok(kind("N"));
    }
    if member["kind"] == "const" {
        let v = &member["value"];
        if let Some(s) = v.as_str() {
            if matches!(s, "Infinity" | "-Infinity" | "NaN") {
                return Ok(kind(s));
            }
            if let Some(s) = s.strip_prefix("0x") {
                return Ok(json!(u64::from_str_radix(s, 16).unwrap_or_default()));
            }
            return Ok(json!(s.parse::<i64>().unwrap_or_default()));
        }
        return Ok(v.clone());
    }
    let t = &member["type"];
    let name = t["name"].as_str().unwrap_or("undefined");
    if t["nullable"] == true
        || [
            "EventHandler",
            "EventHandlerNonNull",
            "OnErrorEventHandlerNonNull",
        ]
        .contains(&name)
    {
        return Ok(Value::Null);
    }
    Ok(match name {
        "boolean" => json!(false),
        "byte"
        | "octet"
        | "short"
        | "unsigned short"
        | "long"
        | "unsigned long"
        | "long long"
        | "unsigned long long"
        | "float"
        | "double"
        | "unrestricted double"
        | "unrestricted float" => json!(0),
        "DOMString" | "USVString" | "ByteString" | "CSSOMString" => json!(""),
        "undefined" => kind("u"),
        _ => kind("o"),
    })
}
fn group(value: &Value) -> Result<String> {
    if let Some(k) = value.get("declarationKind").and_then(Value::as_str) {
        return Ok(k.into());
    }
    Ok(match value {
        Value::Null => "x".into(),
        Value::Bool(b) => if *b { "T" } else { "F" }.into(),
        Value::Number(n) => number(n.as_f64().unwrap()),
        Value::Array(a) => a
            .iter()
            .map(|v| {
                if let Some(s) = v.as_str() {
                    s.to_string()
                } else {
                    v.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::String(s) => s.clone(),
        _ => {
            return Err(crate::Error::new(
                "INVALID_DATA",
                "Unrecognized declared capability",
            ))
        }
    })
}
fn omitted(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        let s = s.trim();
        s.is_empty() || s.parse::<f64>().is_ok_and(|n| !n.is_nan())
    })
}
pub fn calculate(c: &Context<'_>, results: &mut Results) -> Result<()> {
    for m in &c.catalog.modules {
        crate::cpu::check()?;
        let values: HashSet<_> = c
            .response
            .between(m.start, m.end)
            .iter()
            .map(|l| l.value.as_str())
            .collect();
        if has(&values, &["lxzW3", "VYOH2", "gMHaY6", "n.", "d.", "so."]) {
            let entered = epoch_ms();
            results.insert(
                &m.id,
                "native-declaration-surface-classification",
                classify(c)?,
                entered,
            );
        }
    }
    Ok(())
}
fn classify(c: &Context<'_>) -> Result<Value> {
    classify_with(
        &crate::tasks::identity::Identity::transport(),
        c.frame_url,
        &json!({}),
        &c.plan.meta["declared_properties"],
    )
}
pub(crate) fn window_values(frame_url: &str, profile: &Value) -> Result<Value> {
    let mut fields = Map::new();
    for (name, default) in [
        ("screen_width", 1920),
        ("screen_height", 1080),
        ("panel_height", 27),
        ("viewport_width", 300),
        ("viewport_height", 150),
        ("pixel_ratio", 1),
        ("color_depth", 24),
    ] {
        let value = profile
            .get(name)
            .map(crate::uint)
            .transpose()?
            .unwrap_or(default);
        require(value <= 32768, "Invalid declared surface dimensions")?;
        fields.insert(name.into(), json!(value));
    }
    let n = |k: &str| fields[k].as_u64().unwrap();
    require(
        n("panel_height") < n("screen_height") && n("pixel_ratio") >= 1,
        "Invalid declared surface geometry",
    )?;
    Ok(
        json!({"innerWidth":n("viewport_width"),"innerHeight":n("viewport_height"),"outerWidth":n("screen_width"),"outerHeight":n("screen_height")-n("panel_height"),"devicePixelRatio":n("pixel_ratio"),"s.width":n("screen_width"),"s.height":n("screen_height"),"s.availWidth":n("screen_width"),"s.availHeight":n("screen_height")-n("panel_height"),"s.colorDepth":n("color_depth"),"s.pixelDepth":n("color_depth"),"isSecureContext":frame_url.starts_with("https:"),"origin":origin(frame_url)?}),
    )
}
pub(crate) fn classify_with(
    identity: &crate::tasks::identity::Identity,
    frame_url: &str,
    profile: &Value,
    declarations: &Value,
) -> Result<Value> {
    static IDL: OnceLock<Value> = OnceLock::new();
    static API: OnceLock<Value> = OnceLock::new();
    let data = IDL.get_or_init(|| {
        serde_json::from_str(include_str!("../../profiles/webidl-surface-v1.json")).unwrap()
    });
    let api = API.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../profiles/browser-api-declarations-v1.json"
        ))
        .unwrap()
    });
    require(
        data["version"] == 1 && api["version"] == 1,
        "Capability catalogue version",
    )?;
    let mut props = Map::new();
    for (interface, prefix) in [
        ("Window", ""),
        ("Navigator", "n."),
        ("Document", "d."),
        ("Screen", "s."),
        ("ScreenOrientation", "so."),
    ] {
        let standard: std::collections::HashMap<_, _> = array(&data["roots"][interface])?
            .iter()
            .map(|m| Ok((string(&m["name"])?, m)))
            .collect::<Result<_>>()?;
        for (name, member) in api["roots"][interface].as_object().unwrap() {
            let descriptor = member["descriptor"].as_str().unwrap_or("");
            let value = if descriptor.starts_with("vf") {
                kind("N")
            } else if descriptor.starts_with('v') && member.get("value").is_some() {
                member["value"].clone()
            } else if let Some(m) = standard.get(name.as_str()) {
                declared(m)?
            } else if name.starts_with("on") {
                Value::Null
            } else {
                kind("o")
            };
            props.insert(prefix.to_string() + name, value);
        }
    }
    let window_standard: std::collections::HashMap<_, _> = array(&data["roots"]["Window"])?
        .iter()
        .map(|m| Ok((string(&m["name"])?, m)))
        .collect::<Result<_>>()?;
    for name in array(&api["windowNames"])? {
        let name = string(name)?;
        if props.contains_key(name) {
            continue;
        }
        let value = if let Some(m) = window_standard.get(name) {
            declared(m)?
        } else if name.chars().next().is_some_and(char::is_uppercase) || name.starts_with("webkit")
        {
            kind("N")
        } else if name.starts_with("on") {
            Value::Null
        } else {
            kind("o")
        };
        props.insert(name.into(), value);
    }
    for name in "Object Function Boolean Symbol Error AggregateError EvalError RangeError ReferenceError SyntaxError TypeError URIError SuppressedError Number BigInt Date String RegExp Array Int8Array Uint8Array Uint8ClampedArray Int16Array Uint16Array Int32Array Uint32Array BigInt64Array BigUint64Array Float16Array Float32Array Float64Array Map Set WeakMap WeakSet ArrayBuffer DataView Atomics JSON Promise Reflect Proxy Intl WebAssembly WeakRef FinalizationRegistry Iterator DisposableStack AsyncDisposableStack eval isFinite isNaN parseFloat parseInt decodeURI decodeURIComponent encodeURI encodeURIComponent escape unescape".split_whitespace(){props.insert(name.into(),kind("N"));}
    props.shift_remove("TestUtils");
    for name in array(&data["namespaces"])? {
        props.insert(string(name)?.into(), kind("o"));
    }
    for name in "window self document location top parent frames navigator screen visualViewport performance console crypto globalThis JSON Math Intl Reflect Atomics WebAssembly".split_whitespace(){props.insert(name.into(),kind("o"));}
    for name in "length scrollX scrollY screenX screenY screenLeft screenTop pageXOffset pageYOffset TEMPORARY".split_whitespace(){props.insert(name.into(),json!(0));}
    props.extend(json!({"innerWidth":300,"innerHeight":150,"outerWidth":1920,"outerHeight":1053,"devicePixelRatio":1,"PERSISTENT":1,"isSecureContext":frame_url.starts_with("https:"),"origin":origin(frame_url)?,"name":"","status":"","closed":false,"opener":null,"crossOriginIsolated":false,"credentialless":false}).as_object().unwrap().clone());
    props.insert("NaN".into(), kind("NaN"));
    props.insert("Infinity".into(), kind("Infinity"));
    props.insert("undefined".into(), kind("u"));
    props.insert("Array".into(), kind("p5"));
    let updates = json!({"n.userAgent":identity.user_agent,"n.appVersion":identity.user_agent.strip_prefix("Mozilla/").unwrap_or(&identity.user_agent),"n.platform":identity.worker_platform,"n.language":identity.languages[0],"n.languages":identity.languages,"n.hardwareConcurrency":identity.cores,"n.deviceMemory":identity.memory_gib,"n.maxTouchPoints":0,"n.webdriver":false,"n.cookieEnabled":true,"n.onLine":true,"n.vendor":"Google Inc.","n.appCodeName":"Mozilla","n.appName":"Netscape","n.product":"Gecko","n.productSub":"20030107","n.vendorSub":"","n.doNotTrack":null,"n.pdfViewerEnabled":true,"n.deprecatedRunAdAuctionEnforcesKAnonymity":false,"d.URL":"about:blank","d.documentURI":"about:blank","d.baseURI":frame_url,"d.referrer":frame_url,"d.domain":validate_url(frame_url)?.host_str(),"d.compatMode":"BackCompat","d.characterSet":"UTF-8","d.charset":"UTF-8","d.inputEncoding":"UTF-8","d.contentType":"text/html","d.readyState":"complete","d.nodeName":"#document","d.nodeType":9,"d.childElementCount":1,"d.isConnected":true,"d.hidden":false,"d.webkitHidden":false,"d.visibilityState":"visible","d.lastModified":chrono::Local::now().format("%m/%d/%Y %H:%M:%S").to_string(),"d.designMode":"off","d.webkitVisibilityState":"visible","d.fullscreenEnabled":true,"d.webkitFullscreenEnabled":true,"d.pictureInPictureEnabled":true,"s.width":1920,"s.height":1080,"s.availWidth":1920,"s.availHeight":1053,"s.availLeft":0,"s.availTop":0,"s.colorDepth":24,"s.pixelDepth":24,"so.angle":0,"so.type":"landscape-primary","s.isExtended":false,"originAgentCluster":true,"offscreenBuffering":true});
    props.extend(updates.as_object().unwrap().clone());
    props.extend(
        window_values(frame_url, profile)?
            .as_object()
            .unwrap()
            .clone(),
    );
    for name in [
        "n.userAgentData",
        "n.connection",
        "n.keyboard",
        "n.plugins",
        "n.mimeTypes",
    ] {
        props.insert(name.into(), kind("o"));
    }
    props.insert("d.cookie".into(), kind("s"));
    props.insert("event".into(), kind("u"));
    for name in "activeElement body customElementRegistry defaultView documentElement firstChild firstElementChild head lastChild lastElementChild scrollingElement location".split_whitespace(){props.insert("d.".to_string()+name,kind("o"));}
    for name in [
        "webkitCurrentFullScreenElement",
        "webkitFullscreenElement",
        "xmlEncoding",
        "xmlVersion",
    ] {
        props.insert("d.".to_string() + name, Value::Null);
    }
    props.insert("Temporal".into(), kind("o"));
    props.insert("frameElement".into(), kind("o"));
    props.insert("d.adoptedStyleSheets".into(), json!([]));
    props.insert("d.webkitIsFullScreen".into(), json!(false));
    props.insert("d.xmlStandalone".into(), json!(false));
    for name in [
        "find",
        "addEventListener",
        "dispatchEvent",
        "removeEventListener",
        "when",
    ] {
        props.insert(name.into(), kind("N"));
    }
    for name in [
        "AudioContext",
        "OfflineAudioContext",
        "webkitAudioContext",
        "webkitOfflineAudioContext",
        "RTCPeerConnection",
        "webkitRTCPeerConnection",
        "mozRTCPeerConnection",
        "PerformanceObserver",
        "localStorage",
        "sessionStorage",
        "n.storage",
    ] {
        props.shift_remove(name);
    }
    let mut groups = Map::<String, Value>::new();
    for (name, value) in props {
        if omitted(&value) {
            continue;
        }
        groups
            .entry(group(&value)?)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap()
            .push(json!(name));
    }
    if declarations.is_object() {
        let mut parent = declarations.as_object().cloned().unwrap_or_default();
        parent.extend(
        json!({"innerWidth":profile["viewport_width"].as_u64().unwrap_or(300),"innerHeight":65,"screenY":14,"screenTop":14,"vvfHL3":[]})
            .as_object()
            .unwrap()
            .clone(),
    );
        parent.insert("Array".into(), kind("N"));
        parent.insert("frameElement".into(), kind("u"));
        for (name, value) in parent {
            if omitted(&value) {
                continue;
            }
            let names = groups
                .entry(group(&value)?)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .unwrap();
            if !names.iter().any(|v| v == &name) {
                names.push(json!("o.".to_string() + &name));
            }
        }
    }
    Ok(
        json!({"lxzW3":groups,"SgRiW9":{"gMHaY6":false,"VYOH2":frame_url.starts_with("https:"),"FaCsJ8":"denied","jUnXe1":null,"HiXK5":0,"bIWk0":0}}),
    )
}
