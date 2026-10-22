//! Explicit finite task recipes, calculated from this response's operands.
use crate::{
    array,
    catalog::Catalog,
    data::{Plan, Response},
    epoch_ms,
    html::Facts,
    math, operands, re, require, string, uint, Error, Result,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

pub fn has(values: &HashSet<&str>, required: &[&str]) -> bool {
    required.iter().all(|v| values.contains(v))
}
pub fn sha(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(bytes.as_ref()))
}
pub struct Context<'a> {
    pub response: &'a Response,
    pub plan: &'a Plan,
    pub catalog: &'a Catalog,
    pub html: &'a str,
    pub parent_html: &'a str,
    pub frame_url: &'a str,
    pub page_url: &'a str,
    pub mode: &'a str,
}
#[derive(Default)]
pub struct Results {
    pub fields: HashMap<String, Value>,
    pub timings: HashMap<String, (u64, u64)>,
    pub kinds: Vec<String>,
}
impl Results {
    pub fn insert(&mut self, id: &str, kind: &str, fields: Value, entered: u64) {
        self.fields.insert(id.into(), fields);
        self.timings.insert(id.into(), (entered, epoch_ms()));
        self.kinds.push(kind.into());
    }
}
pub fn calculate(context: &Context<'_>) -> Result<Results> {
    let r = context.response;
    let plan = context.plan;
    let mut results = Results::default();
    crate::identity::Identity::transport().calculate(r, context.catalog, &mut results)?;
    let document = Facts::read(context.html)?;
    let parent = Facts::read(context.parent_html)?;
    let allowed: HashSet<_> = [
        "KOmHB7", "ZLSD0", "YGZZe2", "aymS3", "SgRiW9", "sJvX2", "NjCPO1", "tikJ6", "ipjt9",
        "ORNm0", "PdJpg6", "QlZv9", "YqDCY7", "HTIj5",
    ]
    .into_iter()
    .collect();
    let properties = operands::properties(r, plan, &allowed)?;
    for m in &context.catalog.modules {
        if results.fields.contains_key(&m.id) {
            continue;
        }
        let entered = epoch_ms();
        let literals = r.between(m.start, m.end);
        let mut values: HashSet<_> = literals.iter().map(|l| l.value.as_str()).collect();
        values.extend(
            properties
                .iter()
                .filter(|(p, _)| m.start <= *p && *p < m.end)
                .map(|(_, v)| v.as_str()),
        );
        if has(&values, &["parseInt", "a", "b", "c", "d", "e", "f", "g"])
            && values
                .iter()
                .any(|v| v.contains("kcats") && v.contains("3Smya"))
        {
            values.insert("aymS3");
        }
        if has(
            &values,
            &["parseInt", "shift", "a", "b", "c", "d", "e", "f", "g", "h"],
        ) && plan.meta["build"] == "d2d3316b729e"
            && (1500..=4000).contains(&(m.end - m.start))
        {
            values.insert("aymS3");
        }
        if values.contains("zJGo6") {
            if let Some(seeds) = math::recipe(r, m, plan)? {
                require(
                    !results.kinds.iter().any(|k| k == "arithmetic"),
                    "Ambiguous arithmetic task ownership",
                )?;
                results.insert(&m.id, "arithmetic", math::calculate(seeds)?, entered);
            }
            continue;
        }
        if has(
            &values,
            &[
                "GMYd6",
                "ZnIRF1",
                "fayBq4",
                "GkwBC7",
                "sGUjD4",
                "[native code]",
                "toString",
            ],
        ) {
            require(
                !results
                    .kinds
                    .iter()
                    .any(|k| k == "native-self-check-declared-function-model"),
                "Ambiguous native self-check task",
            )?;
            results.insert(
                &m.id,
                "native-self-check-declared-function-model",
                function_checks(),
                entered,
            );
            continue;
        }
        if has(
            &values,
            &["gPhvu9", "HIxfx6", "OZHY7", "FSbs2", "LWwMX3", "cMggP6"],
        ) {
            results.insert(&m.id, "declared-timezone", calendar(&literals)?, entered);
            continue;
        }
        if let Some((kind, fields)) = environment(&values, &document, &parent, &json!({}), true)? {
            require(
                !results.kinds.iter().any(|k| k == kind),
                "Duplicate native environment task",
            )?;
            results.insert(&m.id, kind, fields, entered);
            continue;
        }
        if has(
            &values,
            &[
                "scji4",
                "EEDTe0",
                "GMvA7",
                "YSycG6",
                "YWjJ7",
                "(prefers-color-scheme: dark)",
            ],
        ) {
            results.insert(
                &m.id,
                "declared-preferences-permissions",
                crate::declared::preference_fields(&values, context.page_url, &json!({}))?,
                entered,
            );
            continue;
        }
        if values.contains("clAR9")
            && values.iter().any(|v| {
                v.starts_with("for(var a=1,b=1,c,d=0;5E3>d;") && v.ends_with("postMessage({ ")
            })
        {
            let origin = Instant::now();
            let fields = crate::declared::clock(5000, || {
                (origin.elapsed().as_nanos() / 100000) as f64 / 10.
            })?;
            results.insert(&m.id, "declared-clock-resolution", fields, entered);
            continue;
        }
        if has(&values, &["preJ6", "OCpH4", "MMzj1", "sCJrb8"]) {
            let samples: Vec<_> = values
                .iter()
                .filter(|s| {
                    s.chars().count() == 24
                        && s.chars().take(2).collect::<String>().repeat(12) == **s
                })
                .collect();
            require(samples.len() == 6, "Font comparison operands")?;
            results.insert(
                &m.id,
                "declared-font-capabilities",
                unavailable_fonts(&samples.iter().map(|s| s.to_string()).collect::<Vec<_>>())?,
                entered,
            );
            continue;
        }
        if has(
            &values,
            &[
                "AXwP6",
                "SecurityError",
                "<html><head></head><body></body></html>",
            ],
        ) {
            let (w, h) = operands::dimensions(r, m, plan)?;
            require(w <= 16 && h <= 16, "Unsupported initial geometry")?;
            results.insert(
                &m.id,
                "declared-initial-css-values",
                crate::declared::style_digest(w, h, None)?,
                entered,
            );
            continue;
        }
        if has(
            &values,
            &[
                "XAgyn2",
                "waxw9",
                "EuHm4",
                "EbfJf0",
                "JktAU1",
                "XEHEv1",
                "JCxy0",
                "createHTML",
                "createScript",
                "createScriptURL",
                "about:blank",
                "script-src 'unsafe-inline'",
            ],
        ) {
            let snapshots = crate::declared::allocator(
                &json!({}),
                r.program.byte_length,
                &r.literals
                    .iter()
                    .map(|l| l.value.clone())
                    .collect::<Vec<_>>(),
            )?;
            let fields: Vec<_> = snapshots
                .into_iter()
                .flat_map(|[a, b, c]| [base31(a as usize), base31(b as usize), base31(c as usize)])
                .collect();
            let cross = crate::static_data::origin(context.frame_url)? != "https://example.org";
            results.insert(&m.id,"declared-allocator-origin-policy",json!({"XAgyn2":true,"waxw9":fields,"EuHm4":"XEHEv1","EbfJf0":1|2|4,"zuWu8":cross,"JktAU1":if cross{"XEHEv1"}else{"JCxy0"}}),entered);
            continue;
        }
        if has(&values, &["BdOzr6", "Nihv6", "UqEc2", "kKOzy9"]) {
            let mut fields = parent.fields(context.page_url);
            let more = json!({"BdOzr6":"","ZOcup4":false,"aghc7":"wrapper","vVvyt8":"wrapper","Zoqe8":context.mode,"Nihv6":"always","kKOzy9":true,"UqEc2":"visible","hdNv7":"","UmNW6":"","NWobI6":"304|69","UDnCy3":true,"QvSt9":false,"cEgto8":"nf"});
            fields
                .as_object_mut()
                .unwrap()
                .extend(more.as_object().unwrap().clone());
            results.insert(&m.id, "native-html-declared-page-view", fields, entered);
            continue;
        }
        if let Some(fields) = console_recipe(r, m, Some(plan))? {
            require(
                !results
                    .kinds
                    .iter()
                    .any(|k| k == "declared-console-formatting"),
                "Duplicate console task",
            )?;
            results.insert(&m.id, "declared-console-formatting", fields, entered);
            continue;
        }
        for (packed_at, packed) in &r.packed {
            if m.start <= *packed_at && *packed_at < m.end && packed.starts_with('{') {
                if let Ok(v) = serde_json::from_str::<Value>(packed) {
                    if v.as_object().is_some_and(|v| {
                        v.len() == 3
                            && v.contains_key("n")
                            && v.contains_key("i")
                            && v.contains_key("l")
                    }) {
                        let (value, _) = ancestry(&v)?;
                        results.insert(&m.id, "ancestry", json!({"yrto5":value}), entered);
                        break;
                    }
                }
            }
        }
    }
    Ok(results)
}
pub(crate) fn environment(
    values: &HashSet<&str>,
    document: &Facts,
    parent: &Facts,
    connection: &Value,
    pdf_viewer: bool,
) -> Result<Option<(&'static str, Value)>> {
    let signatures: &[(&[&str], &str)] = &[
        (
            &["iOxd5", "gKCe1", "saveData", "effectiveType"],
            "connection",
        ),
        (
            &["NvRU8", "PdJpg6", "QlZv9", "JLxoi3"],
            "document-collections",
        ),
        (&["pYjop9", "PCCE0", "RwMi7"], "document-attributes"),
        (&["ZLSD0"], "summarizer"),
        (&["XqFR3", "JmlzT1", "HouO8", "iLkS0"], "disabled-storage"),
        (&["uIfe8", "TUYF7", "HRnR4"], "disabled-audio"),
        (&["SNOCj6", "lwIE5"], "disabled-webrtc"),
        (
            &["Uput2", "navigation", "resource"],
            "disabled-performance-observer",
        ),
        (&["aymS3", "parseInt"], "native-error-label"),
        (&["KOmHB7"], "code-point-error"),
    ];
    let matches = signatures
        .iter()
        .filter(|(signature, _)| has(values, signature))
        .collect::<Vec<_>>();
    require(
        matches.len() <= 1,
        "Native environment task ownership is ambiguous",
    )?;
    let Some((_, kind)) = matches.first() else {
        return Ok(None);
    };
    Ok(Some((
        *kind,
        match *kind {
            "connection" => crate::declared::connection(connection)?,
            "document-collections" => {
                json!({"NvRU8":"","PdJpg6":[document.links,document.images,document.forms,0],"QlZv9":vec!["object";6],"JLxoi3":!document.title.is_empty()})
            }
            "document-attributes" => {
                json!({"pYjop9":if pdf_viewer{"RpUV9"}else{"meLV9"},"ipjt9":parent.root_attributes,"ORNm0":parent.custom_tags,"PCCE0":document.body_attributes,"RwMi7":document.extension_comment})
            }
            "summarizer" => json!({"ZLSD0":"unavailable"}),
            "disabled-storage" => {
                json!({"XqFR3":false,"JmlzT1":null,"HouO8":false,"oxBK4":null,"iLkS0":null})
            }
            "disabled-audio" => json!({"uIfe8":"TUYF7"}),
            "disabled-webrtc" => json!({"SNOCj6":"lwIE5"}),
            "disabled-performance-observer" => json!({"Uput2":[]}),
            "native-error-label" => json!({"aymS3":"Error"}),
            "code-point-error" => json!({"KOmHB7":21}),
            _ => unreachable!(),
        },
    )))
}
pub(crate) fn base31(mut n: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b"0123456789abcdefghijklmnopqrstu"[n % 31]);
        n /= 31;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}
