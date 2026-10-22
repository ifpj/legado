//! Explicit software input descriptor derived from the current widget CSS.
use crate::{css, html::Facts, math, re, require, Result};
use serde_json::{json, Value};
pub fn root() -> Value {
    json!({"CAHKN0":"[]","KOmHB7":"Invalid code point -1".len()-1,"HmLug8":1|2|4})
}
fn style_data(
    html: &str,
) -> Result<(
    Vec<(String, indexmap::IndexMap<String, String>)>,
    Vec<Value>,
)> {
    let facts = Facts::read(html)?;
    require(
        !facts.style_texts.is_empty() && facts.style_texts.len() <= 16,
        "Widget styles bounds",
    )?;
    let mut rules = Vec::new();
    let pattern = re(r"(?s)([^{}]+)\{([^{}]*)\}");
    let mut sheets = vec![json!([]), json!([])];
    for source in &facts.style_texts {
        for cap in pattern.captures_iter(source) {
            if !cap[1].contains('@') {
                rules.push((cap[1].trim().to_string(), css::declarations(&cap[2])?));
            }
        }
        if re(r"^\s*\.[A-Za-z][A-Za-z0-9]*\s*\{\s*background-position:\s*-1px -1px;\s*background-repeat:\s*no-repeat;?\s*}\s*$").is_match(source){continue}
        let keyframes: Vec<_> = css::stylesheet(source)?
            .into_iter()
            .filter(|r| r.starts_with("@keyframes "))
            .collect();
        sheets.push(json!(keyframes));
    }
    sheets.push(json!(0));
    Ok((rules, sheets))
}
pub fn proof_styles(html: &str) -> Result<Value> {
    Ok(json!(style_data(html)?.1))
}
pub fn proof(html: &str) -> Result<Value> {
    let (rules, sheets) = style_data(html)?;
    let inputs: Vec<_> = rules
        .iter()
        .filter(|(selector, s)| {
            re(r"^\.[A-Za-z][A-Za-z0-9]* input$").is_match(selector)
                && s.get("position").map(String::as_str) == Some("absolute")
                && s.get("opacity").map(String::as_str) == Some("0")
                && s.get("width").map(String::as_str) == Some("100%")
                && s.get("height").map(String::as_str) == Some("100%")
        })
        .collect();
    require(inputs.len() == 1, "Widget input declaration")?;
    let prefix = inputs[0].0.strip_suffix(" input").unwrap();
    let sizes: Vec<_> = rules
        .iter()
        .filter(|(selector, s)| {
            selector.starts_with(&format!("{prefix} ."))
                && s.get("width") == s.get("height")
                && s.contains_key("border")
                && s.contains_key("grid-area")
        })
        .map(|(_, s)| css::scalar(s, "width", 0.))
        .collect::<Result<_>>()?;
    let containers: Vec<_> = rules
        .iter()
        .filter(|(_, s)| {
            s.get("height").map(String::as_str) == Some("65px")
                && s.get("display").map(String::as_str) == Some("flex")
                && s.get("padding").map(String::as_str) == Some("0 8px")
                && s.contains_key("border")
        })
        .collect();
    require(
        sizes.len() == 1 && containers.len() == 1 && (16.0..=48.0).contains(&sizes[0]),
        "Widget geometry",
    )?;
    let s = &containers[0].1;
    let size = sizes[0];
    let border = css::scalar(s, "border", 0.)?;
    let height = css::scalar(s, "height", 0.)?;
    let padding = css::length(s["padding"].split_whitespace().nth(1).unwrap())?;
    let left = border + padding;
    let top = border + (height - size) / 2.;
    let width = size + padding + "Verify you are human".len() as f64 * 7.;
    let geometry = WidgetGeometry {
        left,
        top,
        width,
        height: size,
        checkbox: size,
        viewport_width: 300,
        viewport_height: 65,
        border,
        opacity: inputs[0].1["opacity"].clone(),
    };
    Ok(
        json!({"suPgN7":"fAhw2","YBguh4":generated_input(&geometry,0.,None,None)?,"EhaWB7":"Error","bXia4":false,"ckwkN6":sheets}),
    )
}
#[derive(serde::Deserialize)]
pub struct WidgetGeometry {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
    pub checkbox: f64,
    pub viewport_width: usize,
    pub viewport_height: usize,
    pub border: f64,
    pub opacity: String,
}
pub fn generated_input(
    g: &WidgetGeometry,
    elapsed_ms: f64,
    client_x: Option<f64>,
    client_y: Option<f64>,
) -> Result<Value> {
    let WidgetGeometry {
        left,
        top,
        width,
        height,
        checkbox,
        border,
        ..
    } = *g;
    let size = height;
    let x = client_x.unwrap_or((left + checkbox / 2.).floor());
    let y = client_y.unwrap_or((top + checkbox / 2.).floor());
    require(
        [left, top, width, height, checkbox, border, x, y, elapsed_ms]
            .iter()
            .all(|n| n.is_finite())
            && width > 0.
            && height > 0.
            && elapsed_ms >= 0.
            && left <= x
            && x <= left + width
            && top <= y
            && y <= top + height,
        "Generated point outside finite widget bounds",
    )?;
    let mut fields = json!({});
    for name in ["hJWPi4", "rFzv4", "Gylx2", "mXDtL9", "TiAqq0", "HHQd3"] {
        fields[name] = json!("false");
    }
    for name in [
        "UVbS3", "dDlG2", "XxPTZ4", "Kqnqo9", "FenO2", "LHUDG9", "lkuER9", "UbPsJ2", "CMrXf6",
        "JtOo8",
    ] {
        fields[name] = json!("0");
    }
    for name in ["zRYn6", "BuVz3", "AQSxQ9", "yWXAc7"] {
        fields[name] = json!("true");
    }
    for name in ["KiEkL2", "qgvYg9", "ubbtm3", "iyWVk8"] {
        fields[name] = json!(math::number(x));
    }
    for name in ["xiwwc6", "EttxX5", "MnsyE9", "BGhxB2"] {
        fields[name] = json!(math::number(y));
    }
    for name in ["yspV2", "vCSZ5", "yqYok3", "cUvIx3"] {
        fields[name] = json!("1");
    }
    let target = "[object HTMLInputElement]";
    let rest = json!({"NBFSx9":std::f64::consts::FRAC_PI_2,"LIJj0":2,"aIbz6":(x-left).floor()+border,"gDkO2":(y-top).floor()+border,"hvEHc8":"mouse","rELE0":null,"OJXP6":(1920.-g.viewport_width as f64)/2.+x,"BWJTa0":(1053.-g.viewport_height as f64)/2.+14.+y,"aPekv1":target,"LPFt3":target,"HuPfz3":elapsed_ms,"dVmv6":"click","NRsWT2":(x-left).floor(),"oWsw2":(y-top).floor(),"gMHaY6":false,"ThwQR1":target,"OOmW9":left,"yDADU0":top,"Qlbp7":left+width,"AohfY7":top+size,"ZiuW6":width,"lhZZy5":size,"Kharv2":0,"lfGus2":0,"kHejN8":g.viewport_width,"fJPd9":g.viewport_height,"GrIIp1":1,"sgGSr1":target,"uIZQ4":true,"OndX5":width,"gLhqb0":size,"wokz3":width,"nJCd8":size,"GSia9":"block","llYxG1":"visible","QzPbO2":"auto","pYER4":g.opacity});
    for (k, v) in rest.as_object().unwrap() {
        fields[k] = json!(match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => math::number(n.as_f64().unwrap()),
            _ => v.to_string(),
        });
    }
    require(
        fields.as_object().unwrap().len() == 69,
        "Generated descriptor field count",
    )?;
    Ok(fields)
}
