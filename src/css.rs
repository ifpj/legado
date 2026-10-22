//! Finite CSS serialization and scalar declarations; no layout engine.
use crate::{math::number as math_number, re, require, Error, Result};
use indexmap::IndexMap;
const SUPPORTED: &[&str] = &[
    "align-items",
    "align-self",
    "animation",
    "animation-duration",
    "background",
    "background-color",
    "background-position",
    "background-repeat",
    "border",
    "border-color",
    "border-style",
    "border-image",
    "border-radius",
    "border-spacing",
    "border-width",
    "bottom",
    "box-shadow",
    "box-sizing",
    "color",
    "content",
    "cursor",
    "direction",
    "display",
    "fill",
    "flex-direction",
    "flex-flow",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "gap",
    "grid-area",
    "grid-column",
    "grid-template-columns",
    "height",
    "inset",
    "justify-content",
    "left",
    "line-height",
    "list-style-type",
    "margin",
    "margin-bottom",
    "margin-left",
    "margin-right",
    "margin-top",
    "max-width",
    "opacity",
    "outline",
    "outline-offset",
    "overflow",
    "padding",
    "padding-left",
    "padding-right",
    "padding-top",
    "place-content",
    "place-items",
    "position",
    "right",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-width",
    "text-align",
    "text-decoration",
    "top",
    "transform",
    "transform-box",
    "transform-origin",
    "transition",
    "user-select",
    "visibility",
    "width",
    "z-index",
    "-webkit-font-smoothing",
];