pub(crate) fn function_checks() -> Value {
    let cases: [fn() -> bool; 5] = [
        || std::hint::black_box(42).to_string() == "42",
        || std::hint::black_box("42").parse::<i32>() == Ok(42),
        || std::hint::black_box("abc").len() == 3,
        || sha(std::hint::black_box(b"")) == sha([]),
        || serde_json::to_string(&json!({})).unwrap() == "{}",
    ];
    self_checks(&cases, 185, 129, 128).expect("native check bounds")
}
pub(crate) fn self_checks(
    cases: &[fn() -> bool],
    count: usize,
    forms: usize,
    repetitions: usize,
) -> Result<Value> {
    require(
        (1..=32).contains(&cases.len())
            && (1..=256).contains(&count)
            && (1..=256).contains(&forms)
            && (1..=1024).contains(&repetitions),
        "Native self-check bounds",
    )?;
    let (mut times, mut failures) = (Vec::new(), Vec::new());
    for index in 0..count {
        let started = Instant::now();
        let mut valid = false;
        for _ in 0..repetitions {
            valid = cases[index % cases.len()]();
        }
        if !valid {
            failures.push(index)
        }
        times.push(
            ((started.elapsed().as_nanos() as f64 / repetitions as f64 / 100.).round() * 100.)
                as u64,
        );
    }
    Ok(json!({"tikJ6":[],"ZnIRF1":[],"GMYd6":failures,"ltXr7":vec!["sGUjD4";forms],"fayBq4":times}))
}
fn calendar(literals: &[&crate::data::Literal]) -> Result<Value> {
    calendar_at(
        literals,
        &std::env::var("CF_GATEWAY_TIMEZONE").unwrap_or_else(|_| "Asia/Shanghai".into()),
        chrono::Utc::now(),
    )
}
pub(crate) fn calendar_at(
    literals: &[&crate::data::Literal],
    zone: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Value> {
    let names = ["gPhvu9", "HIxfx6", "OZHY7", "FSbs2", "LWwMX3"];
    let mut years = Map::new();
    for name in names {
        let anchors: Vec<_> = literals.iter().filter(|l| l.value == name).collect();
        require(anchors.len() == 1, "Calendar field boundary")?;
        let at = anchors[0].start;
        let end = literals
            .iter()
            .filter(|l| l.start > at && names.contains(&l.value.as_str()))
            .map(|l| l.start)
            .min()
            .unwrap_or(at + 64)
            .min(at + 64);
        let values: Vec<_> = literals
            .iter()
            .filter(|l| {
                at < l.start && l.start < end && re(r"^[1-9][0-9]{2,3}$").is_match(&l.value)
            })
            .collect();
        require(values.len() == 1, "Calendar year ambiguity")?;
        years.insert(name.into(), json!(values[0].value));
    }
    crate::declared::calendar(&json!(years), zone, now)
}
pub(crate) fn console(literals: &[&crate::data::Literal]) -> Result<Value> {
    let markers = [
        "OvQPy9", "rxMM2", "RCFl4", "elYp1", "OVdg4", "jZNT8", "aUpc1", "XuJJ1", "HRNDa0",
    ];
    let methods: Vec<_> = literals
        .iter()
        .filter(|l| markers.contains(&l.value.as_str()))
        .map(|l| l.value.as_str())
        .collect();
    require(methods.len() == markers.len(), "Console method table")?;
    crate::declared::console(&methods.iter().map(|v| v.to_string()).collect::<Vec<_>>())
}

pub fn ancestry(v: &Value) -> Result<(String, Vec<String>)> {
    let definitions = array(&v["n"])?;
    require(
        !definitions.is_empty() && definitions.len() <= 128,
        "Ancestry count",
    )?;
    let (mut nodes, mut parents, mut titles, mut trace) = (
        Vec::<Value>::new(),
        Vec::<Option<usize>>::new(),
        Vec::<String>::new(),
        Vec::<String>::new(),
    );
    fn select(nodes: &[Value], selector: &str, trace: &mut Vec<String>) -> Result<usize> {
        require(
            re(r"^[.#][A-Za-z_][A-Za-z0-9_-]{0,79}$").is_match(selector),
            "Ancestry selector",
        )?;
        let matches: Vec<_> = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                if selector.starts_with('#') {
                    n["e"].as_str() == Some(&selector[1..])
                } else {
                    n["c"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(&selector[1..])))
                }
            })
            .map(|(i, _)| i)
            .collect();
        require(matches.len() == 1, "Ancestry selector ownership")?;
        trace.push(selector.into());
        Ok(matches[0])
    }
    for (i, node) in definitions.iter().enumerate() {
        require(
            matches!(node["t"].as_str(), Some("div" | "span")),
            "Ancestry node type",
        )?;
        let parent = if i > 0 {
            Some(select(&nodes, string(&node["p"])?, &mut trace)?)
        } else {
            None
        };
        nodes.push(node.clone());
        parents.push(parent);
        titles.push(String::new());
    }
    require(
        array(&v["i"])?.len() <= 1024 && array(&v["l"])?.len() <= 1024,
        "Ancestry operation bound",
    )?;
    for op in array(&v["i"])? {
        match uint(&op["k"])?{0=>{require(re(r"^[.#][A-Za-z_][A-Za-z0-9_-]{0,79}(?: +[.#][A-Za-z_][A-Za-z0-9_-]{0,79})*\{font-family:'[A-Za-z0-9_-]{1,80}'\}$").is_match(string(&op["v"])?),"Ancestry style input")?},1=>{let index=select(&nodes,string(&op["s"] )?,&mut trace)?;titles[index]=string(&op["v"] )?.into();},2=>{select(&nodes,string(&op["s"] )?,&mut trace)?;},_=>return Err(Error::new("INVALID_DATA","Ancestry operation"))}
    }
    let mut result = String::new();
    for query in array(&v["l"])? {
        let mut left = Some(select(&nodes, string(&query["a"])?, &mut trace)?);
        let mut right = Some(select(&nodes, string(&query["b"])?, &mut trace)?);
        let mut ancestors = HashSet::new();
        while let Some(n) = left {
            ancestors.insert(n);
            left = parents[n];
        }
        while right.is_some_and(|n| !ancestors.contains(&n)) {
            right = parents[right.unwrap()];
        }
        require(right.is_some(), "Ancestry common node missing")?;
        result.push_str(&titles[right.unwrap()]);
    }
    Ok((result, trace))
}

