use crate::{
    array, css, epoch_ms, math,
    models::{has, sha, Context, Results},
    operands, raster, re, require, Error, Result,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
fn f32(v: f64) -> f64 {
    v as f32 as f64
}
pub fn calculate(c: &Context<'_>, results: &mut Results) -> Result<()> {
    let mut numeric_tag = None;
    for m in &c.catalog.modules {
        let values: HashSet<_> = c
            .response
            .between(m.start, m.end)
            .into_iter()
            .map(|l| l.value.as_str())
            .collect();
        if values.contains("zJGo6") {
            numeric_tag = Some(math::seeds(c.response, m, c.plan)?.2);
        }
    }
    for m in &c.catalog.modules {
        let entered = epoch_ms();
        let literals = c.response.between(m.start, m.end);
        let values: HashSet<_> = literals.iter().map(|l| l.value.as_str()).collect();
        if has(&values, &["bhlS2", "lGWXo4", "diayS1", "px sans-serif"]) {
            let flags: Vec<_> = values.iter().filter(|s| s.starts_with("🇺🇸")).collect();
            require(flags.len() == 1, "Flag probe input")?;
            results.insert(
                &m.id,
                "declared-font-capabilities",
                flag_color(flags[0])?,
                entered,
            );
            continue;
        }
        if has(
            &values,
            &[
                "qLAU7",
                "createRadialGradient",
                "iso-8859-1",
                "JzCDC7",
                "EETJ6",
                "LDst3",
                "iPjX7",
                "bIojw0",
                "NmVj1",
                "WhFi7",
                "mKro0",
                "FuOh7",
                "RGkW0",
                "ewUxO8",
                "JkMX5",
            ],
        ) {
            let anchors: Vec<_> = literals
                .iter()
                .filter(|l| l.value.ends_with("px aanotafontaa"))
                .map(|l| l.start)
                .collect();
            require(!anchors.is_empty(), "Raster font anchor")?;
            let anchor = *anchors.iter().min().unwrap();
            let mut bounded = m.clone();
            bounded.start = m.start.max(anchor.saturating_sub(800));
            bounded.end = m.end.min(anchor + 3500);
            let arrays = operands::arrays(
                c.response,
                &bounded,
                c.plan,
                numeric_tag
                    .ok_or_else(|| Error::new("INVALID_DATA", "Raster floating input tag"))?,
            )?;
            let mut scenes = HashMap::new();
            for (_, v) in arrays {
                if valid_scene(&v) {
                    scenes.insert(serde_json::to_string(&v)?, v);
                }
            }
            require(scenes.len() == 1, "Raster scene ambiguity")?;
            let scene = scenes.into_values().next().unwrap();
            let emoji: Vec<_> = values
                .iter()
                .filter(|v| v.starts_with("😀a") && v.matches('a').count() == 9)
                .map(|s| s.split('a').map(String::from).collect::<Vec<_>>())
                .collect();
            let color_pattern = re(r"^color\(display-p3 ([0-9.]+) ([0-9.]+) ([0-9.]+)\)$");
            let colors: Vec<_> = values
                .iter()
                .filter_map(|v| color_pattern.captures(v))
                .collect();
            require(
                emoji.len() == 1 && colors.len() == 1,
                "Graphics scalar input ambiguity",
            )?;
            let color = [
                colors[0][1]
                    .parse()
                    .map_err(|_| Error::new("INVALID_DATA", "Color scalar"))?,
                colors[0][2]
                    .parse()
                    .map_err(|_| Error::new("INVALID_DATA", "Color scalar"))?,
                colors[0][3]
                    .parse()
                    .map_err(|_| Error::new("INVALID_DATA", "Color scalar"))?,
            ];
            results.insert(
                &m.id,
                "native-software-graphics",
                raster::graphics(&scene[0], &scene[1], &emoji[0], color)?,
                entered,
            );
            continue;
        }
        if has(&values, &["PMjKp8", "QklTK4", "pIVXg3", "DVHV7", "LSqkE3"]) {
            let words: Vec<_> = literals
                .iter()
                .filter(|l| re(r"^[a-f0-9]{64}(?:A[a-f0-9]{64}){1,31}$").is_match(&l.value))
                .collect();
            let scales: Vec<_> = values
                .iter()
                .filter_map(|v| {
                    re(r"^scale\(([0-9.]+)\)$")
                        .captures(v)
                        .map(|c| c[1].to_string())
                })
                .collect();
            let sizes: Vec<_> = values
                .iter()
                .filter_map(|v| {
                    re(r"^italic ([0-9.]+)px serif$")
                        .captures(v)
                        .map(|c| c[1].to_string())
                })
                .collect();
            require(
                words.len() == 1 && scales.len() == 1 && sizes.len() == 1 && values.contains("32"),
                "Text geometry scalars",
            )?;
            let mut bounded = m.clone();
            bounded.start = m.start.max(words[0].start.saturating_sub(800));
            bounded.end = words[0].start;
            let mut inputs = HashMap::new();
            for (_, v) in operands::arrays(
                c.response,
                &bounded,
                c.plan,
                numeric_tag.ok_or_else(|| Error::new("INVALID_DATA", "Geometry float tag"))?,
            )? {
                if v.as_array().is_some_and(|rows| {
                    rows.len() == 4
                        && rows.iter().all(|row| {
                            row.as_array().is_some_and(|row| {
                                !row.is_empty()
                                    && row.len() <= 16
                                    && row.iter().all(|v| {
                                        v.as_u64().is_some_and(|v| (256..=0x10ffff).contains(&v))
                                    })
                            })
                        })
                }) {
                    inputs.insert(serde_json::to_string(&v)?, v);
                }
            }
            require(inputs.len() == 1, "Unicode scalar declaration ambiguity")?;
            let codes = inputs.into_values().next().unwrap();
            let size = sizes[0]
                .parse()
                .map_err(|_| Error::new("INVALID_DATA", "Geometry font size"))?;
            let scale = scales[0]
                .parse()
                .map_err(|_| Error::new("INVALID_DATA", "Geometry scale"))?;
            results.insert(
                &m.id,
                "native-scalar-text-geometry",
                text_geometry(
                    &codes,
                    &words[0]
                        .value
                        .split('A')
                        .map(String::from)
                        .collect::<Vec<_>>(),
                    size,
                    scale,
                )?,
                entered,
            );
            continue;
        }
        let containers: Vec<_> = values
            .iter()
            .filter(|s| {
                s.starts_with("position:fixed;left:0;top:0;width:")
                    && s.contains("visibility:hidden;overflow:hidden")
            })
            .collect();
        if containers.len() == 1 && values.iter().any(|v| v.contains("<progress value=")) {
            let fragments: Vec<_> = literals
                .iter()
                .map(|l| l.value.as_str())
                .filter(|s| s.starts_with('{') && s.contains('}') && s.len() <= 1024)
                .collect();
            require(
                fragments.len() == 12 && fragments[0].starts_with("{position:absolute}"),
                "Box declaration order",
            )?;
            let shared = css::declarations(fragments[1][1..].split('}').next().unwrap())?;
            let mut styles = Vec::new();
            let mut caption = None;
            for fragment in &fragments[2..] {
                let (first, rest) = fragment[1..]
                    .split_once('}')
                    .ok_or_else(|| Error::new("INVALID_DATA", "Box fragment"))?;
                styles.push(css::declarations(first)?);
                if let Some(rest) = rest.strip_prefix("caption{") {
                    caption = Some(css::declarations(rest.split('}').next().unwrap())?);
                }
            }
            for i in [6, 7, 8, 9] {
                let mut out = indexmap::IndexMap::new();
                out.insert("position".into(), "absolute".into());
                if i >= 7 {
                    out.extend(shared.clone())
                }
                out.extend(styles[i].clone());
                styles[i] = out;
            }
            let width = css::scalar(&css::declarations(containers[0])?, "width", 0.)?;
            results.insert(
                &m.id,
                "native-finite-box-arithmetic",
                boxes(
                    &styles,
                    &caption.ok_or_else(|| Error::new("INVALID_DATA", "Table caption"))?,
                    width,
                )?,
                entered,
            );
            continue;
        }
        if values.contains("sJvX2") {
            let selectors: Vec<_> = values
                .iter()
                .filter(|v| re(r"^\.[A-Za-z][A-Za-z0-9]{3,15}$").is_match(v))
                .collect();
            require(selectors.len() == 1, "Stylesheet selector")?;
            let fields = stylesheets(c.html, selectors[0])?;
            results.insert(&m.id, "native-stylesheet-text", fields, entered);
            continue;
        }
    }
    Ok(())
}
pub(crate) fn valid_scene(v: &Value) -> bool {
    let Some(v) = v.as_array() else { return false };
    if v.len() != 2 {
        return false;
    }
    let (Some(shapes), Some(gradients)) = (v[0].as_array(), v[1].as_array()) else {
        return false;
    };
    if shapes.is_empty() || shapes.len() > 16 || gradients.is_empty() || gradients.len() > 16 {
        return false;
    }
    let numeric = |v: &Value| v.as_f64().is_some_and(|v| v.is_finite() && v.abs() <= 1e6);
    let color = |v: &Value| {
        v.as_str()
            .is_some_and(|v| re(r"^#[0-9a-fA-F]{6}$").is_match(v))
    };
    shapes.iter().all(|v| {
        v.as_array().is_some_and(|v| {
            !v.is_empty()
                && match v[0].as_u64() {
                    Some(1) => {
                        v.len() == 8
                            && numeric(&v[1])
                            && color(&v[2])
                            && v[3].as_str().is_some_and(|s| s.len() <= 128)
                            && matches!(v[4].as_u64(), Some(0 | 1))
                            && v[5].as_str().is_some_and(|s| s.len() <= 1024)
                            && v[6..].iter().all(numeric)
                    }
                    Some(2) => {
                        v.len() == 11
                            && numeric(&v[1])
                            && color(&v[2])
                            && v[3..].iter().all(numeric)
                    }
                    Some(3) => {
                        v.len() == 9 && numeric(&v[1]) && color(&v[2]) && v[3..].iter().all(numeric)
                    }
                    Some(4) => v.len() == 8 && v[1..].iter().all(numeric),
                    _ => false,
                }
        })
    }) && gradients.iter().all(|v| {
        v.as_array().is_some_and(|v| {
            v.len() == 9 && v[0] == 0 && v[1..7].iter().all(numeric) && v[7..].iter().all(color)
        })
    })
}
pub(crate) fn text_box(
    text: &str,
    size: f64,
    scale: f64,
    x: f64,
    y: f64,
) -> Result<([f64; 4], f64)> {
    require(
        size > 0. && size <= 256. && (0.1..=10.).contains(&scale),
        "Text box bounds",
    )?;
    let effective = if scale != 1. {
        (size * scale * 100. + 1e-7).floor() / 100.
    } else {
        size
    };
    let width = (text.encode_utf16().count() as f64 * effective * 0.5 * 64.).ceil() / 64.;
    let ascent = (effective * 0.9 + 0.5).floor();
    let descent = (effective * 0.2 + 0.5).floor();
    let s = f32(scale);
    let inverse = f32(1. / s);
    Ok((
        [
            f32(f32(x * s) * inverse),
            f32(f32(f32(y * s) - ascent) * inverse),
            f32(width * inverse),
            f32((ascent + descent) * inverse),
        ],
        f32(width / s),
    ))
}
pub fn text_geometry(codes: &Value, words: &[String], size: f64, scale: f64) -> Result<Value> {
    let mut values = Vec::new();
    for row in array(codes)? {
        let mut text = String::new();
        for code in array(row)? {
            text.push(
                char::from_u32(
                    code.as_u64()
                        .ok_or_else(|| Error::new("INVALID_DATA", "Unicode scalar"))?
                        as u32,
                )
                .ok_or_else(|| Error::new("INVALID_DATA", "Invalid Unicode scalar"))?,
            );
        }
        values.push(text_box(&text, 16., scale, 32., 32.)?)
    }
    for word in words {
        values.push(text_box(word, size, scale, 32., 32.)?)
    }
    let mut bounds = Vec::new();
    for (boxed, _) in &values {
        let [x, y, w, h] = *boxed;
        let points = [(x, y), (x + w, y), (x, y + h), (x + w, y + h)]
            .map(|(x, y)| (f32(scale * x), f32(scale * y)));
        let left = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
        let top = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
        let w = f32(points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max) - left);
        let h = f32(points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max) - top);
        bounds.push([left, top, f32(left + w), f32(top + h)]);
    }
    let x = bounds.iter().map(|b| b[0]).fold(f64::INFINITY, f64::min);
    let y = bounds.iter().map(|b| b[1]).fold(f64::INFINITY, f64::min);
    let group = [
        x,
        y,
        f32(bounds
            .iter()
            .map(|b| b[2])
            .fold(f64::NEG_INFINITY, f64::max)
            - x),
        f32(bounds
            .iter()
            .map(|b| b[3])
            .fold(f64::NEG_INFINITY, f64::max)
            - y),
    ];
    let first = &values[0];
    let extent = [first.0[0], first.0[1], first.1, first.0[3]];
    let mut unique = Vec::new();
    for (_, length) in &values {
        if !unique.contains(length) {
            unique.push(*length)
        }
    }
    let numbers = [
        group.iter().map(|v| v.abs()).sum(),
        extent.iter().map(|v| v.abs()).sum(),
        values[0].1,
        unique.iter().sum::<f64>() * 1e-5,
    ];
    let mut out = serde_json::Map::new();
    for (key, value) in ["PMjKp8", "QklTK4", "pIVXg3", "DVHV7"]
        .into_iter()
        .zip(numbers)
    {
        out.insert(key.into(), json!(sha(math::number(value))));
    }
    Ok(json!(out))
}
fn quantize(v: f64) -> f64 {
    (v * 64.).floor() / 64.
}
pub(crate) fn boxes(
    styles: &[indexmap::IndexMap<String, String>],
    caption: &indexmap::IndexMap<String, String>,
    width: f64,
) -> Result<Value> {
    require(
        styles.len() == 10 && (100.0..=2000.0).contains(&width),
        "Box recipe bounds",
    )?;
    let (mut out, mut flow) = (Vec::new(), 0.);
    for (i, s) in styles.iter().enumerate() {
        let border = css::scalar(s, "border", 0.)?;
        let padding = css::scalar(s, "padding", 0.)?;
        let mx = css::scalar(s, "margin-left", 0.)?;
        let my = css::scalar(s, "margin-top", 0.)?;
        let x = css::scalar(s, "left", 0.)? + mx;
        let y = css::scalar(s, "top", flow)? + my;
        let w = width - mx;
        let size = css::scalar(s, "font-size", 16.)?;
        let line = (size * 1.15 + 0.5).floor();
        let h = if [0, 1, 2, 3, 5].contains(&i) {
            quantize(line + 2. * (border + padding))
        } else if i == 4 {
            let cs = css::scalar(caption, "font-size", 16.)?;
            let ch = quantize((cs * 1.15 + 0.5).floor() + 2. * css::scalar(caption, "border", 0.)?);
            let cy = y + border;
            quantize(quantize(cy) + ch + 2. * line - cy + 2. * border)
        } else if i == 7 {
            let cy = y + border;
            let summary = quantize(cy) + line;
            let details = quantize(summary - cy);
            quantize(quantize(cy) + details - cy + 2. * border)
        } else {
            let cy = y + border;
            let atomic = match i {
                6 => 19.,
                8 => 0.,
                9 => 21.,
                _ => return Err(Error::new("INVALID_DATA", "Atomic box")),
            };
            let ascent = (size * 0.9 + 0.5).floor();
            let inline = ascent.max(atomic) + (line - ascent).max(0.);
            quantize(quantize(cy + inline) - cy + 2. * border)
        };
        let (x, y, w) = (quantize(x), quantize(y), quantize(w));
        out.push(json!({"ymsu2":y+h,"vCSZ5":h,"IBsj3":x,"RHQtR0":x+w,"WAZR9":y,"yqYok3":w,"iyWVk8":x,"BGhxB2":y}));
        if !matches!(
            s.get("position").map(String::as_str),
            Some("absolute" | "fixed")
        ) {
            flow = y + h;
        }
    }
    Ok(json!({"YGZZe2":out}))
}

