//! Finite integer software surface, adapted under LICENSES/nokk.txt.
//! No browser, GPU, image fixture or recorded digest is used.
use crate::{array, re, require, string, tasks::math::number, tasks::models::sha, Error, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
fn n(v: &Value) -> Result<f64> {
    v.as_f64()
        .ok_or_else(|| Error::new("INVALID_DATA", "Pixel scalar"))
}
fn scalar(v: &Value) -> Result<String> {
    Ok(match v {
        Value::Null => String::new(),
        Value::Object(_) => "[o]".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(_) => number(n(v)?),
        Value::String(s) => s.clone(),
        _ => return Err(Error::new("INVALID_DATA", "Pixel argument type")),
    })
}
pub fn seed() -> u32 {
    let parts=["Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36","Linux x86_64","Google Inc. (Intel)","ANGLE (Intel, Mesa Intel(R) Xe Graphics (TGL GT2), OpenGL ES 3.2)","America/New_York","1920","1080","8","8"];
    let mut hash = 2166136261u32;
    for part in parts {
        for b in part.bytes() {
            hash = (hash ^ b as u32).wrapping_mul(16777619)
        }
        hash = (hash ^ 255).wrapping_mul(16777619)
    }
    hash & 0x7fffffff
}
pub fn windows_digest(bytes: &[u8]) -> String {
    let special = [
        0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039,
        0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
        0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
    ];
    let text: String = bytes
        .iter()
        .map(|b| {
            char::from_u32(if (128..160).contains(b) {
                special[(*b - 128) as usize]
            } else {
                *b as u32
            })
            .unwrap()
        })
        .collect();
    sha(text)
}
pub struct Surface {
    pub pixels: Vec<u8>,
    width: i64,
    height: i64,
    hash: u32,
    seed: u32,
    count: usize,
    matrix: [f64; 6],
    bounds: [f64; 4],
    style: HashMap<String, Value>,
}
impl Surface {
    pub fn new() -> Self {
        Self {
            pixels: vec![],
            width: -1,
            height: -1,
            hash: 2166136261,
            seed: seed(),
            count: 0,
            matrix: [1., 0., 0., 1., 0., 0.],
            bounds: [0.; 4],
            style: HashMap::new(),
        }
    }
    pub fn resize(&mut self, width: usize, height: usize) -> Result<()> {
        require(
            width > 0 && height > 0 && width <= 256 && height <= 256,
            "Surface bounds",
        )?;
        if self.width == width as i64 && self.height == height as i64 {
            return Ok(());
        }
        self.width = width as i64;
        self.height = height as i64;
        self.pixels = vec![0; width * height * 4];
        self.matrix = [1., 0., 0., 1., 0., 0.];
        self.bounds = [0.; 4];
        self.style = [
            ("fillStyle", json!("#000000")),
            ("strokeStyle", json!("#000000")),
            ("font", json!("10px sans-serif")),
            ("textAlign", json!("start")),
            ("textBaseline", json!("alphabetic")),
            ("lineWidth", json!(1)),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();
        Ok(())
    }
    fn note(&mut self, name: &str, args: &[Value]) -> Result<()> {
        self.count += 1;
        require(self.count <= 512, "Surface operation budget")?;
        let mut text = name.to_string();
        if !args.is_empty() {
            text.push('|');
            text.push_str(
                &args
                    .iter()
                    .map(scalar)
                    .collect::<Result<Vec<_>>>()?
                    .join(","),
            );
        }
        for c in text.encode_utf16() {
            self.hash = (self.hash ^ c as u32).wrapping_mul(16777619)
        }
        Ok(())
    }
    fn point(&mut self, x: f64, y: f64) {
        let [a, b, c, d, e, f] = self.matrix;
        let (x, y) = (a * x + c * y + e, b * x + d * y + f);
        let [mut x0, mut y0, mut x1, mut y1] = self.bounds;
        if x1 <= x0 && y1 <= y0 {
            x0 = x;
            x1 = x;
            y0 = y;
            y1 = y;
        }
        self.bounds = [x0.min(x), y0.min(y), x1.max(x), y1.max(y)];
    }
    fn stamp(&mut self, x: f64, y: f64, w: f64, h: f64) -> Result<()> {
        require(
            [x, y, w, h].iter().all(|n| n.is_finite() && n.abs() <= 1e6),
            "Surface coordinate bounds",
        )?;
        let (mut x, mut y, mut w, mut h) = (
            (x + 0.5).floor() as i64,
            (y + 0.5).floor() as i64,
            (w + 0.5).floor() as i64,
            (h + 0.5).floor() as i64,
        );
        if w < 0 {
            x += w;
            w = -w
        }
        if h < 0 {
            y += h;
            h = -h
        }
        for yy in y.max(0)..(y + h).min(self.height) {
            for xx in x.max(0)..(x + w).min(self.width) {
                let value = self.hash
                    ^ ((xx + 1) as u32).wrapping_mul(2654435761)
                    ^ ((yy + 1) as u32).wrapping_mul(40503)
                    ^ self.seed;
                let value = (value ^ (value >> 15)).wrapping_mul(2246822519);
                let value = value ^ (value >> 13);
                let offset = ((yy * self.width + xx) * 4) as usize;
                self.pixels[offset..offset + 4].copy_from_slice(&[
                    value as u8,
                    (value >> 8) as u8,
                    (value >> 16) as u8,
                    255 - ((value >> 24) & 63) as u8,
                ]);
            }
        }
        Ok(())
    }
    fn paint(&mut self) -> Result<()> {
        let [x0, y0, x1, y1] = self.bounds;
        self.stamp(x0 - 1., y0 - 1., x1 - x0 + 2., y1 - y0 + 2.)
    }
    pub fn step(&mut self, name: &str, args: &[Value]) -> Result<()> {
        require(args.len() <= 8, "Surface argument count")?;
        for v in args {
            if let Some(v) = v.as_f64() {
                require(v.is_finite() && v.abs() <= 1e6, "Surface scalar bounds")?
            }
        }
        if let Some(key) = name.strip_prefix("set:") {
            require(
                [
                    "fillStyle",
                    "strokeStyle",
                    "font",
                    "shadowColor",
                    "shadowBlur",
                    "globalCompositeOperation",
                    "textAlign",
                    "textBaseline",
                ]
                .contains(&key),
                "Surface attribute",
            )?;
            let mut value = args
                .first()
                .cloned()
                .ok_or_else(|| Error::new("INVALID_DATA", "Surface assignment value"))?;
            if ["fillStyle", "strokeStyle", "shadowColor"].contains(&key) && value.is_string() {
                let c = string(&value)?;
                require(
                    re(r"^#[0-9a-fA-F]{3}(?:[0-9a-fA-F]{3})?$").is_match(c),
                    "Surface color",
                )?;
                let c = &c[1..];
                value = json!(
                    "#".to_string()
                        + &if c.len() == 3 {
                            c.chars().flat_map(|c| [c, c]).collect::<String>()
                        } else {
                            c.to_string()
                        }
                        .to_lowercase()
                );
            }
            self.style.insert(key.into(), value);
            return Ok(());
        }
        match name {
            "radialGradient" | "stop" => self.note(name, args)?,
            "scale" => {
                self.note(name, args)?;
                let [a, b, c, d, e, f] = self.matrix;
                let (x, y) = (n(&args[0])?, n(&args[1])?);
                self.matrix = [a * x, b * x, c * y, d * y, e, f];
            }
            "beginPath" => {
                self.note(name, &[])?;
                self.bounds = [0.; 4]
            }
            "closePath" => self.note(name, &[])?,
            "moveTo" | "lineTo" => {
                self.note(name, args)?;
                self.point(n(&args[0])?, n(&args[1])?);
            }
            "quadraticCurveTo" | "bezierCurveTo" => {
                self.note(name, args)?;
                self.point(n(&args[0])?, n(&args[1])?);
                self.point(n(&args[args.len() - 2])?, n(&args[args.len() - 1])?);
            }
            "ellipse" => {
                self.note(name, &args[..4])?;
                let (x, y, rx, ry) = (n(&args[0])?, n(&args[1])?, n(&args[2])?, n(&args[3])?);
                self.point(x - rx, y - ry);
                self.point(x + rx, y + ry);
            }
            "arc" => {
                self.note(name, args)?;
                let (x, y, r) = (n(&args[0])?, n(&args[1])?, n(&args[2])?);
                self.point(x - r, y - r);
                self.point(x + r, y + r);
            }
            "stroke" => {
                self.note(
                    name,
                    &[
                        self.style["strokeStyle"].clone(),
                        self.style["lineWidth"].clone(),
                    ],
                )?;
                self.paint()?
            }
            "fill" => {
                self.note(name, &[self.style["fillStyle"].clone()])?;
                self.paint()?
            }
            "fillRect" => {
                let mut values = args.to_vec();
                values.push(self.style["fillStyle"].clone());
                self.note(name, &values)?;
                let color = hex::decode(&string(&self.style["fillStyle"])?[1..])
                    .map_err(|_| Error::new("INVALID_DATA", "Rectangle color"))?;
                let mut rgba = color;
                rgba.push(255);
                let (x, y, w, h) = (
                    (n(&args[0])? + 0.5).floor() as i64,
                    (n(&args[1])? + 0.5).floor() as i64,
                    (n(&args[2])? + 0.5).floor() as i64,
                    (n(&args[3])? + 0.5).floor() as i64,
                );
                for yy in y.max(0)..(y + h).min(self.height) {
                    for xx in x.max(0)..(x + w).min(self.width) {
                        let p = ((yy * self.width + xx) * 4) as usize;
                        self.pixels[p..p + 4].copy_from_slice(&rgba);
                    }
                }
            }
            "fillText" | "strokeText" => {
                let (text, x, y) = (string(&args[0])?, n(&args[1])?, n(&args[2])?);
                let font = string(&self.style["font"])?.to_string();
                let key = if name == "fillText" {
                    "fillStyle"
                } else {
                    "strokeStyle"
                };
                let mut values = vec![
                    args[0].clone(),
                    args[1].clone(),
                    args[2].clone(),
                    json!(font),
                    self.style[key].clone(),
                ];
                if name == "fillText" {
                    values.extend([
                        self.style["textAlign"].clone(),
                        self.style["textBaseline"].clone(),
                    ])
                }
                self.note(name, &values)?;
                let pattern = re(r"^(\d+(?:\.\d+)?)px [A-Za-z0-9 -]+$");
                let m = pattern
                    .captures(&font)
                    .ok_or_else(|| Error::new("INVALID_DATA", "Surface font"))?;
                let size = m[1]
                    .parse::<f64>()
                    .map_err(|_| Error::new("INVALID_DATA", "Font size"))?;
                require(
                    size > 0. && size <= 256. && text.chars().count() <= 256,
                    "Surface text bounds",
                )?;
                let width = text.encode_utf16().count() as f64 * 6.7;
                let [a, b, c, d, e, f] = self.matrix;
                self.stamp(
                    a * x + c * y + e,
                    b * x + d * y + f - size,
                    width,
                    size * 1.3,
                )?;
            }
            _ => return Err(Error::new("INVALID_DATA", "Unknown surface operation")),
        }
        Ok(())
    }
}
pub fn scene(shapes: &Value, gradients: &Value) -> Result<Surface> {
    scene_with_seed(shapes, gradients, seed())
}
pub(crate) fn scene_with_seed(shapes: &Value, gradients: &Value, seed: u32) -> Result<Surface> {
    let shapes = array(shapes)?;
    let gradients = array(gradients)?;
    require(
        !shapes.is_empty() && shapes.len() <= 16 && !gradients.is_empty() && gradients.len() <= 16,
        "Surface scene size",
    )?;
    let mut surface = Surface::new();
    surface.seed = seed;
    surface.resize(48, 48)?;
    surface.step("scale", &[json!(0.384), json!(0.384)])?;
    for (index, shape) in shapes.iter().enumerate() {
        let shape = array(shape)?;
        if let Some(gradient) = gradients.get(index) {
            let gradient = array(gradient)?;
            require(
                gradient.len() == 9 && gradient[0] == 0,
                "Gradient declaration",
            )?;
            surface.step("radialGradient", &gradient[1..7])?;
            surface.step("stop", &[json!(0), gradient[7].clone()])?;
            surface.step("stop", &[json!(1), gradient[8].clone()])?;
            surface.step("set:fillStyle", &[json!({"gradient":index})])?;
        }
        match shape[0].as_u64() {
            Some(1) if shape.len() == 8 => {
                surface.step("set:shadowBlur", &[shape[1].clone()])?;
                surface.step("set:shadowColor", &[shape[2].clone()])?;
                surface.step("set:font", &[shape[3].clone()])?;
                surface.step(
                    if shape[4] == 0 {
                        "fillText"
                    } else {
                        "strokeText"
                    },
                    &shape[5..8],
                )?;
            }
            Some(kind @ 2..=3) if shape.len() == if kind == 2 { 11 } else { 9 } => {
                surface.step("set:shadowBlur", &[shape[1].clone()])?;
                surface.step("set:shadowColor", &[shape[2].clone()])?;
                surface.step("beginPath", &[])?;
                surface.step("moveTo", &shape[3..5])?;
                surface.step(
                    if kind == 2 {
                        "bezierCurveTo"
                    } else {
                        "quadraticCurveTo"
                    },
                    &shape[5..],
                )?;
                surface.step("stroke", &[])?;
                surface.step("fill", &[])?;
            }
            Some(4) if shape.len() == 8 => {
                surface.step("beginPath", &[])?;
                surface.step("ellipse", &shape[1..])?;
                surface.step("stroke", &[])?;
                surface.step("fill", &[])?;
            }
            _ => return Err(Error::new("INVALID_DATA", "Surface shape variant")),
        }
        surface.step("set:shadowBlur", &[json!(0)])?;
    }
    Ok(surface)
}
pub fn metrics(text: &str, size: f64) -> Value {
    let width = text.encode_utf16().count() as f64 * 6.7;
    let descent = size * 0.2;
    let fa = size * 0.9;
    json!([
        width,
        0,
        width,
        size * 0.7,
        descent,
        fa,
        descent,
        null,
        null,
        (fa * 0.8) as f32 as f64,
        0,
        -descent
    ])
}
pub fn colors(rgb: [f64; 3], space: &str, half: bool) -> Result<Vec<Value>> {
    let mut rgb = rgb;
    require(
        rgb.iter().all(|n| n.is_finite() && *n >= 0. && *n <= 1.),
        "Color bounds",
    )?;
    if space == "srgb" {
        let linear = rgb.map(|v| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        });
        let matrix = [
            [1.2249401762805587, -0.2249401762805586, 0.],
            [-0.04205697751790907, 1.0420569775179091, 0.],
            [-0.019636239203287, -0.07863715131854902, 1.0982734115802371],
        ];
        rgb = matrix.map(|row| {
            let v = row.iter().zip(linear).map(|(a, b)| a * b).sum::<f64>();
            if v.abs() <= 0.0031308 {
                v * 12.92
            } else {
                (1.055 * v.abs().powf(1. / 2.4) - 0.055).copysign(v)
            }
        })
    } else {
        require(space == "display-p3", "Color space")?
    }
    Ok(rgb
        .into_iter()
        .chain([1.])
        .map(|v| {
            if half {
                json!(half::f16::from_f64(v).to_f64())
            } else {
                json!(
                    (v.clamp(0., 1.) * 255. + 0.5 - if space == "display-p3" { 1e-9 } else { 0. })
                        .floor() as i32
                )
            }
        })
        .collect())
}
pub fn graphics(
    shapes: &Value,
    gradients: &Value,
    emoji: &[String],
    color: [f64; 3],
) -> Result<Value> {
    let mut s = scene(shapes, gradients)?;
    let digest = windows_digest(&s.pixels);
    s.resize(2, 2)?;
    for (fill, args) in [
        ("#000", vec![json!(0), json!(0), json!(2), json!(2)]),
        ("#fff", vec![json!(2), json!(2), json!(1), json!(1)]),
    ] {
        s.step("set:fillStyle", &[json!(fill)])?;
        s.step("fillRect", &args)?
    }
    s.step("beginPath", &[])?;
    s.step(
        "arc",
        &[
            json!(0),
            json!(0),
            json!(2),
            json!(0),
            json!(1),
            json!(true),
        ],
    )?;
    s.step("closePath", &[])?;
    s.step("fill", &[])?;
    let small = s
        .pixels
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",")
        + ",";
    let metric: Vec<_> = emoji.iter().map(|s| metrics(s, 16.)).collect();
    let checksum = metric
        .iter()
        .map(|m| (m[3].as_f64().unwrap() + m[4].as_f64().unwrap() + m[0].as_f64().unwrap()) * 1e-5)
        .sum::<f64>();
    let unique = metric
        .iter()
        .map(|m| {
            (
                m[3].as_f64().unwrap().to_bits(),
                m[4].as_f64().unwrap().to_bits(),
                m[0].as_f64().unwrap().to_bits(),
            )
        })
        .collect::<std::collections::HashSet<_>>()
        .len();
    s.resize(220, 40)?;
    s.step("set:font", &[json!("16px sans-serif")])?;
    s.resize(49, 44)?;
    s.step("scale", &[json!(0.4), json!(0.4)])?;
    for (index, (fill, x, y)) in [("#f2f", 40, 40), ("#2ff", 80, 40), ("#ff2", 60, 80)]
        .into_iter()
        .enumerate()
    {
        if index < 2 {
            s.step(
                "set:globalCompositeOperation",
                &[json!(if index == 0 {
                    "source-over"
                } else {
                    "multiply"
                })],
            )?
        }
        s.step("set:fillStyle", &[json!(fill)])?;
        s.step("beginPath", &[])?;
        s.step(
            "arc",
            &[
                json!(x),
                json!(y),
                json!(40),
                json!(0),
                json!(std::f64::consts::TAU),
                json!(true),
            ],
        )?;
        s.step("closePath", &[])?;
        s.step("fill", &[])?;
    }
    s.step("set:globalCompositeOperation", &[json!("source-over")])?;
    s.step("set:fillStyle", &[json!("#fff")])?;
    s.step("beginPath", &[])?;
    for radius in [20, 10] {
        s.step(
            "arc",
            &[
                json!(61),
                json!(53),
                json!(radius),
                json!(0),
                json!(std::f64::consts::TAU),
                json!(true),
            ],
        )?
    }
    s.step("fill", &[json!("evenodd")])?;
    let overlap = windows_digest(&s.pixels)[..32].to_string();
    let mut colors_out = Vec::new();
    for half in [false, true] {
        for space in ["srgb", "display-p3"] {
            let storage = if half { "float16" } else { "unorm8" };
            let mut readback = vec![json!(space), json!(format!("rgba-{storage}")), json!(4)];
            readback.extend(colors(color, space, half)?);
            colors_out.push(json!([
                space,
                storage,
                1,
                [1, space, storage, 0, 1],
                readback
            ]));
        }
    }
    let marker = "JzCDC7".to_string() + &digest[..32] + "EETJ6";
    Ok(
        json!({"qLAU7":marker,"uqDH2":marker,"YGgU2":"LDst3","EVegU8":"iPjX7","xxpv6":"bIojw0","amRnx3":"NmVj1","yckm8":[1,0,0],"rzsf5":"WhFi7","IKqRH7":"mKro0","pHsB0":"FuOh7","nbFk6":"RGkW0","PAiY9":"ewUxO8","UkAR4":"JkMX5","iSmuC7":[small,unique,checksum,overlap,0,metric,[[1,"srgb","unorm8",0,1],[1,"srgb","unorm8",0,1]],colors_out,null,null,null]}),
    )
}
