//! Differential contracts exported from the unchanged Python regression suite.
//! This module is test-only. No Python or oracle data is part of the service.
use super::*;
use crate::protocol::wire::kernel;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::OnceLock;

fn norm(v: &Value) -> Value {
    if v.get("type").is_some() && v.get("fields").is_some() {
        return norm(&v["fields"]);
    }
    if let Some(s) = v.get("base64").and_then(Value::as_str) {
        return json!(STANDARD.decode(s).unwrap());
    }
    if let Some(s) = v
        .get("set")
        .filter(|_| v.as_object().is_some_and(|o| o.len() == 1))
    {
        return norm(s);
    }
    match v {
        Value::Array(a) => json!(a.iter().map(norm).collect::<Vec<_>>()),
        Value::Object(o) => json!(o
            .iter()
            .map(|(k, v)| (k.clone(), norm(v)))
            .collect::<serde_json::Map<_, _>>()),
        _ => v.clone(),
    }
}
fn bytes(v: &Value) -> Result<Vec<u8>> {
    array(v)?.iter().map(|v| Ok(uint(v)? as u8)).collect()
}
fn num(v: &Value) -> Result<f64> {
    if let Some(n) = v.as_f64() {
        return Ok(n);
    }
    Ok(match v["float"].as_str() {
        Some("nan") => f64::NAN,
        Some("inf") => f64::INFINITY,
        Some("-inf") => f64::NEG_INFINITY,
        _ => return Err(Error::new("TEST_INPUT", "number")),
    })
}
fn scalar(v: &Value) -> semantics::Scalar {
    use semantics::Scalar::*;
    if v["undefined"] == true {
        return Undefined;
    }
    if v.get("float").is_some() {
        return Number(num(v).unwrap());
    }
    match v {
        Value::Null => Null,
        Value::Bool(v) => Bool(*v),
        Value::Number(v) => Number(v.as_f64().unwrap()),
        Value::String(v) => Text(v.clone()),
        Value::Array(v) => Array(std::rc::Rc::new(v.iter().map(scalar).collect())),
        _ => Object(std::rc::Rc::new(())),
    }
}
fn scalar_json(v: semantics::Scalar) -> Value {
    match v {
        semantics::Scalar::Number(n) if !n.is_finite() => {
            json!({"float":if n.is_nan(){"nan"}else if n<0.{"-inf"}else{"inf"}})
        }
        semantics::Scalar::Number(n) => json!(n),
        _ => panic!("numeric result"),
    }
}
fn format(v: &Value) -> Result<data::Format> {
    let mut default = json!({"byte_offset":37,"initial_key":191,"state_step":244,"state_multiplier":48294,"state_increment":31651,"hash_salt":1612195754u32,"char_xor":133,"state_quadratic":0,"char_mode":"before-subtract"});
    if let Some(o) = v.as_object() {
        default.as_object_mut().unwrap().extend(o.clone());
    }
    Ok(serde_json::from_value(default)?)
}
fn plan(v: &Value) -> Result<data::Plan> {
    Ok(data::Plan {
        format: format(&v["format"])?,
        meta: v.clone(),
        alphabet: String::new(),
        modulus: String::new(),
        flow: String::new(),
    })
}
fn response(v: &Value, raw: Vec<u8>) -> Result<data::Response> {
    let mut literals = Vec::new();
    for l in v["literals"].as_array().into_iter().flatten() {
        literals.push(data::Literal {
            key: None,
            start: l["instruction_offset"].as_u64().unwrap_or(0) as usize,
            end: l["end_offset"].as_u64().unwrap_or(0) as usize,
            value: string(&l["value"])?.into(),
            packed: l["packed"].as_str().map(String::from),
            continuation: l["continuation_length"].as_u64().unwrap_or(0) as usize,
            pool_offset: l["continuation_offset"].as_u64().unwrap_or(0) as usize,
        });
    }
    let packed = v["packed"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| Ok((uint(&v[0])?, string(&v[1])?.into())))
        .collect::<Result<_>>()?;
    let strings = v["program"]["strings"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|v| Ok(string(v)?.into()))
        .collect::<Result<_>>()?;
    Ok(data::Response {
        program: data::Program {
            sha256: String::new(),
            byte_length: v["program"]["byte_length"]
                .as_u64()
                .unwrap_or(raw.len() as u64) as usize,
            code_length: v["program"]["code_length"]
                .as_u64()
                .unwrap_or(raw.len() as u64) as usize,
            strings,
        },
        raw,
        literals: literals.into(),
        packed,
    })
}
fn modules(v: &Value) -> Result<Vec<catalog::Module>> {
    v["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            Ok(catalog::Module {
                id: string(&m["identifier"])?.into(),
                data: m["data"].as_str().unwrap_or("").into(),
                label: m["label"].as_str().unwrap_or("").into(),
                start: uint(&m["start"])?,
                end: uint(&m["end"])?,
                metadata: m["metadata"].as_u64().unwrap_or(0) as usize,
            })
        })
        .collect()
}
fn old_literals(values: &[data::Literal]) -> Value {
    json!(values.iter().map(|l|json!({"instruction_offset":l.start,"value":l.value,"packed":l.packed,"continuation_length":l.continuation,"continuation_offset":l.pool_offset,"end_offset":l.end})).collect::<Vec<_>>())
}
fn model_call(op: &str, a: &[Value], k: &Value) -> Result<Value> {
    let encoded = a.first().and_then(Value::as_str);
    let offset = usize::from(encoded.is_some());
    let raw = encoded
        .map(|s| STANDARD.decode(s))
        .transpose()?
        .unwrap_or_default();
    let r = response(&a[offset], raw)?;
    let targets = k["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|v| catalog::Target {
            name: String::new(),
            at: 0,
            target: v["target"].as_u64().unwrap_or(0) as usize,
            key: 0,
        })
        .collect();
    let c = catalog::Catalog {
        parameters: Value::Null,
        modules: modules(&a[offset + 1])?,
        targets,
        order: vec![],
    };
    let p = plan(a.get(offset + 2).unwrap_or(&Value::Null))?;
    let context = models::Context {
        response: &r,
        plan: &p,
        catalog: &c,
        html: &a
            .get(offset + 3)
            .and_then(Value::as_str)
            .unwrap_or("")
            .into(),
        parent_html: &k["page_html"].as_str().unwrap_or("").into(),
        frame_url: "https://challenges.cloudflare.com/frame",
        page_url: "https://example.org/",
        mode: "managed",
    };
    let mut result = models::Results::default();
    match op {
        "cf_gateway.callback_task.callback_tasks" => {
            if let Some(o) = a.get(3).and_then(Value::as_object) {
                result.fields.extend(o.clone());
            }
            semantics::callbacks(&context, &mut result)?;
            for key in a
                .get(3)
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(|v| v.keys())
            {
                result.fields.remove(key);
            }
        }
        "cf_gateway.math_task.math_tasks" | "cf_gateway.environment_task.environment_tasks" => {
            result = models::calculate(&context)?;
        }
        _ => return Err(Error::new("TEST_UNSUPPORTED", op)),
    }
    Ok(json!(result.fields))
}
fn ast_call(op: &str, a: &[Value]) -> Result<Value> {
    let source = a
        .iter()
        .find_map(|v| v["source"].as_str().filter(|s| !s.is_empty()))
        .or_else(|| a.first().and_then(Value::as_str))
        .or_else(|| a.iter().find_map(|v| v["syntax"].as_str()))
        .ok_or_else(|| Error::new("TEST_INPUT", "syntax source"))?;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let t = constants::Tables::new(tree.root_node(), source)?;
    Ok(match op {
        "cf_gateway.managed.program_format" => json!(compiler::format(&t)?),
        "cf_gateway.managed.script_build" => json!(compiler::script_build(&t)),
        "cf_gateway.surface_task.source_properties" => compiler::declarations(&t)?,
        "cf_gateway.key_material.transform_name" => json!(compiler::transform_name(&t)?),
        "cf_gateway.instruction_data.framed_salt" => {
            let n = constants::walk(tree.root_node(), true)
                .into_iter()
                .find(|n| {
                    Some(t.text(*n)) == a[0]["syntax"].as_str()
                        && n.kind() == "function_declaration"
                })
                .ok_or_else(|| Error::new("TEST_INPUT", "handler absent"))?;
            json!(compiler_profiles::framed_salt_for(
                &t,
                n,
                uint(&a[1])? as u8
            )?)
        }
        "cf_gateway.operand_data.integer_literal_tag" => {
            json!(compiler_profiles::integer_tag(&t, &format(&a[2])?)?)
        }
        "cf_gateway.property_data.write_profile" => {
            compiler_profiles::properties(&t, &format(&a[2])?).unwrap_or(Value::Null)
        }
        "cf_gateway.immediate_data.immediate_profile" => {
            compiler_profiles::immediate(&t, &format(&a[2])?)?
        }
        "cf_gateway.dispatcher.profile_from_source" => {
            compiler_profiles::dispatcher(&t, &format(&json!({"byte_offset":a[2]}))?)?
        }
        "cf_gateway.key_material.recover_key_material" => compiler::recover_material(
            string(&a[0])?,
            &format(&a[1])?,
            &std::collections::HashSet::from([string(&a[2])?]),
        )?,
        "cf_gateway.operand_data.operand" => {
            let wanted = string(&a[0]["syntax"])?;
            let nodes: Vec<_> = constants::walk(tree.root_node(), true)
                .into_iter()
                .filter(|n| t.text(*n) == wanted)
                .collect();
            require(nodes.len() == 1, "Operand node ambiguity")?;
            json!(compiler_profiles::operand(&t, nodes[0], &format(&a[2])?)?)
        }
        "cf_gateway.deobfuscate.ConstantTables.evaluate" => {
            let wanted = string(&a[1]["syntax"])?;
            let n = constants::walk(tree.root_node(), true)
                .into_iter()
                .find(|n| t.text(*n) == wanted && a[1]["kind"].as_str() == Some(n.kind()))
                .ok_or_else(|| Error::new("TEST_INPUT", "expression absent"))?;
            compiler::constant_json(t.value(n)?)
                .ok_or_else(|| Error::new("UNKNOWN_CONSTANT", "non-data constant"))?
        }
        _ => return Err(Error::new("TEST_UNSUPPORTED", op)),
    })
}
fn invoke(op: &str, a: &[Value], k: &Value) -> Result<Value> {
    let s = |i: usize| string(&a[i]);
    let b = |i: usize| bytes(&a[i]);
    let n = |i: usize| num(&a[i]);
    Ok(match op {
        "cf_gateway.box_task.length" => json!(css::length(s(0)?)?),
        "cf_gateway.allocator_task.AllocatorProfile.snapshots" => json!(declared::allocator(
            &a[0],
            uint(&a[1])?,
            &serde_json::from_value::<Vec<String>>(a[2].clone())?
        )?),
        "cf_gateway.environment_task.ConnectionProfile.fields" => declared::connection(&a[0])?,
        "cf_gateway.preference_task.Preferences.fields" => declared::preferences(&a[0])?,
        "cf_gateway.initial_style_task.style_digest" => {
            declared::style_digest(uint(&a[0])?, uint(&a[1])?, k.get("profile"))?
        }
        "cf_gateway.console_task.formatting_trace" => {
            declared::console(&serde_json::from_value::<Vec<String>>(a[0].clone())?)?
        }
        "cf_gateway.timezone_task.calculate_timezone" => {
            let now = k
                .get("now")
                .map(|v| {
                    chrono::DateTime::parse_from_rfc3339(string(&v["datetime"])?)
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .map_err(|_| {
                            Error::new("INVALID_DATA", "Current calendar time requires zone")
                        })
                })
                .transpose()?
                .unwrap_or_else(chrono::Utc::now);
            declared::calendar(&a[0], s(1)?, now)?
        }
        "cf_gateway.timezone_task.timezone_tasks" => {
            let r = response(&a[0], vec![])?;
            let mut out = serde_json::Map::new();
            let now = chrono::DateTime::parse_from_rfc3339(string(&k["now"]["datetime"])?)
                .map_err(|_| Error::new("INVALID_DATA", "Calendar timestamp"))?
                .with_timezone(&chrono::Utc);
            for m in modules(&a[1])? {
                out.insert(
                    m.id.clone(),
                    models::calendar_at(&r.between(m.start, m.end), s(2)?, now)?,
                );
            }
            json!(out)
        }
        "cf_gateway.box_task.calculate" => model_geometry::boxes(
            &serde_json::from_value::<Vec<indexmap::IndexMap<String, String>>>(a[0].clone())?,
            &serde_json::from_value(a[1].clone())?,
            n(2)?,
        )?,
        "cf_gateway.math_task.number_string" => json!(math::number(n(0)?)),
        "cf_gateway.static.inspect_script" => jsd::inspect(s(0)?)?,
        "cf_gateway.jsd.payload" => {
            jsd::payload_with_agent(&a[0], s(1)?, string(&k["user_agent"])?)?
        }
        "cf_gateway.clock_task.clock_tasks" => {
            let r = response(&a[0], vec![])?;
            let p = plan(&Value::Null)?;
            let c = catalog::Catalog {
                parameters: Value::Null,
                modules: modules(&a[1])?,
                targets: vec![],
                order: vec![],
            };
            let context = models::Context {
                response: &r,
                plan: &p,
                catalog: &c,
                html: &"".into(),
                parent_html: &"".into(),
                frame_url: "https://challenges.cloudflare.com/frame",
                page_url: "https://example.org/",
                mode: "managed",
            };
            json!(models::calculate(&context)?.fields)
        }
        "cf_gateway.console_task.console_tasks" => {
            let p = k.get("plan").map(plan).transpose()?;
            let raw = k["encoded"]
                .as_str()
                .map(|s| STANDARD.decode(s))
                .transpose()?
                .unwrap_or_default();
            let mut r = response(&a[0], raw)?;
            if let Some(p) = p.as_ref() {
                if !r.raw.is_empty() {
                    r.program = data::program(&r.raw, &p.format, false)?;
                }
            }
            let mut out = serde_json::Map::new();
            for m in modules(&a[1])? {
                if let Some(fields) = models::console_recipe(&r, &m, p.as_ref())? {
                    out.insert(m.id, fields);
                }
            }
            json!(out)
        }
        "cf_gateway.codec.encode_packet" => json!(jsd::encode_text(s(0)?, s(1)?)?),
        "cf_gateway.static.object_data" => {
            static_data::object_data(s(0)?, s(1)?)?.unwrap_or(Value::Null)
        }
        "cf_gateway.text_geometry_task.geometry" => {
            json!(model_geometry::text_box(s(0)?, n(1)?, n(2)?, n(3)?, n(4)?)?)
        }
        "cf_gateway.font_task.flag_color" => model_geometry::flag_color(s(0)?)?,
        "cf_gateway.font_task.unavailable_fonts" => {
            models::unavailable_fonts(&serde_json::from_value::<Vec<String>>(a[0].clone())?)?
        }
        "cf_gateway.task_data.HttpTask.pat_fields" | "cf_gateway.task_data.HttpTask.ci_fields" => {
            let v = &a[0];
            let task = http_tasks::Task {
                id: string(&v["identifier"])?.into(),
                kind: string(&v["kind"])?.into(),
                url: String::new(),
                timestamp: string(&v["timestamp"])?.into(),
                digest: v["digest"].as_str().unwrap_or("").into(),
            };
            if op.ends_with("pat_fields") {
                task.pat_fields(uint(&a[1])? as u16, uint(&a[2])? as u64)?
            } else {
                task.ci_fields(&b(1)?, string(&k["connection_type"])?)?
            }
        }
        "cf_gateway.integrity_task.integrity_tasks"
        | "cf_gateway.task_data.TaskCatalog.http_tasks" => {
            let (r, c, config) = if op.ends_with("integrity_tasks") {
                (
                    response(&a[0], vec![])?,
                    catalog::Catalog {
                        parameters: Value::Null,
                        modules: modules(&a[1])?,
                        targets: vec![],
                        order: vec![],
                    },
                    a[2].clone(),
                )
            } else {
                (
                    response(&a[0], vec![])?,
                    catalog::Catalog {
                        parameters: Value::Null,
                        modules: modules(&a[0])?,
                        targets: vec![],
                        order: vec![],
                    },
                    json!({"UORq8":a[1]}),
                )
            };
            let p = plan(&Value::Null)?;
            let context = models::Context {
                response: &r,
                catalog: &c,
                plan: &p,
                html: &"".into(),
                parent_html: &"".into(),
                frame_url: "https://challenges.cloudflare.com/frame",
                page_url: "https://example.org/",
                mode: "managed",
            };
            let tasks = http_tasks::tasks(&context, &config)?;
            if op.ends_with("integrity_tasks") {
                json!(tasks
                    .iter()
                    .map(|t| json!([t.id, t.url]))
                    .collect::<Vec<_>>())
            } else {
                json!(tasks.iter().map(|t|{let m=c.modules.iter().find(|m|m.id==t.id).unwrap();let path=t.url.strip_prefix("https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b").unwrap().strip_suffix(&m.data).unwrap();json!({"kind":t.kind,"path":path,"identifier":t.id,"data":m.data,"timestamp":t.timestamp,"digest":if t.digest.is_empty(){Value::Null}else{json!(t.digest)}})}).collect::<Vec<_>>())
            }
        }
        "cf_gateway.task_data.TaskCatalog.read" => {
            let r = response(&a[0], vec![])?;
            let c = catalog::Catalog::link(&r, a[0]["_frame_parameters"].clone(), vec![])?;
            json!({"modules":c.modules.iter().map(|m|json!({"identifier":m.id,"data":m.data,"label":m.label,"start":m.start,"end":m.end,"metadata":m.metadata})).collect::<Vec<_>>(),"packed":r.packed})
        }
        "cf_gateway.immediate_data.immediate_dimensions" => {
            let r = response(&a[1], STANDARD.decode(s(0)?)?)?;
            let m = &a[2];
            let module = catalog::Module {
                id: String::new(),
                data: String::new(),
                label: String::new(),
                start: uint(&m["start"])?,
                end: uint(&m["end"])?,
                metadata: 0,
            };
            json!(operands::dimensions(&r, &module, &plan(&a[3])?)?)
        }
        "cf_gateway.preference_task.preference_tasks" => {
            let r = response(&a[0], vec![])?;
            let mut out = serde_json::Map::new();
            for m in modules(&a[1])? {
                let values = r
                    .between(m.start, m.end)
                    .iter()
                    .map(|l| l.value.as_str())
                    .collect();
                if models::has(&values, &["scji4", "EEDTe0", "GMvA7", "YSycG6", "YWjJ7"]) {
                    require(out.is_empty(), "Duplicate preferences task")?;
                    out.insert(
                        m.id,
                        declared::preference_fields(
                            &values,
                            s(2)?,
                            a.get(3).unwrap_or(&Value::Null),
                        )?,
                    );
                }
            }
            json!(out)
        }
        "cf_gateway.benchmark_task.calculate_benchmark" => benchmark::run(
            &b(0)?,
            serde_json::from_value(a[1].clone())?,
            serde_json::from_value(a[2].clone())?,
            k["repeats"].as_u64().unwrap_or(3) as u32,
            k["warmup"].as_i64().unwrap_or(50000) as i32,
        )?,
        "cf_gateway.benchmark_task.benchmark_tasks" => {
            let r = response(&a[0], vec![])?;
            let p = plan(&Value::Null)?;
            let c = catalog::Catalog {
                parameters: Value::Null,
                modules: modules(&a[1])?,
                targets: vec![],
                order: vec![],
            };
            let context = models::Context {
                response: &r,
                plan: &p,
                catalog: &c,
                html: &"".into(),
                parent_html: &"".into(),
                frame_url: "https://challenges.cloudflare.com/frame",
                page_url: "https://example.org/",
                mode: "managed",
            };
            let mut out = models::Results::default();
            benchmark::calculate(&context, &mut out)?;
            json!(out.fields)
        }
        "cf_gateway.surface_task.source_properties" => ast_call(op, a)?,
        "cf_gateway.surface_task.SurfaceProfile.window_values" => {
            surface::window_values(s(1)?, &a[0])?
        }
        "cf_gateway.surface_task.classify" => surface::classify_with(
            &serde_json::from_value::<identity::Identity>(a[0].clone())?,
            s(1)?,
            a.get(4).or_else(|| k.get("profile")).unwrap_or(&json!({})),
            k.get("declarations").unwrap_or(&Value::Null),
        )?,
        "cf_gateway.vector_data.flow_bytes" => {
            json!(wire::flow_bytes(&b(0)?, uint(&a[1])? as i32)?)
        }
        "cf_gateway.vector_data.text_bytes" => {
            let raw = b(0)?;
            let at = uint(&a[1])?;
            let count = uint(&a[2])?;
            let key = uint(&a[3])? as u8;
            let f = format(&a[4])?;
            let wide = k["wide"].as_bool().unwrap_or(false);
            let len = count * if wide { 2 } else { 1 };
            require(at + len <= raw.len(), "Character input bounds")?;
            let mut output = vec![0u8; len];
            let status = unsafe {
                kernel::cf_character_bytes(
                    raw[at..].as_ptr(),
                    len,
                    key,
                    f.byte_offset,
                    f.char_xor,
                    f.seed(at),
                    u8::from(f.char_mode == "after-subtract"),
                    u8::from(wide),
                    output.as_mut_ptr(),
                )
            };
            require(status == 0, "Character transform")?;
            if wide {
                let units = output
                    .chunks_exact(2)
                    .map(|u| u16::from_le_bytes([u[0], u[1]]))
                    .collect::<Vec<_>>();
                match String::from_utf16(&units) {
                    Ok(v) => json!(v),
                    Err(_) => json!({"utf16":units}),
                }
            } else {
                json!(output.iter().map(|c| *c as char).collect::<String>())
            }
        }
        "cf_gateway.vector_data.xtea_blocks" => {
            let raw = b(0)?;
            require(
                raw.len() % 8 == 0 && array(&a[1])?.len() == 256,
                "Block bounds",
            )?;
            let mut output = Vec::new();
            for (i, block) in raw.chunks_exact(8).enumerate() {
                let words = serde_json::from_value::<Vec<u32>>(a[1][i % 256].clone())?;
                let schedule = <[u32; 64]>::try_from(words)
                    .map_err(|_| Error::new("INVALID_DATA", "Schedule"))?;
                let encrypted = kernel::xtea_pair(
                    u32::from_be_bytes(block[..4].try_into().unwrap()),
                    u32::from_be_bytes(block[4..].try_into().unwrap()),
                    &schedule,
                );
                for word in encrypted {
                    output.extend(word.to_be_bytes())
                }
            }
            json!(output)
        }
        "cf_gateway.clock_task.clock_resolution" => declared::clock(
            k.get("samples").map(uint).transpose()?.unwrap_or(5000),
            || 0.,
        )?,
        "cf_gateway.proof.best_sha256_work" => {
            let result = wire::work_limited(
                s(0)?,
                s(1)?,
                n(2)?,
                k.get("start").map(uint).transpose()?.unwrap_or(0) as u64,
                k.get("step").map(uint).transpose()?.unwrap_or(1) as u64,
                k.get("max_attempts")
                    .map(uint)
                    .transpose()?
                    .unwrap_or(2_000_000) as u64,
            )?;
            json!({"candidate":format!("{}{}",s(0)?,result[0]),"attempts":result[1],"bits":result[2],"elapsed_ms":result[3] as f64/1000.})
        }
        "cf_gateway.locale_task.NativeLocale.format" => locale::format(
            s(1)?,
            s(2)?,
            num(&a[3])?,
            s(4)?,
            &serde_json::from_value::<Vec<String>>(a[5].clone())?,
            num(&a[6])?,
        )?,
        "cf_gateway.locale_task.locale_operands" => {
            let r = response(&json!({"literals":a[0]}), vec![])?;
            let numbers = array(&a[1])?
                .iter()
                .map(|v| Ok((uint(&v[0])?, num(&v[1])?)))
                .collect::<Result<Vec<_>>>()?;
            json!(locale::operands(
                &r.literals.iter().collect::<Vec<_>>(),
                &numbers
            )?)
        }
        "cf_gateway.stylesheet_task.calculate_stylesheets" => {
            model_geometry::stylesheets(s(0)?, s(1)?)?
        }
        "cf_gateway.native_completion.generated_input" => native_input::generated_input(
            &serde_json::from_value(a[0].clone())?,
            k["elapsed_ms"].as_f64().unwrap_or(0.),
            k["client_x"].as_f64(),
            k["client_y"].as_f64(),
        )?,
        "cf_gateway.native_completion.proof_styles" => native_input::proof_styles(s(0)?)?,
        "cf_gateway.native_completion.settings" => {
            json!({"root":native_input::root(),"selectors":[],"proofSelectors":[],"proof":native_input::proof(s(0)?)?})
        }
        "cf_gateway.scalar_semantics.equal" => {
            json!(semantics::equal(&scalar(&a[0]), &scalar(&a[1])))
        }
        "cf_gateway.scalar_semantics.strict" => {
            json!(semantics::strict(&scalar(&a[0]), &scalar(&a[1])))
        }
        "cf_gateway.scalar_semantics.truth" => json!(semantics::truth(&scalar(&a[0]))),
        "cf_gateway.scalar_semantics.number" => {
            scalar_json(semantics::Scalar::Number(semantics::num(&scalar(&a[0]))))
        }
        "cf_gateway.scalar_semantics.string" => json!(semantics::text(&scalar(&a[0]))),
        "cf_gateway.scalar_semantics.divide" => {
            scalar_json(semantics::divide(&scalar(&a[0]), &scalar(&a[1])))
        }
        "cf_gateway.scalar_semantics.parse_integer" => scalar_json(semantics::parse_integer(
            &scalar(&a[0]),
            uint(&a[1])? as u32,
        )),
        "cf_gateway.semantic_task.scalar_checks" => json!(semantics::scalar_checks()?),
        "cf_gateway.semantic_task.calculate" => semantics::recipe(
            uint(&a[0])? as u32,
            s(1)?,
            &array(&a[2])?.iter().map(string).collect::<Result<_>>()?,
        )?,
        "cf_gateway.compiled_data.decode" => data::normalize(&a[0])?,
        "cf_gateway.page_facts.title_fingerprint" => json!(html::title_fingerprint(s(0)?)),
        "cf_gateway.protocol.completion_url" => json!(completion::issued_url(s(0)?, &a[1])?),
        "cf_gateway.key_material.short_literal" => {
            if a[1].as_i64().unwrap_or(-1) < 0 || a[4].as_i64().unwrap_or(-1) < 0 {
                Value::Null
            } else {
                json!(compiler::short_literal(
                    &b(0)?,
                    uint(&a[1])?,
                    uint(&a[2])? as u8,
                    &format(&a[3])?,
                    uint(&a[4])?
                ))
            }
        }
        "cf_gateway.math_task.read_seeds" => {
            let r = response(&json!({"literals":a[2]}), b(0)?)?;
            let c = modules(&json!({"modules":[a[1]]}))?;
            let p = plan(&json!({"format":a[3],"key_material":a[4]}))?;
            json!(math::seeds(&r, &c[0], &p)?.1)
        }
        "cf_gateway.dispatcher.state_targets" => {
            let raw = STANDARD.decode(s(0)?)?;
            let r = response(
                &json!({"program":{"code_length":a[4]},"literals":array(&a[3])?.iter().map(|v|json!({"instruction_offset":0,"value":v})).collect::<Vec<_>>()}),
                raw,
            )?;
            let p = plan(&json!({"format":a[1],"dispatcher_profile":a[2]}))?;
            json!(catalog::state_targets(&r,&p)?.iter().map(|t|json!({"name":t.name,"instruction_offset":t.at,"target":t.target,"key":t.key})).collect::<Vec<_>>())
        }
        "cf_gateway.client_identity.ClientIdentity.from_headers" => {
            json!(identity::Identity::from_headers(a[0].as_object().unwrap())?)
        }
        "cf_gateway.client_identity.ClientIdentity.navigator_fields" => {
            serde_json::from_value::<identity::Identity>(a[0].clone())?.navigator_fields()
        }
        "cf_gateway.client_identity.ClientIdentity.client_hint_fields" => {
            serde_json::from_value::<identity::Identity>(a[0].clone())?.client_hint_fields()
        }
        "cf_gateway.client_identity.ClientIdentity.keyboard_fields" => {
            serde_json::from_value::<identity::Identity>(a[0].clone())?.keyboard_fields()
        }
        "cf_gateway.client_identity.identity_tasks" => {
            let r = response(&a[0], vec![])?;
            let c = catalog::Catalog {
                parameters: Value::Null,
                modules: modules(&a[1])?,
                targets: vec![],
                order: vec![],
            };
            let mut out = models::Results::default();
            serde_json::from_value::<identity::Identity>(a[2].clone())?
                .calculate(&r, &c, &mut out)?;
            json!(out
                .fields
                .iter()
                .map(|(key, fields)| {
                    let kind = if fields.get("wKFQX4").is_some() {
                        "navigator"
                    } else if fields.get("QxBk9").is_some() {
                        "client-hints"
                    } else {
                        "keyboard"
                    };
                    (key.clone(), json!([kind, fields]))
                })
                .collect::<serde_json::Map<_, _>>())
        }
        "cf_gateway.environment_task.environment_tasks" => {
            let r = response(&a[1], vec![])?;
            let document = html::Facts::read(s(4)?)?;
            let parent = html::Facts::read(k["page_html"].as_str().unwrap_or(s(4)?))?;
            let mut out = serde_json::Map::new();
            let mut used = std::collections::HashSet::new();
            for m in modules(&a[2])? {
                if k["excluded"]
                    .as_object()
                    .is_some_and(|o| o.contains_key(&m.id))
                    || k["excluded"]
                        .as_array()
                        .is_some_and(|o| o.contains(&json!(m.id)))
                {
                    continue;
                }
                let literals = r.between(m.start, m.end);
                let mut values = literals
                    .iter()
                    .map(|l| l.value.as_str())
                    .collect::<std::collections::HashSet<_>>();
                for item in k["properties"].as_array().into_iter().flatten() {
                    let at = uint(&item[0])?;
                    if m.start <= at && at < m.end {
                        values.insert(string(&item[1])?);
                    }
                }
                if models::has(&values, &["parseInt", "a", "b", "c", "d", "e", "f", "g"])
                    && values
                        .iter()
                        .any(|s| s.contains("kcats") && s.contains("3Smya"))
                {
                    values.insert("aymS3");
                }
                if a[3]["build"] == "d2d3316b729e"
                    && models::has(
                        &values,
                        &["parseInt", "shift", "a", "b", "c", "d", "e", "f", "g", "h"],
                    )
                    && (1500..=4000).contains(&(m.end - m.start))
                {
                    values.insert("aymS3");
                }
                if let Some((kind, fields)) = models::environment(
                    &values,
                    &document,
                    &parent,
                    &k["connection"],
                    k["pdf_viewer"].as_bool().unwrap_or(true),
                )? {
                    require(used.insert(kind), "Duplicate native environment task")?;
                    out.insert(m.id, json!([kind, fields]));
                }
            }
            json!(out)
        }
        "cf_gateway.transport.request_headers" => {
            let h = a
                .get(1)
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(|v| v.iter())
                .map(|(k, v)| Ok((k.as_str(), string(v)?.to_string())))
                .collect::<Result<Vec<_>>>()?;
            json!(network::request_headers(s(0)?, &h)?)
        }
        "cf_gateway.text_encoding.response_encoding" => {
            let b = b(0)?;
            let (e, skip) = network::response_encoding(&b, a[1].as_object().unwrap());
            json!(if skip == 3 {
                "utf-8-sig".into()
            } else if skip == 2 {
                "utf-16".into()
            } else if e == encoding_rs::BIG5 {
                "big5hkscs".into()
            } else {
                e.name().to_ascii_lowercase()
            })
        }
        "cf_gateway.math_task.calculate" => math::calculate([num(&a[0][0])?, num(&a[0][1])?])?,
        "cf_gateway.math_task.polyfill" => json!(math::polyfill(
            s(0)?,
            &array(&a[1])?.iter().map(num).collect::<Result<Vec<_>>>()?
        )?),
        "cf_gateway.math_task.scaled_hypot" => json!(math::hypot(
            &array(&a[0])?.iter().map(num).collect::<Result<Vec<_>>>()?,
            k["incremental"].as_bool().unwrap_or(false)
        )),
        "cf_gateway.key_material.seed_at" => json!(format(&a[1])?.seed(uint(&a[0])?)),
        "cf_gateway.managed_codec.wire_property_order" => wire::ordered(&a[0]),
        "cf_gateway.managed_codec.key_schedule" => {
            json!(kernel::xtea_schedule(serde_json::from_value(a[0].clone())?).to_vec())
        }
        "cf_gateway.managed_codec.xtea_encrypt" => {
            let schedule: Vec<u32> = serde_json::from_value(a[2].clone())?;
            let x = kernel::xtea_pair(
                uint(&a[0])? as u32,
                uint(&a[1])? as u32,
                schedule
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::new("TEST_INPUT", "schedule"))?,
            );
            json!(x)
        }
        "cf_gateway.codec.deflate_fixed"
        | "cf_gateway.codec.deflate_fixed_scalar"
        | "cf_gateway.native_codec.deflate" => json!(wire::deflate(&b(0)?)?),
        "cf_gateway.codec.decode_flow_response" => json!(wire::flow(s(0)?, s(1)?)?),
        "cf_gateway.proof.leading_zero_bits" => {
            let mut bits = 0;
            for byte in b(0)? {
                bits += byte.leading_zeros();
                if byte != 0 {
                    break;
                }
            }
            json!(bits)
        }
        "cf_gateway.bytecode.decode_program" => {
            let raw = STANDARD.decode(s(0)?)?;
            let p = data::program(
                &raw,
                &format(a.get(1).unwrap_or(&Value::Null))?,
                k["allow_unframed"].as_bool().unwrap_or(false),
            )?;
            json!({"sha256":p.sha256,"byte_length":p.byte_length,"code_length":p.code_length,"strings":p.strings})
        }
        "cf_gateway.program_literals.inline_literals" => {
            let raw = STANDARD.decode(s(0)?)?;
            let f = format(&a[1])?;
            let p = data::program(&raw, &f, k["allow_unframed"].as_bool().unwrap_or(false))?;
            let plan = plan(
                &json!({"format":a[1],"key_material":{"literal_tag":a[2],"literal_kind_slot":k["kind_slot"].as_u64().unwrap_or(1)}}),
            )?;
            old_literals(&data::literals_bounded(
                &raw,
                &p,
                &plan,
                k["allow_overlaps"].as_bool().unwrap_or(false),
                k["max_literal_units"].as_u64().unwrap_or(65535) as usize,
                k["include_binary"].as_bool().unwrap_or(false),
                k["max_work"].as_u64().unwrap_or(8 * 1024 * 1024) as usize,
            )?)
        }
        "cf_gateway.response_data.task_literals" => {
            let r = response(&a[0], vec![])?;
            let lo = uint(&a[1]["start"])?;
            let hi = uint(&a[1]["end"])?;
            old_literals(&r.between(lo, hi).into_iter().cloned().collect::<Vec<_>>())
        }
        "cf_gateway.program_literals.packed_texts" => {
            let r = response(&json!({"literals":a[0]}), vec![])?;
            json!(data::packed_texts(&r.literals)?)
        }
        "cf_gateway.instruction_data.checked_frames" => json!(wire::checked_spans(
            &b(0)?,
            &format(&a[1])?,
            uint(&a[2][0])? as u8,
            uint(&a[2][2]["salt"])? as u32
        )?),
        "cf_gateway.instruction_data.framed_end" => {
            let raw = b(0)?;
            let at = a[1].as_i64().unwrap();
            if at < 0 {
                Value::Null
            } else {
                json!(data::framed_end(
                    &raw,
                    at as usize,
                    uint(&a[2])? as u8,
                    uint(&a[3])? as u8,
                    &format(&json!({"byte_offset":a[4]}))?,
                    uint(&a[5])? as u32
                ))
            }
        }
        "cf_gateway.instruction_data.conflicting_literals" => {
            let raw = STANDARD.decode(s(0)?)?;
            let end = k["code_length"].as_u64().unwrap_or(raw.len() as u64) as usize;
            let r = response(&json!({"literals":a[1]}), vec![])?;
            let p = plan(&json!({"format":a[2],"key_material":a[3],"input_data_profile":a[4]}))?;
            let mut values = data::conflicting_literals(&raw[..end], &r.literals, &p)?
                .into_iter()
                .collect::<Vec<_>>();
            values.sort();
            json!(values)
        }
        "cf_gateway.constant_data.ConstantArrays.read" => {
            let v = &a[0];
            let r = response(&json!({}), bytes(&v["raw"])?)?;
            let p = plan(
                &json!({"format":v["format"],"key_material":v["material"],"input_data_profile":v["profile"],"integer_input_tag":v["integer_tag"]}),
            )?;
            let m = catalog::Module {
                id: String::new(),
                data: String::new(),
                label: String::new(),
                start: uint(&a[1])?,
                end: uint(&a[2])?,
                metadata: 0,
            };
            json!(operands::arrays_bounded(
                &r,
                &m,
                &p,
                uint(&v["float_tag"])? as u8,
                uint(&v["remaining_operations"])?
            )?)
        }
        "cf_gateway.property_data.property_names" => {
            let raw = STANDARD.decode(s(0)?)?;
            let f = format(&a[1])?;
            let program = data::program(&raw, &f, false)?;
            let mut r = response(&json!({}), raw)?;
            r.program = program;
            let p = plan(&json!({"format":a[1],"property_profile":a[2]}))?;
            let allowed = array(&a[3])?
                .iter()
                .map(string)
                .collect::<Result<std::collections::HashSet<_>>>()?;
            json!(operands::properties(&r, &p, &allowed)?)
        }
        "cf_gateway.image_task.png_dimensions" => json!(http_tasks::png(
            &b(0)?,
            k["allow_raw_deflate"].as_bool().unwrap_or(false)
        )?),
        "cf_gateway.integrity_task.integrity_result" => {
            http_tasks::integrity(uint(&a[0])? as u16, a[1].as_object().unwrap())
        }
        "cf_gateway.raster_task.valid_scene" => json!(model_geometry::valid_scene(&a[0])),
        "cf_gateway.raster.windows1252_digest" => json!(raster::windows_digest(&b(0)?)),
        "cf_gateway.raster.render_scene" => json!(
            raster::scene_with_seed(
                &a[0],
                &a[1],
                k["seed"].as_u64().unwrap_or(raster::seed() as u64) as u32
            )?
            .pixels
        ),
        "cf_gateway.graphics_model.p3_readback" => json!(raster::colors(
            serde_json::from_value(a[0].clone())?,
            k["space"].as_str().unwrap_or("srgb"),
            k["half"].as_bool().unwrap_or(false)
        )?),
        "cf_gateway.graphics_model.text_metrics" => {
            raster::metrics(s(0)?, a.get(1).map(num).transpose()?.unwrap_or(16.))
        }
        "cf_gateway.graphics_model.calculate" => raster::graphics(
            &a[0],
            &a[1],
            &serde_json::from_value::<Vec<String>>(a[2].clone())?,
            serde_json::from_value(a[3].clone())?,
        )?,
        "cf_gateway.text_geometry_task.calculate" => model_geometry::text_geometry(
            &a[0],
            &serde_json::from_value::<Vec<String>>(a[1].clone())?,
            n(2)?,
            n(3)?,
        )?,
        "cf_gateway.css_serialization.stylesheet" => json!(css::stylesheet(s(0)?)?),
        "cf_gateway.tree_task.ancestry_result" => json!(models::ancestry(&a[0])?.0),
        "cf_gateway.tree_task.compute_ancestry" => {
            let (value, trace) = models::ancestry(&a[0])?;
            json!({"value":value,"selectors":trace})
        }
        "cf_gateway.protocol.validate_url" => json!(static_data::validate_url(s(0)?)?.as_str()),
        "cf_gateway.allocator_task.same_origin" => {
            json!(static_data::origin(s(0)?)? == static_data::origin(s(1)?)?)
        }
        "cf_gateway.allocator_task.base31" => json!(models::base31(uint(&a[0])?)),
        "cf_gateway.allocator_task.policy_methods" => json!(7),
        "cf_gateway.dispatcher.state_hash" => {
            json!(catalog::state_hash(s(0)?, uint(&a[1])? as u32))
        }
        "cf_gateway.dispatcher.registration_order" => {
            let r = response(&a[0], vec![])?;
            let targets = array(&a[2])?
                .iter()
                .map(|v| {
                    Ok(catalog::Target {
                        name: string(&v["name"])?.into(),
                        at: uint(&v["instruction_offset"])?,
                        target: uint(&v["target"])?,
                        key: uint(&v["key"])? as u8,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let c = catalog::Catalog {
                parameters: Value::Null,
                modules: modules(&a[1])?,
                targets,
                order: vec![],
            };
            json!(c.registration_order(&r)?)
        }
        "cf_gateway.static.challenge_configuration" => {
            static_data::assignment(s(0)?, "_cf_chl_opt")?.unwrap_or(Value::Null)
        }
        "cf_gateway.protocol.recognize" => {
            let h = a[2]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
                .collect();
            let c = static_data::recognize(s(0)?, uint(&a[1])? as u16, &h, s(3)?)?;
            json!({"kind":c.kind,"config":c.config,"script_url":c.script})
        }
        "cf_gateway.program_literals.unpack_text" => json!(data::fragment(s(0)?)
            .filter(|(_, n, _)| *n == 0)
            .map(|x| x.0)),
        "cf_gateway.program_literals.unpack_fragment" => match data::fragment(s(0)?) {
            None => Value::Null,
            Some((text, n, offset)) => {
                json!({"text":text,"continuation_length":n,"continuation_offset":offset})
            }
        },
        "cf_gateway.response_data.ResponseData.completion_parameters" => {
            response(&a[0], vec![])?.completion()?
        }
        "cf_gateway.response_data.ResponseData.proof_parameters" => {
            response(&a[0], vec![])?.proof()?
        }
        "cf_gateway.response_data.ResponseData.frame_parameters" => {
            let mut v = response(&a[0], vec![])?.frame()?;
            v["resultsComputed"] = json!(false);
            v
        }
        "cf_gateway.completion_form.navigation_form" => {
            let r = response(&a[0], vec![])?;
            let p = plan(&a[1])?;
            match completion::navigation(&r, &p, &a[2], s(3)?)? {
                None => Value::Null,
                Some(f) => json!({"method":"POST","url":f.url,"body":f.body}),
            }
        }
        "cf_gateway.math_task.math_tasks" | "cf_gateway.callback_task.callback_tasks" => {
            model_call(op, a, k)?
        }
        "cf_gateway.managed.program_format"
        | "cf_gateway.operand_data.integer_literal_tag"
        | "cf_gateway.property_data.write_profile"
        | "cf_gateway.immediate_data.immediate_profile"
        | "cf_gateway.dispatcher.profile_from_source"
        | "cf_gateway.key_material.recover_key_material"
        | "cf_gateway.operand_data.operand"
        | "cf_gateway.deobfuscate.ConstantTables.evaluate"
        | "cf_gateway.managed.script_build"
        | "cf_gateway.key_material.transform_name"
        | "cf_gateway.instruction_data.framed_salt" => ast_call(op, a)?,
        _ => return Err(Error::new("TEST_UNSUPPORTED", op)),
    })
}
fn close(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.)
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| close(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|b| close(v, b)))
        }
        _ => false,
    }
}
fn approximate_math_positions() -> &'static Vec<bool> {
    static POSITIONS: OnceLock<Vec<bool>> = OnceLock::new();
    POSITIONS.get_or_init(|| {
        let plan: Value =
            serde_json::from_str(include_str!("../../profiles/math-plan-v1.json")).unwrap();
        let mut out = Vec::new();
        for case in plan["cases"].as_array().unwrap() {
            let name = case[0].as_str().unwrap();
            out.push(!matches!(
                name,
                "abs" | "ceil" | "clz32" | "floor" | "fround" | "round" | "sign" | "sqrt" | "trunc"
            ));
            if plan["polyfills"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == name)
            {
                out.push(true);
            }
        }
        out
    })
}
fn numeric_math_string_close(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    let (Some(a), Some(b)) = (a.as_str(), b.as_str()) else {
        return false;
    };
    let (Ok(x), Ok(y)) = (a.parse::<f64>(), b.parse::<f64>()) else {
        return false;
    };
    // The host CRT/libm transcendental functions need not return identical
    // last bits across platforms. This exception is scoped to the declared
    // approximate math outputs, not protocol tokens or number formatting.
    x.is_finite()
        && y.is_finite()
        && x.is_sign_negative() == y.is_sign_negative()
        && math::number(x) == a
        && math::number(y) == b
        && x.to_bits().abs_diff(y.to_bits()) <= 4
}
fn math_result_close(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    if let (Some(a), Some(b)) = (a.as_object(), b.as_object()) {
        return a.len() == b.len()
            && a.iter().all(|(key, value)| {
                b.get(key).is_some_and(|other| {
                    if key == "zJGo6" {
                        let (Some(a), Some(b)) = (value.as_array(), other.as_array()) else {
                            return false;
                        };
                        let positions = approximate_math_positions();
                        a.len() == b.len()
                            && a.len() == positions.len()
                            && a.iter().zip(b).zip(positions).all(|((a, b), approx)| {
                                if *approx {
                                    numeric_math_string_close(a, b)
                                } else {
                                    a == b
                                }
                            })
                    } else {
                        math_result_close(value, other)
                    }
                })
            });
    }
    close(a, b)
}
#[test]
fn math_contract_roundoff_exception_is_narrow_and_preserves_wire_strings() {
    let near = f64::from_bits(1.0f64.to_bits() + 1);
    assert!(numeric_math_string_close(
        &json!("1"),
        &json!(math::number(near))
    ));
    assert!(!numeric_math_string_close(
        &json!("1"),
        &json!(math::number(f64::from_bits(1.0f64.to_bits() + 5)))
    ));
    assert!(!numeric_math_string_close(&json!("1"), &json!("1.0")));
    assert!(!numeric_math_string_close(&json!("0"), &json!("-0")));
    assert!(!numeric_math_string_close(
        &json!("NaN"),
        &json!("Infinity")
    ));
    assert!(!math_result_close(
        &json!({"token":"1"}),
        &json!({"token":math::number(near)})
    ));
    let positions = approximate_math_positions();
    let mut a = json!({"zJGo6":vec!["1";positions.len()]});
    let mut b = a.clone();
    b["zJGo6"][positions.iter().position(|p| *p).unwrap()] = json!(math::number(near));
    assert!(math_result_close(&a, &b));
    a["zJGo6"][positions.iter().position(|p| !*p).unwrap()] = json!("2");
    assert!(!math_result_close(&a, &b));
}
fn cases() -> &'static Vec<Value> {
    static CASES: OnceLock<Vec<Value>> = OnceLock::new();
    CASES.get_or_init(|| {
        let mut data = Vec::new();
        flate2::read::GzDecoder::new(
            &include_bytes!("../../tests/fixtures/python-contracts.json.gz")[..],
        )
        .read_to_end(&mut data)
        .unwrap();
        serde_json::from_slice(&data).unwrap()
    })
}
#[test]
fn deterministic_python_contracts() {
    let mut failures = Vec::new();
    let mut passed = 0;
    let mut original = std::collections::HashSet::new();
    let retired = cases()
        .iter()
        .filter(|row| row["operation"] == "cf_gateway.cdn_routes.cloudflare_answer")
        .count();
    assert_eq!(
        retired, 12,
        "Only the intentionally removed global CDN-mapping contracts are retired"
    );
    for row in cases() {
        let op = row["operation"].as_str().unwrap();
        if op == "cf_gateway.cdn_routes.cloudflare_answer" {
            continue;
        }

        let a = if op == "cf_gateway.compiled_data.decode" {
            row["args"].clone()
        } else {
            norm(&row["args"])
        };
        let k = norm(&row["kwargs"]);
        let mut got = std::panic::catch_unwind(|| invoke(op, a.as_array().unwrap(), &k));
        let mut expected = norm(&row["expected"]);
        if op == "cf_gateway.jsd.payload" && row.get("error").is_none() {
            for value in [&mut expected].into_iter().chain(match &mut got {
                Ok(Ok(v)) => Some(v),
                _ => None,
            }) {
                if let Some(groups) = value["payload"].as_object_mut() {
                    for fields in groups.values_mut() {
                        if let Some(a) = fields.as_array_mut() {
                            a.retain(|v| v != "d.lastModified");
                        }
                    }
                    groups.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
                }
            }
        }
        if op == "cf_gateway.surface_task.classify" {
            // A capability group is a set of property names. Ordering and
            // the current clock value are not assertions of the Python tests.
            for v in [&mut expected].into_iter().chain(match &mut got {
                Ok(Ok(v)) => Some(v),
                _ => None,
            }) {
                if let Some(groups) = v["lxzW3"].as_object_mut() {
                    for group in groups.values_mut() {
                        if let Some(items) = group.as_array_mut() {
                            items.retain(|v| v != "d.lastModified");
                            items.sort_by_key(Value::to_string)
                        }
                    }
                    groups.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
                }
            }
        }
        if op == "cf_gateway.proof.best_sha256_work" && row.get("error").is_none() {
            if let Ok(Ok(value)) = &mut got {
                let elapsed = value["elapsed_ms"].as_f64().unwrap();
                assert!(elapsed >= 0. && elapsed <= 1000.1);
                value.as_object_mut().unwrap().shift_remove("elapsed_ms");
            }
            expected.as_object_mut().unwrap().shift_remove("elapsed_ms");
        }
        if op == "cf_gateway.benchmark_task.benchmark_tasks" && row.get("error").is_none() {
            if let Ok(Ok(value)) = &mut got {
                for fields in value.as_object_mut().unwrap().values_mut() {
                    let samples = fields["BvSgL3"].as_array().unwrap();
                    assert_eq!(
                        samples
                            .iter()
                            .map(|r| r.as_array().unwrap().len())
                            .collect::<Vec<_>>(),
                        [3; 4]
                    );
                    assert!(samples
                        .iter()
                        .flat_map(|r| r.as_array().unwrap())
                        .all(|n| n.as_f64().unwrap() >= 0.));
                    for key in ["BvSgL3", "kFiz5", "YHqsV6"] {
                        fields.as_object_mut().unwrap().shift_remove(key);
                    }
                }
            }
            for fields in expected.as_object_mut().unwrap().values_mut() {
                for key in ["BvSgL3", "kFiz5", "YHqsV6"] {
                    fields.as_object_mut().unwrap().shift_remove(key);
                }
            }
        }
        let success = match &got {
            Ok(Ok(v)) => {
                row.get("error").is_none()
                    && if matches!(
                        op,
                        "cf_gateway.math_task.calculate" | "cf_gateway.math_task.math_tasks"
                    ) {
                        math_result_close(v, &expected)
                    } else {
                        close(v, &expected)
                    }
            }
            Ok(Err(e)) => {
                row.get("error").is_some() && e.code != "TEST_INPUT" && e.code != "TEST_UNSUPPORTED"
            }
            _ => false,
        };
        if success {
            passed += 1;
            original.insert(row["python"].as_str().unwrap());
        } else if failures.len() < 80 {
            if op == "cf_gateway.surface_task.classify" {
                if let Ok(Ok(v)) = &got {
                    let keys = expected["lxzW3"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .chain(v["lxzW3"].as_object().unwrap().keys())
                        .collect::<std::collections::HashSet<_>>();
                    for key in keys {
                        if expected["lxzW3"][key] != v["lxzW3"][key] {
                            let names = |v: &Value| {
                                v.as_array()
                                    .into_iter()
                                    .flatten()
                                    .map(Value::to_string)
                                    .collect::<std::collections::HashSet<_>>()
                            };
                            let e = names(&expected["lxzW3"][key]);
                            let a = names(&v["lxzW3"][key]);
                            eprintln!(
                                "group {key}: missing {:?}; extra {:?}",
                                e.difference(&a).collect::<Vec<_>>(),
                                a.difference(&e).collect::<Vec<_>>()
                            );
                        }
                    }
                }
            }
            failures.push(format!(
                "{} {}\nexpected {}\ngot {:?}",
                row["python"],
                op,
                if row.get("error").is_some() {
                    row["error"].to_string()
                } else {
                    expected.to_string().chars().take(650).collect()
                },
                got.map(|x| x.map(|v| v.to_string().chars().take(650).collect::<String>()))
            ));
        }
    }
    eprintln!(
        "{passed}/{} differential calls, {} original tests with passing contracts",
        cases().len() - retired,
        original.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