pub(crate) fn stylesheets(html: &str, selector: &str) -> Result<Value> {
    require(
        re(r"^\.[A-Za-z][A-Za-z0-9]{3,15}$").is_match(selector),
        "Stylesheet selector",
    )?;
    let facts = crate::html::Facts::read(html)?;
    require(facts.style_texts.len() <= 64, "Stylesheet count bound")?;
    let (mut others, mut selected) = (Vec::new(), None);
    for (index, source) in facts.style_texts.iter().enumerate() {
        let value = css::stylesheet(source)?.join("|") + &index.to_string();
        if value.contains(selector) {
            selected = Some(value)
        } else {
            others.push(value)
        }
    }
    let combined = others.join("|");
    let mut fields =
        json!({"sJvX2":if combined.is_empty(){"-".into()}else{sha(combined)[..32].to_string()}});
    if let Some(value) = selected {
        fields["RutZ6"] = json!(value)
    }
    Ok(fields)
}

pub(crate) fn flag_color(text: &str) -> Result<Value> {
    let mut s = raster::Surface::new();
    s.resize(1, 1)?;
    s.step("set:font", &[json!("1px sans-serif")])?;
    s.step("fillText", &[json!(text), json!(0), json!(1)])?;
    let colored = s
        .pixels
        .chunks(4)
        .any(|p| p[3] != 0 && (p[0] != p[1] || p[1] != p[2]));
    Ok(json!({"bhlS2":if colored{"lGWXo4"}else{"diayS1"}}))
}
