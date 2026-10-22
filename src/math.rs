use crate::{
    catalog::Module,
    data::{Plan, Response},
    require, Error, Result,
};
use serde_json::{json, Value};
use std::sync::OnceLock;
pub fn number(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v < 0.0 { "-Infinity" } else { "Infinity" }.into();
    }
    if v == 0.0 {
        return "0".into();
    }
    if v.abs() < 1e-6 || v.abs() >= 1e21 {
        let text = format!("{v:e}");
        let (a, b) = text.split_once('e').unwrap();
        let b = b.parse::<i32>().unwrap();
        return format!("{a}e{}{b}", if b >= 0 { "+" } else { "" });
    }
    let text = serde_json::to_string(&v).expect("finite float");
    if let Some((mantissa, power)) = text.split_once('e') {
        let power = power.parse::<i32>().unwrap();
        let negative = mantissa.starts_with('-');
        let digits = mantissa.trim_start_matches('-').replace('.', "");
        let point = mantissa
            .trim_start_matches('-')
            .find('.')
            .unwrap_or(digits.len()) as i32
            + power;
        let out = if point <= 0 {
            format!("0.{}{digits}", "0".repeat((-point) as usize))
        } else if point as usize >= digits.len() {
            format!("{digits}{}", "0".repeat(point as usize - digits.len()))
        } else {
            format!(
                "{}.{}",
                &digits[..point as usize],
                &digits[point as usize..]
            )
        };
        return if negative { format!("-{out}") } else { out };
    }
    text.strip_suffix(".0").unwrap_or(&text).to_string()
}
pub fn seeds(response: &Response, module: &Module, plan: &Plan) -> Result<(usize, [f64; 2], u8)> {
    let f = &plan.format;
    let raw = &response.raw;
    let mut result = Vec::new();
    for l in response.between(module.start, module.end) {
        if l.value != "acosh"
            || l.start < module.start + 24
            || l.start + 3 > module.end.min(raw.len())
        {
            continue;
        }
        let first = l.start - 24;
        let second = l.start - 13;
        let old1 = f.byte(raw[first], f.state_step);
        let key1 = f.key(old1, f.state_step);
        let old2 = f.byte(raw[second], f.state_step);
        let key2 = f.key(old2, f.state_step);
        let tag = f.byte(raw[first + plan.kind_slot()?], key1);
        let tag2 = f.byte(raw[second + plan.kind_slot()?], key2);
        let a = f64::from_be_bytes(std::array::from_fn(|i| f.byte(raw[first + 3 + i], key1)));
        let b = f64::from_be_bytes(std::array::from_fn(|i| f.byte(raw[second + 3 + i], key2)));
        let obj_op = f.byte(raw[l.start - 2], key2);
        let acosh_old = f.byte(raw[l.start], f.state_step);
        let acosh_tag = f.byte(
            raw[l.start + plan.kind_slot()?],
            f.key(acosh_old, f.state_step),
        );
        if old2 == key1
            && acosh_old == f.key(key2, obj_op)
            && tag == tag2
            && tag != plan.literal_tag()?
            && acosh_tag == plan.literal_tag()?
            && a > 0.0
            && a < 1.0
            && b > 1e30
            && b < 1e100
        {
            result.push((first, [a, b], tag))
        }
    }
    require(result.len() == 1, "Math seed chain ambiguous")?;
    Ok(result.remove(0))
}
/// Recognize the complete current equation signature before claiming a task.
pub fn recipe(response: &Response, module: &Module, plan: &Plan) -> Result<Option<[f64; 2]>> {
    let (start, seeds, _) = seeds(response, module, plan)?;
    let spec: Value = serde_json::from_str(include_str!("../profiles/math-plan-v1.json"))?;
    let expected = spec["signature"]
        .as_object()
        .ok_or_else(|| Error::new("INVALID_DATA", "Math signature"))?;
    let literals = response.between(module.start, module.end);
    let numeric: std::collections::HashSet<_> = crate::array(&spec["cases"])?
        .iter()
        .filter_map(|v| v[0].as_str())
        .collect();
    let bounded = plan.meta["build"] == "d2d3316b729e";
    let math_at = if bounded {
        literals
            .iter()
            .filter(|l| l.value == "Math")
            .map(|l| l.start)
            .min()
            .unwrap_or(module.start)
    } else {
        module.start
    };
    let index_op = plan.meta["input_data_profile"]["index"][0].as_u64();
    let following = |l: &crate::data::Literal| -> Result<u8> {
        require(
            l.start < l.end && l.end < module.end.min(response.raw.len()),
            "Math following instruction bounds",
        )?;
        let f = &plan.format;
        Ok(f.byte(
            response.raw[l.end],
            f.key(f.byte(response.raw[l.start], f.state_step), f.state_step),
        ))
    };
    let anchors: Vec<_> = literals
        .iter()
        .filter(|l| l.start == start + 24 && l.value == "acosh" && l.end > 0)
        .collect();
    let closure_op = if bounded && index_op.is_some() && anchors.len() == 1 {
        Some(following(anchors[0])?)
    } else {
        None
    };
    let mut counts = std::collections::HashMap::<&str, u64>::new();
    for l in literals {
        if l.start < math_at || (bounded && l.start < start && numeric.contains(l.value.as_str())) {
            continue;
        }
        if let (Some(index), Some(closure)) = (index_op, closure_op) {
            if numeric.contains(l.value.as_str())
                && !matches!(following(l)?,v if v==index as u8||v==closure||v==plan.format.state_step)
            {
                continue;
            }
        }
        if expected.contains_key(&l.value) {
            *counts.entry(&l.value).or_default() += 1;
        }
    }
    Ok(
        if expected.len() == counts.len()
            && expected
                .iter()
                .all(|(k, v)| v.as_u64() == counts.get(k.as_str()).copied())
        {
            Some(seeds)
        } else {
            None
        },
    )
}
fn expr(v: &Value, seeds: [f64; 2], depth: usize) -> Result<f64> {
    require(depth <= 8, "Math depth")?;
    if let Some(n) = v.as_f64() {
        return Ok(n);
    }
    Ok(match v[0].as_str().unwrap_or("") {
        "seed" => {
            seeds[v[1]
                .as_u64()
                .filter(|i| *i < 2)
                .ok_or_else(|| Error::new("INVALID_DATA", "Seed index"))?
                as usize]
        }
        "const" => match v[1].as_str().unwrap_or("") {
            "LN2" => std::f64::consts::LN_2,
            "LOG2E" => std::f64::consts::LOG2_E,
            "LOG10E" => std::f64::consts::LOG10_E,
            "PI" => std::f64::consts::PI,
            "E" => std::f64::consts::E,
            "SQRT2" => std::f64::consts::SQRT_2,
            "SQRT1_2" => std::f64::consts::FRAC_1_SQRT_2,
            "LN10" => std::f64::consts::LN_10,
            _ => return Err(Error::new("INVALID_DATA", "Math constant")),
        },
        "neg" => -expr(&v[1], seeds, depth + 1)?,
        "mul" => expr(&v[1], seeds, depth + 1)? * expr(&v[2], seeds, depth + 1)?,
        _ => return Err(Error::new("INVALID_DATA", "Math operation")),
    })
}
pub(crate) fn hypot(args: &[f64], incremental: bool) -> f64 {
    if args.iter().any(|n| n.is_infinite()) {
        return f64::INFINITY;
    }
    if args.iter().any(|n| n.is_nan()) {
        return f64::NAN;
    }
    let (mut max, mut total, mut compensation) = (0f64, 0f64, 0f64);
    if incremental {
        for a in args.iter().map(|n| n.abs()) {
            if a > max {
                total *= (max / a).powi(2);
                max = a;
            }
            if max != 0.0 {
                total += (a / max).powi(2)
            }
        }
    } else {
        max = args.iter().map(|n| n.abs()).fold(0.0, f64::max);
        if max == 0.0 {
            return 0.0;
        }
        for a in args {
            let term = (a / max).powi(2) - compensation;
            let next = total + term;
            compensation = next - total - term;
            total = next;
        }
    }
    total.sqrt() * max
}
fn function(name: &str, args: &[f64]) -> Result<f64> {
    let x = args[0];
    Ok(match name {
        "abs" => x.abs(),
        "acos" => x.acos(),
        "acosh" => x.acosh(),
        "asin" => x.asin(),
        "asinh" => x.asinh(),
        "atan" => x.atan(),
        "atanh" => x.atanh(),
        "atan2" => x.atan2(args[1]),
        "cbrt" => x.cbrt(),
        "ceil" => x.ceil(),
        "clz32" => (x as u32).leading_zeros() as f64,
        "cos" => x.cos(),
        "cosh" => x.cosh(),
        "exp" => x.exp(),
        "expm1" => x.exp_m1(),
        "floor" => x.floor(),
        "fround" => (x as f32) as f64,
        "hypot" => hypot(args, false),
        "log" => x.ln(),
        "log1p" => x.ln_1p(),
        "log2" => x.log2(),
        "log10" => x.log10(),
        "pow" => x.powf(args[1]),
        "round" => (x + 0.5).floor(),
        "sign" => {
            if x == 0.0 {
                x
            } else {
                x.signum()
            }
        }
        "sin" => x.sin(),
        "sinh" => x.sinh(),
        "sqrt" => x.sqrt(),
        "tan" => x.tan(),
        "tanh" => x.tanh(),
        "trunc" => x.trunc(),
        _ => return Err(Error::new("INVALID_DATA", "Unknown native math function")),
    })
}
pub(crate) fn polyfill(name: &str, args: &[f64]) -> Result<f64> {
    let x = args[0];
    Ok(match name {
        "acosh" => (x + (x * x - 1.0).sqrt()).ln(),
        "asinh" => {
            let a = x.abs();
            if a < 2f64.powi(-28) {
                x
            } else {
                let r = if a > 2f64.powi(28) {
                    a.ln() + std::f64::consts::LN_2
                } else if a > 2.0 {
                    (2.0 * a + 1.0 / (a * a + 1.0).sqrt()).ln()
                } else {
                    (a + a * a / (1.0 + (a * a + 1.0).sqrt())).ln_1p()
                };
                if x > 0.0 {
                    r
                } else {
                    -r
                }
            }
        }
        "atanh" => ((1.0 + x) / (1.0 - x)).ln() / 2.0,
        "cbrt" => x.abs().powf(1.0 / 3.0).copysign(x),
        "cosh" => (x.exp() + (-x).exp()) / 2.0,
        "expm1" => x.exp() - 1.0,
        "hypot" => hypot(args, true),
        "log1p" => (1.0 + x).ln(),
        "log2" => x.ln() * std::f64::consts::LOG2_E,
        "log10" => x.ln() * std::f64::consts::LOG10_E,
        "sinh" => (x.exp() - (-x).exp()) / 2.0,
        "tanh" => (x.exp() - (-x).exp()) / (x.exp() + (-x).exp()),
        _ => return Err(Error::new("INVALID_DATA", "Unknown alternative math")),
    })
}
pub fn calculate(seeds: [f64; 2]) -> Result<Value> {
    require(
        seeds.iter().all(|v| v.is_finite()),
        "Finite math seeds required",
    )?;
    static PLAN: OnceLock<Value> = OnceLock::new();
    let p = PLAN.get_or_init(|| {
        serde_json::from_str(include_str!("../profiles/math-plan-v1.json")).unwrap()
    });
    let mut out = Vec::new();
    for case in crate::array(&p["cases"])? {
        let name = crate::string(&case[0])?;
        let args = crate::array(&case[1])?
            .iter()
            .map(|v| expr(v, seeds, 0))
            .collect::<Result<Vec<_>>>()?;
        out.push(number(function(name, &args)?));
        if crate::array(&p["polyfills"])?.iter().any(|v| v == name) {
            out.push(number(polyfill(name, &args)?))
        }
    }
    Ok(json!({"zJGo6":out}))
}