pub(crate) fn unavailable_fonts(samples: &[String]) -> Result<Value> {
    require(
        samples.len() == 6
            && samples.iter().all(|s| {
                s.chars().count() == 24 && s.chars().take(2).collect::<String>().repeat(12) == *s
            }),
        "Finite font comparison operands",
    )?;
    let widths = samples
        .iter()
        .map(|s| s.encode_utf16().count() as f64 * 6.7)
        .collect::<Vec<_>>();
    let diff = widths.iter().map(|v| (*v - widths[0]).abs()).sum::<f64>();
    let unique = widths
        .iter()
        .map(|v| v.to_bits())
        .collect::<HashSet<_>>()
        .len();
    Ok(
        json!({"preJ6":[diff,diff,diff],"OCpH4":unique-1,"JXhZM3":[],"KfgrY7":0,"yHRu3":"MMzj1","sCJrb8":sha([])}),
    )
}

pub(crate) fn console_recipe(
    r: &Response,
    m: &crate::catalog::Module,
    plan: Option<&Plan>,
) -> Result<Option<Value>> {
    let prefixes = HashSet::from([
        "fAhw2", "OROhR9", "RmGBF2", "dzUp7", "nJkL5", "YCge9", "Hsul6", "hipz3", "mvyF4",
        "dbLIw8", "YPqu7",
    ]);
    let methods = [
        "OvQPy9", "rxMM2", "RCFl4", "elYp1", "OVdg4", "jZNT8", "aUpc1", "XuJJ1", "HRNDa0",
    ];
    let literals = r.between(m.start, m.end);
    let mut values = literals
        .iter()
        .map(|l| l.value.clone())
        .collect::<HashSet<_>>();
    if !methods
        .iter()
        .chain(
            [
                "console",
                "stack",
                "toString",
                "__playwright_global_listeners_check__",
            ]
            .iter(),
        )
        .all(|v| values.contains(*v))
    {
        return Ok(None);
    }
    if !prefixes.iter().all(|v| values.contains(*v)) {
        if let Some(plan) = plan.filter(|_| !r.raw.is_empty()) {
            for l in crate::data::literals(&r.raw, &r.program, plan, true, 64, false)? {
                if prefixes.contains(l.value.as_str()) && m.start <= l.start && l.start < m.end {
                    values.insert(l.value);
                }
            }
        }
    }
    if !prefixes.iter().all(|v| values.contains(*v)) {
        return Ok(None);
    }
    let mut sorted = literals;
    sorted.sort_by_key(|l| l.start);
    Ok(Some(console(&sorted)?))
}
