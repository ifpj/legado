//! Finite scalar/fragment equations for the identified semantic recipe.
use crate::{
    epoch_ms, re, require,
    tasks::math,
    tasks::models::{has, Context, Results},
    Result,
};
use serde_json::{json, Value};
use std::{collections::HashSet, rc::Rc};
#[derive(Clone)]
pub(crate) enum Scalar {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    Array(Rc<Vec<Scalar>>),
    Object(Rc<()>),
}
use Scalar::*;
fn a(v: Vec<Scalar>) -> Scalar {
    Array(Rc::new(v))
}
fn object() -> Scalar {
    Object(Rc::new(()))
}
fn s(v: impl Into<String>) -> Scalar {
    Text(v.into())
}
fn kind(v: &Scalar) -> &str {
    match v {
        Undefined => "undefined",
        Null | Array(_) | Object(_) => "object",
        Bool(_) => "boolean",
        Number(_) => "number",
        Text(_) => "string",
    }
}
fn callback_literal_allowed(value: &str) -> bool {
    matches!(
        value,
        "RTCRtpReceiver" | "trustedTypes" | "bezierCurveTo" | "dispatchEvent" | "visibilityState"
    )
}
pub(crate) fn text(v: &Scalar) -> String {
    match v {
        Undefined => "undefined".into(),
        Null => "null".into(),
        Bool(b) => b.to_string(),
        Number(v) => math::number(*v),
        Text(s) => s.clone(),
        Array(a) => a
            .iter()
            .map(|v| {
                if matches!(v, Undefined | Null) {
                    String::new()
                } else {
                    text(v)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Object(_) => "[object Object]".into(),
    }
}
pub(crate) fn num(v: &Scalar) -> f64 {
    match v {
        Undefined => f64::NAN,
        Null => 0.,
        Bool(b) => u8::from(*b) as f64,
        Number(n) => *n,
        Array(_) | Object(_) => num(&Text(text(v))),
        Text(s) => {
            let s = s.trim();
            if s.is_empty() {
                0.
            } else if let Some(s) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(s, 16)
                    .map(|n| n as f64)
                    .unwrap_or(f64::NAN)
            } else if matches!(s, "Infinity" | "+Infinity" | "-Infinity")
                || crate::re(r"^[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?$")
                    .is_match(s)
            {
                s.parse().unwrap_or(f64::NAN)
            } else {
                f64::NAN
            }
        }
    }
}
pub(crate) fn truth(v: &Scalar) -> bool {
    match v {
        Undefined | Null => false,
        Bool(b) => *b,
        Number(n) => *n != 0. && !n.is_nan(),
        Text(s) => !s.is_empty(),
        _ => true,
    }
}
pub(crate) fn strict(a: &Scalar, b: &Scalar) -> bool {
    match (a, b) {
        (Undefined, Undefined) | (Null, Null) => true,
        (Number(a), Number(b)) => a == b,
        (Bool(a), Bool(b)) => a == b,
        (Text(a), Text(b)) => a == b,
        (Array(a), Array(b)) => Rc::ptr_eq(a, b),
        (Object(a), Object(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}
pub(crate) fn equal(a: &Scalar, b: &Scalar) -> bool {
    if kind(a) == kind(b) {
        return strict(a, b);
    }
    if matches!((a, b), (Undefined, Null) | (Null, Undefined)) {
        return true;
    }
    if matches!(a, Bool(_)) {
        return equal(&Number(num(a)), b);
    }
    if matches!(b, Bool(_)) {
        return equal(a, &Number(num(b)));
    }
    if matches!((a, b), (Number(_), Text(_)) | (Text(_), Number(_))) {
        return num(a) == num(b);
    }
    if matches!(a, Array(_) | Object(_)) && matches!(b, Text(_) | Number(_)) {
        return equal(&s(text(a)), b);
    }
    if matches!(b, Array(_) | Object(_)) && matches!(a, Text(_) | Number(_)) {
        return equal(a, &s(text(b)));
    }
    false
}
fn add(a: &Scalar, b: &Scalar) -> Scalar {
    let a = if matches!(a, Array(_) | Object(_)) {
        s(text(a))
    } else {
        a.clone()
    };
    let b = if matches!(b, Array(_) | Object(_)) {
        s(text(b))
    } else {
        b.clone()
    };
    if matches!(a, Text(_)) || matches!(b, Text(_)) {
        s(text(&a) + &text(&b))
    } else {
        Number(num(&a) + num(&b))
    }
}
pub(crate) fn divide(a: &Scalar, b: &Scalar) -> Scalar {
    Number(num(a) / num(b))
}
pub(crate) fn parse_integer(value: &Scalar, radix: u32) -> Scalar {
    let value = text(value);
    let mut input = value.trim_start();
    let sign = if let Some(s) = input.strip_prefix('-') {
        input = s;
        -1.
    } else {
        if let Some(s) = input.strip_prefix('+') {
            input = s
        }
        1.
    };
    let mut radix = radix;
    if radix == 0 {
        radix = if input.starts_with("0x") || input.starts_with("0X") {
            16
        } else {
            10
        };
    }
    if radix == 16 {
        input = input
            .strip_prefix("0x")
            .or_else(|| input.strip_prefix("0X"))
            .unwrap_or(input);
    }
    if !(2..=36).contains(&radix) {
        return Number(f64::NAN);
    }
    let (mut result, mut digits) = (0., 0);
    for c in input.chars() {
        let Some(n) = c.to_digit(radix) else { break };
        result = result * radix as f64 + n as f64;
        digits += 1;
    }
    Number(if digits == 0 { f64::NAN } else { result * sign })
}
// 3.14 is a literal from the declared recipe, not an approximation of pi.
#[allow(clippy::approx_constant)]
pub(crate) fn scalar_checks() -> Result<Vec<Value>> {
    let nan = Number(f64::NAN);
    let inf = Number(f64::INFINITY);
    let small = Number(5e-7);
    let large = Number(5e25);
    let small_text = s(text(&small));
    let large_text = s(text(&large));
    let mut checks = vec![
        equal(&s("1"), &Number(1.)),
        !strict(&Number(1.), &s("1")),
        strict(&Number(1.), &Number(1.)),
        !equal(&s("0.0000005"), &small_text),
        equal(&small_text, &small),
        !strict(&small_text, &s("0.0000005")),
        strict(&small_text, &s("5e-7")),
        equal(&small_text, &s("5e-7")),
        !equal(&large_text, &s("50000000000000000000000000")),
        equal(&large_text, &large),
        strict(&large_text, &s("5e+25")),
        equal(&s("5e+25"), &large_text),
        strict(&parse_integer(&large, 12), &Number(5.)),
        strict(&s("Infinity"), &s(text(&divide(&Number(1.), &Number(0.))))),
        equal(&Number(0.), &s(text(&divide(&Number(1.), &s("Infinity"))))),
        "NaN".repeat(15) + " Batman!" == text(&nan).repeat(15) + " Batman!",
        strict(&s(text(&divide(&Number(0.), &nan))), &s("NaN")),
        !strict(&nan, &nan),
        equal(&a(vec![]), &Bool(false)),
        equal(&Bool(!truth(&a(vec![]))), &a(vec![])),
        equal(&Bool(!truth(&a(vec![]))), &a(vec![])),
        equal(&Bool(!truth(&a(vec![]))), &Bool(false)),
        equal(&Bool(truth(&s("false"))), &Bool(truth(&s("true")))),
        strict(&Bool(truth(&s("true"))), &Bool(truth(&s("false")))),
        strict(
            &s("baNaNa"),
            &add(&add(&add(&s("b"), &s("a")), &Number(num(&s("a")))), &s("a")),
        ),
        strict(&s("fooNaN"), &add(&s("foo"), &Number(num(&s("bar"))))),
        "fail"
            != format!(
                "{}{}{}{}",
                text(&Bool(false)).chars().nth(0).unwrap(),
                text(&Bool(false)).chars().nth(1).unwrap(),
                text(&Undefined).chars().nth(5).unwrap(),
                text(&Undefined)
            ),
        matches!(object(), Object(_)),
        equal(&Number(0.), &a(vec![])),
        equal(&a(vec![]), &s("")),
        equal(&a(vec![s("")]), &s("")),
        equal(&s(""), &a(vec![Null])),
        equal(&Number(0.), &a(vec![Null])),
        equal(&s(""), &a(vec![Undefined])),
        equal(&a(vec![Undefined]), &Number(0.)),
        equal(&a(vec![a(vec![])]), &Number(0.)),
        equal(&a(vec![a(vec![])]), &s("")),
        equal(
            &s(""),
            &a(vec![a(vec![a(vec![a(vec![a(vec![a(vec![])])])])])]),
        ),
        !equal(
            &Number(0.),
            &a(vec![a(vec![a(vec![a(vec![a(vec![
                a(vec![]),
                Undefined,
                Undefined,
            ])])])])]),
        ),
        !strict(&Number(num(&Undefined)), &nan),
        strict(&parse_integer(&s("f*v"), 16), &Number(15.)),
        !strict(&parse_integer(&s("Infinity"), 10), &nan),
        !strict(&parse_integer(&s("Infinity"), 18), &nan),
        strict(
            &parse_integer(&s("Infinity"), 19),
            &parse_integer(&s("Infinity"), 23),
        ),
        !strict(&parse_integer(&inf, 10), &nan),
        !strict(
            &parse_integer(&s("Infinity"), 29),
            &parse_integer(&s("Infinity"), 30),
        ),
        !strict(
            &parse_integer(&s("Infinity"), 24),
            &parse_integer(&s("Infinity"), 23),
        ),
        strict(&parse_integer(&Null, 24), &Number(23.)),
        !strict(&parse_integer(&s("06"), 0), &Number(0.)),
        strict(&Number(1e16), &Number(1e16 + 1.)),
        !strict(&Number(1e16 + 1.1), &Number(1.0000000000000002e16 + 1.1)),
        !truth(&nan),
        truth(&inf),
        !truth(&Number(1. + -1.)),
        strict(&parse_integer(&small, 10), &Number(5.)),
        text(&add(
            &add(&add(&s("b"), &s("a")), &Number(num(&s("n")))),
            &s("a"),
        ))
        .to_lowercase()[2..5]
            == text(&Number(num(&s("NaN")))).to_lowercase(),
        strict(&s(""), &add(&a(vec![]), &a(vec![]))),
        strict(&add(&a(vec![]), &object()), &s("[object Object]")),
        !strict(&Number(0.1 + 0.2), &Number(0.3)),
        strict(&s(text(&Number(f64::NEG_INFINITY))), &s("-Infinity")),
        strict(
            &s(text(&Number(0. - f64::INFINITY))),
            &s(text(&Number(f64::NEG_INFINITY))),
        ),
        !strict(&Number(3.14 % 5.), &Number(13.14 % 5.)),
    ];
    let mut iterator = [1, 2, 3].into_iter();
    for expected in [1, 2, 3] {
        checks.push(equal(
            &s(iterator.next().unwrap().to_string()),
            &Number(expected as f64),
        ))
    }
    checks.push(strict(
        &divide(&Number(1.), &Number(-0.)),
        &Number(f64::NEG_INFINITY),
    ));
    require(checks.len() == 66, "Scalar recipe size")?;
    Ok(checks.into_iter().map(|v| json!(v)).collect())
}
fn operands(c: &Context<'_>, m: &crate::program::catalog::Module) -> Result<(u32, String)> {
    let literals = c.response.between(m.start, m.end);
    let anchors: Vec<_> = literals.iter().filter(|l| l.value == "luagobrr").collect();
    require(anchors.len() == 1, "Semantic input anchor")?;
    let anchor = anchors[0];
    let f = &c.plan.format;
    let raw = &c.response.raw;
    let mut key = anchor.instruction_key(raw, f);
    let mut position = anchor.end;
    let mut unary = None;
    for _ in 0..2 {
        require(position + 3 <= m.end, "Semantic conversion bounds")?;
        let opcode = f.opcode(raw[position], key, position);
        require(
            unary.is_none_or(|v| v == opcode),
            "Semantic integer conversion",
        )?;
        unary = Some(opcode);
        key = f.key(key, opcode);
        position += 3;
    }
    require(
        position + 3 <= m.end && f.opcode(raw[position], key, position) == f.state_step,
        "Semantic integer operand",
    )?;
    key = f.key(key, f.state_step);
    require(
        f.byte(raw[position + c.plan.kind_slot()?], key) as u64
            == c.plan.meta["integer_input_tag"]
                .as_u64()
                .unwrap_or(u64::MAX),
        "Semantic integer kind",
    )?;
    position += 3;
    let (mut integer, mut shift) = (0u32, 0);
    loop {
        require(position < m.end && shift <= 28, "Semantic integer bounds")?;
        let byte = f.byte(raw[position], key);
        position += 1;
        integer |= ((byte & 127) as u32) << shift;
        if byte < 128 {
            break;
        }
        shift += 7;
    }
    let expected = r"\u0067\u0075\u006c\u0065";
    let mut found = 0;
    let keys = f.instruction_keys(f.state_step);
    for at in position..(m.end.saturating_sub(expected.len() + 5)).min(position + 1200) {
        for &(_, candidate) in keys.at(f, raw[at], at) {
            if f.byte(raw[at + 3], candidate) as usize != expected.len() {
                continue;
            }
            let begin = at + 4;
            let mut seed = f.seed(begin);
            let first = f.byte(raw[begin], candidate);
            let high = (seed >> 16) as u8;
            let mask = if f.char_mode == "before-subtract" {
                first ^ high ^ expected.as_bytes()[0].wrapping_add(seed as u8)
            } else {
                first ^ (expected.as_bytes()[0] ^ high).wrapping_add(seed as u8)
            };
            let mut transform = f.clone();
            transform.char_xor = mask;
            let mut bytes = Vec::new();
            for b in &raw[begin..begin + expected.len()] {
                bytes.push(transform.char(f.byte(*b, candidate), seed));
                seed = seed.wrapping_add(2654435769)
            }
            if bytes == expected.as_bytes() {
                found += 1;
            }
        }
    }
    require(found == 1, "Fresh semantic regexp input not authenticated")?;
    Ok((integer, expected.into()))
}
pub(crate) fn recipe(integer: u32, regexp: &str, values: &HashSet<&str>) -> Result<Value> {
    let attrs: Vec<_> = values
        .iter()
        .filter(|v| v.starts_with("<div data-foo=\""))
        .collect();
    require(attrs.len() == 1, "Semantic fragment operand")?;
    let paragraphs = fragment("<p>mgkO0</div><p>kCzq0</p>")?;
    let attribute = fragment(attrs[0])?;
    let mut out = scalar_checks()?;
    let form_checks = vec![
        json!(strict(&Bool(true), &Bool(true))),
        json!(strict(&Bool(false), &Bool(false))),
        json!(kind(&Bool(true)) == "boolean"),
        json!(strict(&s(""), &s(""))),
        json!(strict(&Bool(false), &Bool(false))),
        json!(strict(&s(""), &s(""))),
        json!(strict(&s(""), &s(""))),
    ];
    let random_text =
        format!(
            "0.{}",
            (0..11)
                .map(|_| b"0123456789abcdefghijklmnopqrstuvwxyz"
                    [(rand::random::<u32>() % 36) as usize] as char)
                .collect::<String>()
        );
    let root = Rc::new(());
    let owner = root.clone();
    out.extend([
        json!(
            serde_json::to_string("function get plugins() { [native code] }")?
                .chars()
                .nth(2)
                .unwrap() as u32
        ),
        json!(kind(&Undefined)),
        json!(kind(&Undefined)),
        json!(kind(&object())),
        json!(kind(&object())),
        json!(kind(&Undefined)),
        json!(strict(&object(), &object())),
        json!(kind(&Undefined)),
        json!(!strict(&object(), &object())),
        json!(random_text.len()),
        json!(num(&a(vec![])) as i64),
        json!((0.5f64 + 0.5).floor() as i64),
        json!(strict(&s(""), &s(""))),
        json!(strict(&s(""), &Null)),
        json!(-1),
        json!(-1),
        json!(".hocD1 { display: flex; }" == format!(".{} {{ display: flex; }}", "hocD1")),
        json!(true),
        json!(Rc::ptr_eq(&owner, &root)),
        json!(paragraphs),
        json!(attribute),
    ]);
    out.extend(form_checks.clone());
    out.extend([json!(-1), json!(-1)]);
    out.extend(form_checks.clone());
    out.extend([json!(paragraphs), json!(attribute), json!(-1), json!(-1)]);
    out.extend(form_checks[..6].iter().cloned());
    out.extend([
        json!(num(&a(vec![])) as i64),
        json!("fmsM8"),
        json!(strict(&Bool(false), &Bool(false))),
        json!(false),
        json!(format!("[object {}]", "Undefined")),
        json!(format!("BLdW9{integer}")),
        json!(format!("BLdW9/{regexp}/")),
    ]);
    require(out.len() == 120, "Semantic recipe output size")?;
    Ok(json!({"eHTM1":"uvFa8","NjCPO1":out}))
}
pub fn calculate(c: &Context<'_>, results: &mut Results) -> Result<()> {
    for m in &c.catalog.modules {
        crate::cpu::check()?;
        let entered = epoch_ms();
        let values: HashSet<_> = c
            .response
            .between(m.start, m.end)
            .iter()
            .map(|l| l.value.as_str())
            .collect();
        if !has(
            &values,
            &[
                "eHTM1", "luagobrr", "plugins", " Batman!", "baNaNa", "BLdW9", "uvFa8", "mgkO0",
                "kCzq0", "fmsM8", "hocD1",
            ],
        ) {
            continue;
        }
        let (integer, regexp) = operands(c, m)?;
        let fields = recipe(integer, &regexp, &values)?;
        results.insert(
            &m.id,
            "native-finite-scalar-semantic-recipe",
            fields,
            entered,
        );
    }
    Ok(())
}
/// Shared root-operand block: fixed field names, each followed by its packed
/// value. Layout randomisation can place it before the first task identifier,
/// inside the last module's span. Exclude it only when its submit, nonce and
/// state values are exactly this response's frame parameters.
fn operand_block(c: &Context<'_>) -> Option<(usize, usize)> {
    const FIELDS: [&str; 13] = [
        "qrKd7", "vaZBm3", "nidg0", "gRYZ0", "yVlbA2", "TYrU7", "Hoqd1", "vvfHL3", "lffN9",
        "bkOl5", "gfOgY8", "Bxtcq6", "jLEVv0",
    ];
    let r = c.response;
    let value_after = |l: &crate::program::data::Literal| -> Option<&str> {
        let next = r
            .between(l.end, l.end + 33)
            .into_iter()
            .find(|x| x.packed.is_some())?;
        let i = r
            .packed
            .binary_search_by_key(&next.start, |(p, _)| *p)
            .ok()?;
        Some(r.packed[i].1.as_str())
    };
    let names = |name: &'static str| {
        r.literals
            .iter()
            .filter(move |l| l.packed.is_none() && l.value == name)
    };
    let anchor = |name: &'static str| -> Option<usize> {
        let want = c.catalog.parameters[name].as_str()?;
        let found: Vec<_> = names(name)
            .filter(|l| value_after(l) == Some(want))
            .map(|l| l.start)
            .collect();
        (found.len() == 1).then(|| found[0])
    };
    let (submit, nonce, state) = (anchor("vaZBm3")?, anchor("nidg0")?, anchor("yVlbA2")?);
    if !(submit < nonce && nonce < state && state - submit <= 4096) {
        return None;
    }
    let start = names("qrKd7")
        .filter(|l| l.start < submit && submit - l.start <= 8192 && value_after(l) == Some("[]"))
        .map(|l| l.start)
        .max()?;
    let end = names("jLEVv0")
        .filter(|l| state < l.start && l.start - state <= 1024)
        .map(|l| l.end)
        .min()?;
    let names: Vec<_> = r
        .between(start, end)
        .into_iter()
        .filter(|l| l.packed.is_none() && !l.value.is_empty())
        .map(|l| l.value.as_str())
        .collect();
    let distinct: HashSet<_> = names.iter().copied().collect();
    (names.len() == FIELDS.len() && FIELDS.iter().all(|f| distinct.contains(f)))
        .then_some((start, end))
}
pub fn callbacks(c: &Context<'_>, results: &mut Results) -> Result<()> {
    if c.plan.meta["build"] != "d2d3316b729e" {
        return Ok(());
    }
    let fields: HashSet<_> = results
        .fields
        .values()
        .flat_map(|v| v.as_object().into_iter().flat_map(|m| m.keys()))
        .map(String::as_str)
        .collect();
    if !fields.contains("KOmHB7") || !fields.contains("ZLSD0") {
        return Ok(());
    }
    let mut candidates = Vec::new();
    let last = c.catalog.modules.iter().map(|m| m.start).max().unwrap_or(0);
    let block = operand_block(c);
    for m in &c.catalog.modules {
        crate::cpu::check()?;
        if results.fields.contains_key(&m.id) {
            continue;
        }
        let literals = c.response.between(m.start, m.end);
        let mut end = m.end;
        let tails: Vec<_> = literals
            .iter()
            .filter(|l| {
                l.packed.as_deref() == Some("/cdn-cgi/challenge-platform")
                    && (700..=1200).contains(&(l.start - m.start))
            })
            .collect();
        if tails.len() == 1 {
            let preceding: Vec<_> = literals
                .iter()
                .filter(|l| l.start < tails[0].start)
                .map(|l| l.value.as_str())
                .collect();
            if preceding.ends_with(&["VawoD4", "dasFF6"]) {
                end = tails[0].start
            }
        }
        if m.start == last {
            let pool = literals
                .iter()
                .filter(|l| l.packed.is_some() && l.value.chars().count() >= 4098)
                .map(|l| l.start)
                .min();
            if let Some(pool) = pool.filter(|p| *p - m.start <= 1200) {
                end = pool;
                let registry = c
                    .catalog
                    .targets
                    .iter()
                    .filter(|t| end < t.target && t.target < m.end)
                    .map(|t| t.target)
                    .min()
                    .unwrap_or(m.end);
                let trailing: Vec<_> = literals
                    .iter()
                    .filter(|l| {
                        end <= l.start
                            && l.start < registry
                            && l.packed.is_none()
                            && !matches!(l.value.as_str(), "" | "hmwT6")
                            && !block.is_some_and(|(a, b)| a <= l.start && l.start < b)
                    })
                    .collect();
                let array_tail = trailing.len() == 1
                    && trailing[0].value == "Uint8Array"
                    && registry != m.end
                    && registry > trailing[0].start
                    && registry - trailing[0].start <= 64;
                if !trailing.is_empty() && !array_tail {
                    continue;
                }
            }
        }
        if !(700..=1200).contains(&(end - m.start)) {
            continue;
        }
        let values: HashSet<_> = literals
            .iter()
            .filter(|l| l.start < end)
            .map(|l| l.value.as_str())
            .collect();
        if !has(&values, &["eval", "this", "qotUd2", "wOEFN7", &m.label, ""]) {
            continue;
        }
        if values.iter().any(|v| {
            !callback_literal_allowed(v)
                && !matches!(*v, "" | "eval" | "this" | "removeEventListener")
                && !(v.len() == 1
                    && v.chars()
                        .all(|c| c.is_ascii_graphic() || matches!(c, ' ' | '\r' | '\n' | '\t')))
                && !(v.len() >= 2
                    && v.len() <= 3
                    && v.chars().all(|c| c.is_ascii_graphic() || c == ' ')
                    && v.chars().any(|c| c.is_ascii_alphabetic())
                    && v.chars()
                        .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '$'))
                && !re(r"^[A-Za-z_$][A-Za-z0-9_$]{0,10}$").is_match(v)
                && !re(r"^[A-Za-z0-9+/]{20,160}={0,2}\$[A-Za-z0-9+/]{20,24}={0,2}$").is_match(v)
        }) {
            continue;
        }
        candidates.push(m.id.clone());
    }
    if candidates.len() == 3 {
        for id in candidates {
            results.insert(&id, "native-protocol-empty-callback", json!({}), epoch_ms());
        }
    }
    Ok(())
}

#[cfg(test)]
mod callback_tests;

#[cfg(test)]
mod tests {
    use super::callback_literal_allowed;

    #[test]
    fn callback_filter_accepts_audited_browser_api_names_only() {
        for value in [
            "RTCRtpReceiver",
            "trustedTypes",
            "bezierCurveTo",
            "dispatchEvent",
            "visibilityState",
        ] {
            assert!(callback_literal_allowed(value));
        }
        assert!(!callback_literal_allowed("unknownLongConstant"));
    }
}

pub(crate) fn fragment(source: &str) -> Result<String> {
    require(source.len() <= 1024, "Finite HTML fragment bounds")?;
    let html = scraper::Html::parse_fragment(source);
    let mut depth = 0usize;
    for element in html.select(&scraper::Selector::parse("*").unwrap()) {
        require(
            matches!(element.value().name(), "html" | "p" | "div"),
            "Unsupported finite HTML fragment",
        )?;
        depth = depth.max(
            element
                .ancestors()
                .filter(|n| n.value().is_element())
                .count(),
        );
    }
    require(depth <= 6, "Finite HTML fragment depth")?;
    Ok(html.root_element().inner_html())
}