fn segments(source: &str, separator: char) -> Vec<String> {
    let (mut out, mut depth, mut quote, mut escaped, mut start) =
        (Vec::new(), 0i32, None, false, 0);
    for (i, c) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            continue;
        }
        if matches!(c, '(' | '[') {
            depth += 1
        } else if matches!(c, ')' | ']') {
            depth -= 1
        } else if c == separator && depth == 0 {
            out.push(source[start..i].trim().into());
            start = i + c.len_utf8()
        }
    }
    if start < source.len() {
        out.push(source[start..].trim().into())
    }
    out
}
fn number(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let scientific = format!("{v:.5e}");
    let rounded = scientific.parse::<f64>().unwrap_or(v);
    if v.fract() == 0.0 && v.abs() < 1e7 {
        return format!("{v:.0}");
    }
    if v.fract() == 0.0 {
        return format!("{rounded:.0}");
    }
    if (1e-4..1e6).contains(&rounded.abs()) {
        return math_number(rounded);
    }
    let (mantissa, exponent) = scientific.split_once('e').unwrap();
    let m = mantissa.trim_end_matches('0').trim_end_matches('.');
    let e = exponent.parse::<i32>().unwrap();
    format!("{m}e{e:+03}")
}
pub fn split(value: &str) -> Vec<String> {
    segments(value, ' ')
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect()
}
pub fn declarations(source: &str) -> Result<IndexMap<String, String>> {
    require(source.len() <= 256 * 1024, "CSS declarations bounds")?;
    let mut out = IndexMap::new();
    for item in segments(source, ';') {
        if item.trim().is_empty() {
            continue;
        }
        let (k, v) = item
            .split_once(':')
            .ok_or_else(|| Error::new("INVALID_DATA", "CSS declaration expected"))?;
        require(
            re(r"^[a-zA-Z_-][a-zA-Z0-9_-]*$").is_match(k.trim()),
            "CSS property name",
        )?;
        out.shift_remove(k.trim());
        out.insert(k.trim().to_lowercase(), v.trim().into());
    }
    Ok(out)
}
pub fn length(value: &str) -> Result<f64> {
    let pat = re(r"^(-?(?:\d+(?:\.\d*)?|\.\d+))(px|pt|rem)?(?:\s|$)");
    let m = pat
        .captures(value)
        .ok_or_else(|| Error::new("INVALID_DATA", "Finite CSS length"))?;
    let n = m[1]
        .parse::<f64>()
        .map_err(|_| Error::new("INVALID_DATA", "CSS scalar"))?;
    Ok(n * match m.get(2).map(|v| v.as_str()) {
        Some("pt") => 4.0 / 3.0,
        Some("rem") => 16.0,
        _ => 1.0,
    })
}
pub fn scalar(style: &IndexMap<String, String>, name: &str, default: f64) -> Result<f64> {
    style.get(name).map(|v| length(v)).unwrap_or(Ok(default))
}
fn value(source: &str, colors: bool) -> Result<String> {
    let pat = re(
        r#""(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|#[a-zA-Z0-9]+|-?(?:\d+\.\d*|\.\d+|\d+)(?:[eE][+-]?\d+)?(?:[a-zA-Z]+|%)?|[a-zA-Z_-][a-zA-Z0-9_-]*|\s+|."#,
    );
    let mut out = String::new();
    for m in pat.find_iter(source) {
        let s = m.as_str();
        if s.chars().all(char::is_whitespace) {
            out.push(' ')
        } else if s.starts_with('\'') {
            let quoted = s.trim_matches('\'').replace('"', "\\\"");
            out.push('"');
            out.push_str(&quoted);
            out.push('"')
        } else if s.starts_with('"') {
            out.push_str(s)
        } else if colors
            && (s.starts_with('#')
                || matches!(s, "red" | "green" | "white" | "black" | "transparent"))
        {
            let rgb = if s == "transparent" {
                Some((0, 0, 0, 0))
            } else if let Some(h) = s.strip_prefix('#') {
                let h = if h.len() == 3 {
                    h.chars().flat_map(|c| [c, c]).collect::<String>()
                } else {
                    h.into()
                };
                if h.len() == 6 {
                    u32::from_str_radix(&h, 16)
                        .ok()
                        .map(|v| ((v >> 16) & 255, (v >> 8) & 255, v & 255, 1))
                } else {
                    None
                }
            } else {
                match s {
                    "red" => Some((255, 0, 0, 1)),
                    "green" => Some((0, 128, 0, 1)),
                    "white" => Some((255, 255, 255, 1)),
                    "black" => Some((0, 0, 0, 1)),
                    _ => None,
                }
            };
            if let Some((r, g, b, a)) = rgb {
                out.push_str(&if a == 1 {
                    format!("rgb({r}, {g}, {b})")
                } else {
                    format!("rgba({r}, {g}, {b}, 0)")
                })
            } else {
                out.push_str(s)
            }
        } else if let Some(n) =
            re(r"^(-?(?:\d+\.\d*|\.\d+|\d+)(?:[eE][+-]?\d+)?)([a-zA-Z%]*)$").captures(s)
        {
            let scalar: f64 = n[1]
                .parse()
                .map_err(|_| Error::new("INVALID_DATA", "CSS number"))?;
            require(scalar.is_finite(), "Finite CSS number required")?;
            out.push_str(&number(scalar));
            out.push_str(&n[2].to_lowercase())
        } else {
            out.push_str(match s {
                "," => ", ",
                "/" => " / ",
                ":" => ": ",
                _ => s,
            })
        }
    }
    let mut out = re(r"\s+").replace_all(&out, " ").trim().to_string();
    for name in [
        "rotate", "rotateX", "rotateY", "rotateZ", "skew", "skewX", "skewY",
    ] {
        out = out.replace(&format!("{name}(0)"), &format!("{name}(0deg)"));
    }
    for name in ["translateX", "translateY", "translateZ"] {
        out = out.replace(&format!("{name}(0)"), &format!("{name}(0px)"));
    }
    Ok(out)
}
fn lengths(source: &str) -> String {
    split(source)
        .into_iter()
        .map(|s| if s == "0" { "0px".into() } else { s })
        .collect::<Vec<_>>()
        .join(" ")
}
fn normalize(name: &str, source: &str) -> String {
    let length_props = [
        "margin",
        "padding",
        "inset",
        "border-radius",
        "border-width",
        "gap",
        "top",
        "left",
        "right",
        "bottom",
        "margin-top",
        "margin-bottom",
        "margin-left",
        "margin-right",
        "padding-top",
        "padding-bottom",
        "padding-left",
        "padding-right",
        "outline-offset",
        "width",
        "height",
        "max-width",
        "border-spacing",
    ];
    if length_props.contains(&name) {
        return lengths(source);
    }
    if name == "flex-flow" {
        return source.replace(" nowrap", "");
    }
    if name == "transform-origin" && source == "center" {
        return "center center".into();
    }
    let args = split(source);
    if name == "box-shadow" {
        let colors: Vec<_> = args
            .iter()
            .filter(|s| s.starts_with("rgb(") || s.starts_with("rgba("))
            .cloned()
            .collect();
        let rest = lengths(
            &args
                .iter()
                .filter(|s| !colors.contains(s) && s.as_str() != "inset")
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
        );
        return colors
            .into_iter()
            .chain([rest])
            .chain(if args.iter().any(|s| s == "inset") {
                Some("inset".into())
            } else {
                None
            })
            .collect::<Vec<_>>()
            .join(" ");
    }
    if name == "animation" || name == "transition" {
        let times: Vec<_> = args
            .iter()
            .filter(|s| re(r"^-?[0-9.]+m?s$").is_match(s))
            .cloned()
            .collect();
        let timing = args
            .iter()
            .find(|s| {
                s.starts_with("cubic-bezier(")
                    || s.starts_with("steps(")
                    || [
                        "linear",
                        "ease",
                        "ease-in",
                        "ease-out",
                        "ease-in-out",
                        "step-start",
                        "step-end",
                    ]
                    .contains(&s.as_str())
            })
            .cloned()
            .unwrap_or_else(|| "ease".into());
        if name == "transition" {
            return times
                .iter()
                .take(1)
                .cloned()
                .chain([timing.clone()])
                .chain(times.iter().skip(1).cloned())
                .chain(
                    args.into_iter()
                        .filter(|s| !times.contains(s) && s != &timing && s != "all"),
                )
                .collect::<Vec<_>>()
                .join(" ");
        }
        let iterations = args
            .iter()
            .find(|s| s.as_str() == "infinite" || re(r"^[0-9.]+$").is_match(s))
            .cloned()
            .unwrap_or_else(|| "1".into());
        let pick = |choices: &[&str], default: &str| {
            args.iter()
                .find(|s| choices.contains(&s.as_str()))
                .cloned()
                .unwrap_or_else(|| default.into())
        };
        let direction = pick(
            &["normal", "reverse", "alternate", "alternate-reverse"],
            "normal",
        );
        let fill = pick(&["forwards", "backwards", "both"], "none");
        let play = pick(&["running", "paused"], "running");
        let name = args
            .iter()
            .filter(|s| {
                !times.contains(s)
                    && **s != timing
                    && **s != iterations
                    && **s != direction
                    && **s != fill
                    && **s != play
                    && s.as_str() != "none"
            })
            .last()
            .cloned()
            .unwrap_or_else(|| "none".into());
        return [
            times.first().map(String::as_str).unwrap_or("auto"),
            &timing,
            times.get(1).map(String::as_str).unwrap_or("0s"),
            &iterations,
            &direction,
            &fill,
            &play,
            &name,
        ]
        .join(" ");
    }
    if name == "outline" {
        let colors: Vec<_> = args
            .iter()
            .filter(|v| v.starts_with("rgb(") || v.starts_with("rgba("))
            .cloned()
            .collect();
        let styles = ["none", "solid", "dotted", "dashed", "double", "auto"];
        return colors
            .into_iter()
            .chain(
                args.iter()
                    .filter(|s| styles.contains(&s.as_str()))
                    .cloned(),
            )
            .chain(
                args.iter()
                    .filter(|s| {
                        !s.starts_with("rgb(")
                            && !s.starts_with("rgba(")
                            && !styles.contains(&s.as_str())
                    })
                    .map(|s| lengths(s)),
            )
            .collect::<Vec<_>>()
            .join(" ");
    }
    source.into()
}
pub fn stylesheet(source: &str) -> Result<Vec<String>> {
    let output = rules(source, 0, false)?;
    require(output.len() <= 2048, "CSS rule count")?;
    Ok(output)
}
fn rules(source: &str, depth: usize, keyframe: bool) -> Result<Vec<String>> {
    require(depth <= 8 && source.len() <= 256 * 1024, "CSS rule bounds")?;
    let source = re(r"(?s)/\*.*?\*/").replace_all(source, "");
    let (mut position, mut output) = (0usize, Vec::new());
    while position < source.len() {
        let Some(open) = source[position..].find('{').map(|p| p + position) else {
            require(source[position..].trim().is_empty(), "Unsupported CSS tail")?;
            break;
        };
        let selector = source[position..open].trim();
        let (mut level, mut quote, mut escaped, mut end) = (1, None, false, None);
        for (i, c) in source[open + 1..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if c == '\\' {
                escaped = true;
                continue;
            }
            if let Some(q) = quote {
                if c == q {
                    quote = None
                }
                continue;
            }
            if c == '\'' || c == '"' {
                quote = Some(c)
            } else if c == '{' {
                level += 1
            } else if c == '}' {
                level -= 1;
                if level == 0 {
                    end = Some(open + 1 + i);
                    break;
                }
            }
        }
        let end = end.ok_or_else(|| Error::new("INVALID_DATA", "Unterminated CSS rule"))?;
        let body = &source[open + 1..end];
        position = end + 1;
        if selector.starts_with('@') {
            let keyframes = selector.starts_with("@keyframes ");
            require(
                keyframes || selector.starts_with("@media "),
                "Unsupported CSS at-rule",
            )?;
            let children = rules(body, depth + 1, keyframes)?;
            let rendered = children
                .iter()
                .map(|s| format!("  {}", s.replace('\n', "\n  ")))
                .collect::<Vec<_>>()
                .join("\n");
            output.push(format!(
                "{}{}{}\n}}",
                value(selector, false)?,
                if keyframes { " { \n" } else { " {\n" },
                rendered
            ));
        } else {
            let mut selector = value(selector, false)?;
            selector = re(r"\s*([>~+])\s*")
                .replace_all(&selector, " $1 ")
                .into_owned();
            selector = re(r":\s+").replace_all(&selector, ":").into_owned();
            if keyframe {
                selector = selector
                    .split(',')
                    .map(|s| match s.trim() {
                        "from" => "0%",
                        "to" => "100%",
                        s => s,
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            }
            let declared = declarations(body)?;
            let mut out = IndexMap::new();
            for (k, v) in declared {
                require(SUPPORTED.contains(&k.as_str()), "Unsupported CSS property")?;
                let v = value(&v, true)?;
                let v = normalize(&k, &v);
                out.shift_remove(&k);
                out.insert(k.clone(), v);
                if k == "border-width" && out.contains_key("border") {
                    let border = out.shift_remove("border").unwrap();
                    let args = split(&border);
                    let style = args
                        .iter()
                        .find(|s| {
                            ["solid", "none", "dotted", "dashed", "double"].contains(&s.as_str())
                        })
                        .cloned()
                        .unwrap_or_else(|| "none".into());
                    let color = args
                        .iter()
                        .find(|s| s.starts_with("rgb(") || s.starts_with("rgba("))
                        .cloned()
                        .unwrap_or_else(|| "currentcolor".into());
                    let last = out.shift_remove(&k).unwrap();
                    out.insert("border-style".into(), style);
                    out.insert("border-color".into(), color);
                    out.insert("border-image".into(), "none".into());
                    out.insert(k, last);
                }
            }
            output.push(format!(
                "{selector} {{ {} }}",
                out.into_iter()
                    .map(|(k, v)| format!("{k}: {v};"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        require(output.len() <= 2048, "CSS rule count")?;
    }
    Ok(output)
}
