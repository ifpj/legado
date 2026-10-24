//! Bounded reading of protocol constant data. Does not execute VM instructions.
use crate::{array, re, require, string, uint, Error, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

#[derive(Default)]
struct LayoutCache {
    layouts: Mutex<Vec<(bool, Value)>>,
    compiling: Mutex<()>,
}
impl LayoutCache {
    fn lookup(
        &self,
        frame: bool,
        matches: &impl Fn(&Value) -> Result<bool>,
    ) -> Result<Option<Value>> {
        let layouts = crate::cpu::lock(&self.layouts)?;
        let mut found = None;
        for (_, layout) in layouts.iter().filter(|(tasks, _)| *tasks == frame) {
            crate::cpu::check()?;
            if matches(layout)? {
                if let Some(previous) = &found {
                    require(previous == layout, "Ambiguous initializer layout")?;
                } else {
                    found = Some(layout.clone());
                }
            }
        }
        Ok(found)
    }
    fn resolve(
        &self,
        frame: bool,
        matches: impl Fn(&Value) -> Result<bool>,
        compile: impl FnOnce() -> Result<Value>,
    ) -> Result<(Value, bool)> {
        if let Some(layout) = self.lookup(frame, &matches)? {
            return Ok((layout, true));
        }
        // Only cold misses serialize. Do not hold the layouts mutex while
        // compiling; warm readers remain independent and waiters can cancel.
        let _compiling = crate::cpu::lock(&self.compiling)?;
        if let Some(layout) = self.lookup(frame, &matches)? {
            return Ok((layout, true));
        }
        let layout = compile()?;
        crate::cpu::check()?;
        let mut layouts = crate::cpu::lock(&self.layouts)?;
        if !layouts
            .iter()
            .any(|(tasks, existing)| *tasks == frame && *existing == layout)
        {
            if layouts.len() >= 32 {
                layouts.remove(0);
            }
            layouts.push((frame, layout.clone()));
        }
        Ok((layout, false))
    }
}

#[cfg(test)]
mod layout_cache_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };

    #[test]
    fn concurrent_misses_compile_and_store_one_layout() {
        let cache = Arc::new(LayoutCache::default());
        let starts = Arc::new(Barrier::new(8));
        let compiled = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (cache, starts, compiled) = (cache.clone(), starts.clone(), compiled.clone());
                std::thread::spawn(move || {
                    starts.wait();
                    cache
                        .resolve(
                            true,
                            |v| Ok(v["build"] == "current"),
                            || {
                                compiled.fetch_add(1, Ordering::SeqCst);
                                std::thread::sleep(std::time::Duration::from_millis(20));
                                Ok(json!({"build":"current","shape":1}))
                            },
                        )
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(compiled.load(Ordering::SeqCst), 1);
        assert_eq!(cache.layouts.lock().unwrap().len(), 1);
        assert_eq!(results.iter().filter(|(_, reused)| !reused).count(), 1);
        assert!(results.iter().all(|(v, _)| *v == results[0].0));
    }

    #[test]
    fn identical_layouts_are_equivalent_but_conflicts_still_fail() {
        let cache = LayoutCache::default();
        let layout = json!({"build":"current","shape":1});
        *cache.layouts.lock().unwrap() = vec![
            (true, layout.clone()),
            (true, layout.clone()),
            (false, json!({"shape":2})),
        ];
        assert_eq!(cache.lookup(true, &|_| Ok(true)).unwrap(), Some(layout));
        cache
            .layouts
            .lock()
            .unwrap()
            .push((true, json!({"build":"current","shape":3})));
        assert_eq!(
            cache.lookup(true, &|_| Ok(true)).unwrap_err().message,
            "Ambiguous initializer layout"
        );
    }

    #[test]
    fn failed_compilation_does_not_poison_cache_or_block_retry() {
        let cache = LayoutCache::default();
        assert!(cache
            .resolve(
                false,
                |_| Ok(true),
                || Err(Error::new("TIMEOUT", "cancelled"))
            )
            .is_err());
        assert!(cache.layouts.lock().unwrap().is_empty());
        assert_eq!(
            cache
                .resolve(false, |_| Ok(true), || Ok(json!({"fresh":true})))
                .unwrap(),
            (json!({"fresh":true}), false)
        );
    }

    #[tokio::test]
    async fn cancelled_cold_waiter_cannot_publish_a_layout() {
        let cache = Arc::new(LayoutCache::default());
        let compiled = Arc::new(AtomicUsize::new(0));
        let owner = cache.compiling.lock().unwrap();
        let (worker_cache, worker_compiled) = (cache.clone(), compiled.clone());
        let result = crate::cpu::run(
            std::time::Instant::now() + std::time::Duration::from_millis(20),
            move || {
                worker_cache.resolve(
                    true,
                    |_| Ok(true),
                    || {
                        worker_compiled.fetch_add(1, Ordering::SeqCst);
                        Ok(json!({"cancelled":true}))
                    },
                )
            },
        )
        .await;
        assert_eq!(result.unwrap_err().code, "TIMEOUT");
        drop(owner);
        let current = cache
            .resolve(true, |_| Ok(true), || Ok(json!({"fresh":true})))
            .unwrap();
        assert_eq!(current.0, json!({"fresh":true}));
        assert_eq!(compiled.load(Ordering::SeqCst), 0);
        assert_eq!(cache.layouts.lock().unwrap().len(), 1);
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Format {
    pub byte_offset: u8,
    pub initial_key: u8,
    pub state_step: u8,
    pub state_multiplier: u32,
    pub state_increment: u32,
    pub hash_salt: u32,
    pub char_xor: u8,
    pub state_quadratic: u32,
    pub char_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opcode_multiplier: Option<u32>,
}
impl Format {
    pub fn key(&self, key: u8, opcode: u8) -> u8 {
        let n = key as u32 + opcode as u32;
        self.state_quadratic
            .wrapping_mul(n)
            .wrapping_mul(n)
            .wrapping_add(self.state_multiplier.wrapping_mul(n))
            .wrapping_add(self.state_increment) as u8
    }
    pub fn byte(&self, raw: u8, key: u8) -> u8 {
        raw.wrapping_sub(self.byte_offset) ^ key
    }
    pub fn opcode(&self, raw: u8, key: u8, position: usize) -> u8 {
        let value = self.byte(raw, key);
        if let Some(multiplier) = self.opcode_multiplier {
            (value ^ (self.seed(position) ^ (key as u32).wrapping_mul(multiplier)) as u8)
                .rotate_right((key & 7) as u32)
        } else {
            value
        }
    }
    pub fn instruction_keys(&self, opcode: u8) -> InstructionKeys {
        let mut buckets: [Vec<(u8, u8)>; 256] = std::array::from_fn(|_| Vec::new());
        for key in 0..=255u8 {
            let raw = if let Some(multiplier) = self.opcode_multiplier {
                opcode.rotate_left((key & 7) as u32)
                    ^ key
                    ^ (key as u32).wrapping_mul(multiplier) as u8
            } else {
                opcode ^ key
            };
            buckets[raw as usize].push((key, self.key(key, opcode)));
        }
        InstructionKeys { buckets }
    }
    pub fn char(&self, value: u8, seed: u32) -> u8 {
        let high = (seed >> 16) as u8;
        if self.char_mode == "before-subtract" {
            (value ^ self.char_xor ^ high).wrapping_sub(seed as u8)
        } else {
            (value ^ self.char_xor).wrapping_sub(seed as u8) ^ high
        }
    }
    pub fn seed(&self, position: usize) -> u32 {
        let mut x = position as u32 ^ self.hash_salt;
        x ^= x >> 16;
        x = x.wrapping_mul(40503);
        x ^= x >> 13;
        x = x.wrapping_mul(13849);
        x ^ (x >> 16)
    }
    pub fn keys(&self, opcode: u8) -> [u8; 256] {
        std::array::from_fn(|i| self.key(self.byte(i as u8, opcode), opcode))
    }
}
pub struct InstructionKeys {
    buckets: [Vec<(u8, u8)>; 256],
}
impl InstructionKeys {
    pub fn at(&self, format: &Format, raw: u8, position: usize) -> &[(u8, u8)] {
        let index = raw.wrapping_sub(format.byte_offset)
            ^ if format.opcode_multiplier.is_some() {
                format.seed(position) as u8
            } else {
                0
            };
        &self.buckets[index as usize]
    }
}
pub fn normalize(v: &Value) -> Result<Value> {
    fn read(v: &Value, depth: usize, schema: &Value) -> Result<Value> {
        require(depth <= 24, "Compiler data depth limit")?;
        if let Some(o) = v.as_object() {
            if o.len() == 2 && o.contains_key("type") && o.contains_key("fields") {
                let kind = string(&o["type"])?;
                let fields = o["fields"]
                    .as_object()
                    .ok_or_else(|| Error::new("INVALID_DATA", "Compiler fields"))?;
                let names = array(&schema[kind])?;
                require(
                    names.len() == fields.len()
                        && names
                            .iter()
                            .all(|n| n.as_str().is_some_and(|k| fields.contains_key(k))),
                    "Unknown compiler structure",
                )?;
                return read(&o["fields"], depth + 1, schema);
            }
            if o.len() == 1 {
                if let Some(kind) = o.get("declarationKind") {
                    require(
                        matches!(kind.as_str(), Some("N" | "o" | "u" | "p5")),
                        "Unknown declaration kind",
                    )?;
                    return Ok(v.clone());
                }
                if let Some(v) = o.get("tuple").or_else(|| o.get("set")) {
                    require(v.is_array(), "Compiler sequence")?;
                    return read(v, depth + 1, schema);
                }
                if let Some(v) = o.get("bytes") {
                    return Ok(json!(hex::decode(string(v)?)
                        .map_err(|_| Error::new("INVALID_DATA", "Compiler bytes"))?));
                }
            }
            return Ok(Value::Object(
                o.iter()
                    .map(|(k, v)| Ok((k.clone(), read(v, depth + 1, schema)?)))
                    .collect::<Result<_>>()?,
            ));
        }
        if let Some(a) = v.as_array() {
            return Ok(json!(a
                .iter()
                .map(|v| read(v, depth + 1, schema))
                .collect::<Result<Vec<_>>>()?));
        }
        Ok(v.clone())
    }
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    read(
        v,
        0,
        SCHEMA.get_or_init(|| {
            serde_json::from_str(include_str!("../profiles/compiler-schema-v1.json"))
                .expect("compiler schema")
        }),
    )
}
#[derive(Clone)]
pub struct Plan {
    pub format: Format,
    pub meta: Value,
    pub alphabet: String,
    pub modulus: String,
    pub flow: String,
}
impl Plan {
    pub fn analyze(source: &str, frame: bool) -> Result<Self> {
        let prepared = crate::literal_strings::prepare(source)?;
        let source = prepared.as_ref();
        static LAYOUTS: OnceLock<LayoutCache> = OnceLock::new();
        let layouts = LAYOUTS.get_or_init(|| {
            let data: Value =
                serde_json::from_str(include_str!("../profiles/protocol-layouts-v1.json"))
                    .expect("compiled layouts");
            LayoutCache {
                layouts: Mutex::new(
                    data["layouts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| {
                            (
                                v["frameTasks"] == true,
                                normalize(&v["plan"]).expect("compiled layout schema"),
                            )
                        })
                        .collect(),
                ),
                compiling: Mutex::new(()),
            }
        });
        let programs = re(r#"['"`]([A-Za-z0-9+/]{1000,20000}={0,2})['"`]"#);
        let hashes: HashSet<_> = programs
            .captures_iter(source)
            .filter_map(|c| STANDARD.decode(&c[1]).ok())
            .map(|v| hex::encode(Sha256::digest(v)))
            .collect();
        let mut compiled = None;
        let (mut meta, reused) = layouts.resolve(
            frame,
            |plan| {
                Ok(
                    hashes.contains(string(&plan["key_material"]["program_sha256"])?)
                        && source.contains(string(&plan["build"])?)
                        && source.contains(string(&plan["key_material"]["helper"])?)
                        && (!frame || plan["boundaryProfileVersion"] == 2),
                )
            },
            || {
                let plan = crate::compiler::compile_prepared(source, frame)?;
                let meta = plan.meta.clone();
                compiled = Some(plan);
                Ok(meta)
            },
        )?;
        if let Some(mut plan) = compiled {
            plan.meta["compiledLayoutReused"] = json!(false);
            return Ok(plan);
        }
        meta["compiledLayoutReused"] = json!(reused);
        Self::from_layout(source, meta)
    }
    pub(crate) fn from_layout(source: &str, meta: Value) -> Result<Self> {
        fn one(source: &str, pat: &str) -> Result<String> {
            let values: HashSet<String> = re(pat)
                .captures_iter(source)
                .map(|c| c[1].to_string())
                .collect();
            require(values.len() == 1, "Non-unique current wire parameters")?;
            Ok(values.into_iter().next().unwrap())
        }
        let alphabet = one(source, r#"['"`]([A-Za-z0-9+$-]{65})['"`]"#)?;
        let modulus = one(source, r#"['"`](0x[0-9a-fA-F]{258})['"`]"#)?;
        let flow = one(source, r#"['"`](/fo/[0-9]+:[0-9]+:[A-Za-z0-9_-]+/)['"`]"#)?;
        let format = serde_json::from_value(meta["format"].clone())?;
        Ok(Self {
            format,
            meta,
            alphabet,
            modulus,
            flow,
        })
    }
    pub fn literal_tag(&self) -> Result<u8> {
        Ok(uint(&self.meta["key_material"]["literal_tag"])? as u8)
    }
    pub fn kind_slot(&self) -> Result<usize> {
        uint(&self.meta["key_material"]["literal_kind_slot"])
    }
    pub fn mask(&self) -> Result<Vec<u8>> {
        Ok(array(&self.meta["key_material"]["mask"])?
            .iter()
            .map(|v| v.as_u64().unwrap_or_default() as u8)
            .collect())
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Program {
    pub sha256: String,
    pub byte_length: usize,
    pub code_length: usize,
    pub strings: Vec<String>,
}
fn read(raw: &[u8], pos: &mut usize, f: &Format, key: u8) -> Result<u8> {
    let value = *raw
        .get(*pos)
        .ok_or_else(|| Error::new("INVALID_DATA", "Truncated operand"))?;
    *pos += 1;
    Ok(f.byte(value, key))
}
fn descriptor(raw: &[u8], position: &mut usize, f: &Format, key: u8) -> Result<u32> {
    let mut result = 0;
    for shift in (0..21).step_by(7) {
        let b = read(raw, position, f, key)?;
        result |= ((b & 127) as u32) << shift;
        if b & 128 == 0 {
            return Ok(result);
        }
    }
    Err(Error::new("INVALID_DATA", "Oversize length"))
}
fn text(
    raw: &[u8],
    pos: &mut usize,
    len: usize,
    wide: bool,
    f: &Format,
    key: u8,
) -> Result<String> {
    require(
        *pos + len * (if wide { 2 } else { 1 }) <= raw.len(),
        "String bounds",
    )?;
    let mut seed = f.seed(*pos);
    let mut units = Vec::with_capacity(len);
    let mut byte = |p: &mut usize| -> Result<u8> {
        let v = f.char(read(raw, p, f, key)?, seed);
        seed = seed.wrapping_add(2654435769);
        Ok(v)
    };
    for i in 0..len {
        if i % 1024 == 0 {
            crate::cpu::check()?;
        }
        units.push(if wide {
            ((byte(pos)? as u16) << 8) | byte(pos)? as u16
        } else {
            byte(pos)? as u16
        })
    }
    String::from_utf16(&units).map_err(|_| Error::new("INVALID_DATA", "Unpaired UTF16 literal"))
}
pub fn program(raw: &[u8], f: &Format, unframed: bool) -> Result<Program> {
    crate::cpu::check()?;
    require(
        !raw.is_empty() && raw.len() <= 1024 * 1024,
        "Program bounds",
    )?;
    let trailer = [183, 76, 57, 162];
    if raw.len() < 8
        || raw[raw.len() - 4..]
            .iter()
            .map(|b| f.byte(*b, f.initial_key))
            .ne(trailer)
    {
        require(
            unframed && raw.len() <= 512 * 1024,
            "Unknown constant-table trailer",
        )?;
        return Ok(Program {
            sha256: hex::encode(Sha256::digest(raw)),
            byte_length: raw.len(),
            code_length: raw.len(),
            strings: vec![],
        });
    }
    let end = raw.len() - 7;
    let size = raw[end..end + 3]
        .iter()
        .fold(0usize, |a, b| (a << 8) | f.byte(*b, f.initial_key) as usize);
    require(size > 0 && size <= end, "Constant pool bounds")?;
    let start = end - size;
    let mut pos = start;
    let count = f.byte(raw[pos], f.initial_key) as usize;
    pos += 1;
    require(count > 0, "Empty pool")?;
    let mut key = f.initial_key;
    let mut strings = Vec::new();
    for _ in 0..count {
        crate::cpu::check()?;
        key = f.key(key, f.state_step);
        let d = descriptor(raw, &mut pos, f, key)?;
        strings.push(text(
            raw,
            &mut pos,
            (d & 65535) as usize,
            d & 65536 != 0,
            f,
            key,
        )?)
    }
    require(pos == end, "Constant pool trailing data")?;
    Ok(Program {
        sha256: hex::encode(Sha256::digest(raw)),
        byte_length: raw.len(),
        code_length: start,
        strings,
    })
}
#[derive(Clone, Debug, Serialize)]
pub struct Literal {
    #[serde(skip)]
    pub key: Option<u8>,
    pub start: usize,
    pub end: usize,
    pub value: String,
    pub packed: Option<String>,
    pub continuation: usize,
    pub pool_offset: usize,
}
impl Literal {
    pub fn instruction_key(&self, raw: &[u8], format: &Format) -> u8 {
        self.key.unwrap_or_else(|| {
            format.key(
                format.byte(raw[self.start], format.state_step),
                format.state_step,
            )
        })
    }
}
pub(crate) fn fragment(value: &str) -> Option<(String, usize, usize)> {
    let chars: Vec<_> = value.chars().map(|c| c as u32).collect();
    if chars.len() < 2 || chars[0] > 255 || chars[1] > 255 {
        return None;
    }
    let mut size = (chars[0] * 256 + chars[1]) as usize;
    let (mut start, mut cont, mut offset) = (2, 0, 0);
    if size == 65535 {
        if chars.len() < 12 || chars[..12].iter().any(|c| *c > 255) {
            return None;
        }
        size = (chars[2] * 256 + chars[3]) as usize;
        offset = chars[4..8]
            .iter()
            .fold(0usize, |a, b| (a << 8) | *b as usize);
        cont = chars[8..12]
            .iter()
            .fold(0usize, |a, b| (a << 8) | *b as usize);
        if cont == 0 || offset + cont > 4 * 1024 * 1024 {
            return None;
        }
        start = 12
    }
    if start + size > chars.len() || chars[start + size..].iter().any(|c| *c != 0) {
        return None;
    }
    Some((value.chars().skip(start).take(size).collect(), cont, offset))
}
pub fn literals(
    raw: &[u8],
    p: &Program,
    plan: &Plan,
    overlaps: bool,
    max_units: usize,
    binary: bool,
) -> Result<Vec<Literal>> {
    literals_bounded(raw, p, plan, overlaps, max_units, binary, 8 * 1024 * 1024)
}
pub(crate) fn literals_bounded(
    raw: &[u8],
    p: &Program,
    plan: &Plan,
    overlaps: bool,
    max_units: usize,
    binary: bool,
    max_work: usize,
) -> Result<Vec<Literal>> {
    literals_excluding(raw, p, plan, overlaps, max_units, binary, max_work, &[])
}
fn literals_excluding(
    raw: &[u8],
    p: &Program,
    plan: &Plan,
    overlaps: bool,
    max_units: usize,
    binary: bool,
    max_work: usize,
    excluded: &[(usize, usize)],
) -> Result<Vec<Literal>> {
    let f = &plan.format;
    let tag = plan.literal_tag()?;
    let slot = plan.kind_slot()?;
    require(
        (slot == 1 || slot == 2)
            && p.code_length <= 512 * 1024
            && p.code_length <= raw.len()
            && max_units <= 65535,
        "Literal scan bounds",
    )?;
    let keys = f.instruction_keys(f.state_step);
    let (mut result, mut occupied, mut occupied_frame, mut work) =
        (Vec::<Literal>::new(), 0, false, 0usize);
    let mut excluded_index = 0;
    for start in 0..p.code_length.saturating_sub(4) {
        if start % 256 == 0 {
            crate::cpu::check()?;
        }
        while excluded_index < excluded.len() && excluded[excluded_index].1 <= start {
            excluded_index += 1;
        }
        if excluded
            .get(excluded_index)
            .is_some_and(|(a, _)| *a <= start)
        {
            continue;
        }
        let mut candidates = Vec::new();
        for &(_, key) in keys.at(f, raw[start], start) {
            if f.byte(raw[start + slot], key) != tag {
                continue;
            }
            let overlap = !overlaps && start < occupied;
            if overlap && occupied_frame {
                continue;
            }
            let candidate = (|| -> Result<Literal> {
                let mut pos = start + 3;
                if f.byte(raw[pos], key) == 127 {
                    require(!overlap, "Overlapping pool index")?;
                    pos += 1;
                    require(pos < p.code_length, "Pool index bounds")?;
                    let index = f.byte(raw[pos], key) ^ f.char_xor;
                    pos += 1;
                    return Ok(Literal {
                        key: Some(key),
                        start,
                        end: pos,
                        value: p
                            .strings
                            .get(index as usize)
                            .ok_or_else(|| Error::new("INVALID_DATA", "Pool index"))?
                            .clone(),
                        packed: None,
                        continuation: 0,
                        pool_offset: 0,
                    });
                }
                let desc = descriptor(raw, &mut pos, f, key)?;
                let size = (desc & 65535) as usize;
                let wide = desc & 65536 != 0;
                require(
                    desc <= 131071
                        && size <= max_units
                        && pos + size * (if wide { 2 } else { 1 }) <= p.code_length,
                    "Literal bounds",
                )?;
                if overlap {
                    require(size >= 2, "Overlap prefix")?;
                    let mut pp = pos;
                    let prefix = text(raw, &mut pp, 2, wide, f, key)?;
                    let prefix: Vec<_> = prefix.chars().map(|c| c as usize).collect();
                    require(
                        prefix.len() == 2
                            && prefix[0] <= 255
                            && prefix[1] <= 255
                            && (prefix[0] * 256 + prefix[1] == 65535
                                || prefix[0] * 256 + prefix[1] <= size - 2),
                        "Overlap frame length",
                    )?
                }
                work += size * (if wide { 2 } else { 1 });
                require(work <= max_work, "Literal operation budget")?;
                let value = text(raw, &mut pos, size, wide, f, key)?;
                let frag = fragment(&value);
                require(!overlap || frag.is_some(), "Unframed overlap")?;
                require(
                    binary
                        || frag.is_some()
                        || value.chars().all(|c| {
                            c == '\n' || c == '\r' || c == '\t' || (' '..='~').contains(&c)
                        })
                        || (value.chars().count() <= 256
                            && (value.starts_with("\u{1f600}a")
                                || value.starts_with("\u{1f1fa}\u{1f1f8}"))),
                    "Nontext candidate",
                )?;
                Ok(Literal {
                    key: Some(key),
                    start,
                    end: pos,
                    value,
                    packed: frag.as_ref().map(|x| x.0.clone()),
                    continuation: frag.as_ref().map_or(0, |x| x.1),
                    pool_offset: frag.as_ref().map_or(0, |x| x.2),
                })
            })();
            crate::cpu::check()?;
            if work > max_work {
                return Err(Error::new("INVALID_DATA", "Literal operation budget"));
            }
            if let Ok(literal) = candidate {
                candidates.push(literal);
            }
        }
        if candidates.len() == 1 {
            let literal = candidates.pop().unwrap();
            let overlap = !overlaps && start < occupied;
            if overlap {
                result.pop();
            }
            occupied = literal.end;
            occupied_frame = literal.packed.is_some();
            result.push(literal)
        }
    }
    Ok(result)
}
pub(crate) fn packed_texts(all: &[Literal]) -> Result<Vec<(usize, String)>> {
    let extensions: Vec<_> = all.iter().filter(|l| l.continuation > 0).collect();
    let needed = extensions
        .iter()
        .map(|l| l.pool_offset + l.continuation)
        .max()
        .unwrap_or(0);
    let pools: HashSet<_> = all
        .iter()
        .filter(|l| {
            needed > 0
                && l.continuation == 0
                && l.value.chars().count() == 32770
                && l.packed.as_ref().is_some_and(|p| p.len() >= needed)
        })
        .filter_map(|l| l.packed.clone())
        .collect();
    require(
        needed == 0 || pools.len() == 1,
        "Missing/ambiguous continuation pool",
    )?;
    let pool = pools.into_iter().next().unwrap_or_default();
    let mut packed = Vec::new();
    for l in all {
        crate::cpu::check()?;
        if let Some(v) = &l.packed {
            if needed > 0 && l.continuation == 0 && l.value.chars().count() == 32770 {
                continue;
            }
            let mut v = v.clone();
            if l.continuation > 0 {
                require(
                    pool.is_ascii() && l.pool_offset + l.continuation <= pool.len(),
                    "Continuation bounds",
                )?;
                v.push_str(&pool[l.pool_offset..l.pool_offset + l.continuation])
            }
            packed.push((l.start, v))
        }
    }
    Ok(packed)
}
/// Check ordering once per version of the list. Mutation invalidates the check,
/// including edits through indexing; legacy unsorted inputs retain their order.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(transparent)]
pub struct LiteralList {
    values: Vec<Literal>,
    #[serde(skip)]
    sorted: OnceLock<bool>,
}
impl From<Vec<Literal>> for LiteralList {
    fn from(values: Vec<Literal>) -> Self {
        Self {
            values,
            sorted: OnceLock::new(),
        }
    }
}
impl FromIterator<Literal> for LiteralList {
    fn from_iter<T: IntoIterator<Item = Literal>>(values: T) -> Self {
        Vec::from_iter(values).into()
    }
}
impl std::ops::Deref for LiteralList {
    type Target = Vec<Literal>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
impl std::ops::DerefMut for LiteralList {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.sorted.take();
        &mut self.values
    }
}
impl<'a> IntoIterator for &'a LiteralList {
    type Item = &'a Literal;
    type IntoIter = std::slice::Iter<'a, Literal>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}
#[cfg(test)]
fn recover_literals(
    raw: &[u8],
    program: &Program,
    plan: &Plan,
    all: &mut Vec<Literal>,
) -> Result<()> {
    recover_literals_excluding(raw, program, plan, all, &[])
}
fn recover_literals_excluding(
    raw: &[u8],
    program: &Program,
    plan: &Plan,
    all: &mut Vec<Literal>,
    excluded: &[(usize, usize)],
) -> Result<()> {
    let raw = &raw[..program.code_length];
    let protected: Vec<_> = all
        .iter()
        .filter(|l| l.packed.is_some() || l.value.len() >= 8)
        .map(|l| (l.start, l.end))
        .collect();
    let seen: HashSet<_> = all.iter().map(|l| (l.start, l.value.clone())).collect();
    let mut budget = 8 * 1024 * 1024usize;
    for l in literals_excluding(
        raw,
        program,
        plan,
        true,
        64,
        false,
        8 * 1024 * 1024,
        excluded,
    )? {
        crate::cpu::check()?;
        if l.packed.is_some()
            || seen.contains(&(l.start, l.value.clone()))
            || protected.iter().any(|(a, b)| *a < l.start && l.start < *b)
        {
            continue;
        }
        let identifier = re(r"^[A-Za-z0-9_$]{4,64}$").is_match(&l.value);
        let authenticated = if !identifier && !l.value.is_empty() {
            literal_call_anchor(raw, plan, &l, &mut budget)?
        } else {
            false
        };
        if identifier || authenticated {
            if authenticated {
                // A short pool-reference false positive can hide an inline
                // string. The following call authenticates the latter's key
                // and exact endpoint; do not keep its overlapping competitor.
                all.retain(|old| !(old.start < l.start && l.start < old.end));
            }
            all.push(l);
        }
    }
    Ok(())
}
fn literal_call_anchor(
    raw: &[u8],
    plan: &Plan,
    literal: &Literal,
    budget: &mut usize,
) -> Result<bool> {
    let Some(entry) = plan.meta["input_data_profile"].get("flow-call") else {
        return Ok(false);
    };
    let at = literal.end;
    if literal.start >= at || at + 8 > raw.len() {
        return Ok(false);
    }
    let f = &plan.format;
    let previous = literal.instruction_key(raw, f);
    let opcode = uint(&entry[0])? as u8;
    if f.opcode(raw[at], previous, at) != opcode {
        return Ok(false);
    }
    let key = f.key(previous, opcode);
    let len = ((f.byte(raw[at + 1], key) as usize) << 16)
        | ((f.byte(raw[at + 2], key) as usize) << 8)
        | f.byte(raw[at + 3], key) as usize;
    if !(4..=65536).contains(&len) || at + 8 + len > raw.len() {
        return Ok(false);
    }
    require(len <= *budget, "Literal recovery operation budget")?;
    *budget -= len;
    Ok(framed_end(raw, at, key, opcode, f, uint(&entry[2]["salt"])? as u32).is_some())
}

#[cfg(test)]
#[path = "data/recovery_tests.rs"]
mod recovery_tests;

fn response_literals(raw: &[u8], program: &Program, plan: &Plan) -> Result<Vec<Literal>> {
    let checked = checked_instruction_spans(&raw[..program.code_length], plan)?;
    let mut all = literals_excluding(
        raw,
        program,
        plan,
        false,
        65535,
        false,
        8 * 1024 * 1024,
        &checked,
    )?;
    recover_literals_excluding(raw, program, plan, &mut all, &checked)?;
    all.sort_by_key(|l| l.start);
    let conflicts =
        conflicting_literals_with_spans(&raw[..program.code_length], &all, plan, &checked)?;
    all.retain(|l| !conflicts.contains(&l.start));
    Ok(all)
}

#[derive(Clone)]
pub struct Response {
    pub raw: Vec<u8>,
    pub program: Program,
    pub literals: LiteralList,
    pub packed: Vec<(usize, String)>,
}
impl Response {
    pub fn read(encoded: &str, plan: &Plan, unframed: bool) -> Result<Self> {
        crate::cpu::check()?;
        let raw = STANDARD.decode(encoded)?;
        let program = program(&raw, &plan.format, unframed)?;
        let all = response_literals(&raw, &program, plan)?;
        let packed = packed_texts(&all)?;
        Ok(Self {
            raw,
            program,
            literals: all.into(),
            packed,
        })
    }
    pub fn values(&self) -> Vec<&str> {
        self.packed.iter().map(|(_, s)| s.as_str()).collect()
    }
    pub fn one(&self, pattern: &str) -> Result<String> {
        one(self
            .values()
            .into_iter()
            .filter(|v| re(pattern).is_match(v)))
    }
    pub fn between(&self, start: usize, end: usize) -> Vec<&Literal> {
        if start >= end {
            return Vec::new();
        }
        if !self
            .literals
            .sorted
            .get_or_init(|| self.literals.windows(2).all(|w| w[0].start <= w[1].start))
        {
            return self
                .literals
                .iter()
                .filter(|l| start <= l.start && l.start < end)
                .collect();
        }
        let lo = self.literals.partition_point(|l| l.start < start);
        let hi = self.literals.partition_point(|l| l.start < end);
        self.literals[lo..hi].iter().collect()
    }
    pub fn root(&self) -> Result<Value> {
        let values = self.values();
        let tokens: HashSet<_> = values
            .iter()
            .filter(|v| token(v) && v.contains("-1.3.1.1-"))
            .copied()
            .collect();
        let ext: HashSet<_> = self
            .literals
            .iter()
            .filter(|l| l.continuation > 0)
            .map(|l| l.start)
            .collect();
        let state = one(self
            .packed
            .iter()
            .filter(|(p, _)| ext.contains(p))
            .map(|(_, v)| v.as_str()))?;
        let repeated = one(tokens
            .iter()
            .filter(|v| **v != state && values.iter().filter(|x| **x == **v).count() > 1)
            .copied())?;
        Ok(
            json!({"sitekey":self.one(r"^0x4[A-Za-z0-9_-]{15,40}$")?,"flow":self.one(FLOW)?,"hmwT6":self.one(TASK)?,"nuvy8":json_input(&values)?,"nidg0":self.one(NONCE)?,"yVlbA2":state,"vaZBm3":repeated,"pageData":one(tokens.into_iter().filter(|v|*v!=state&&*v!=repeated))?}),
        )
    }
}
pub const FLOW: &str = r"^/fo/[0-9]+:[0-9]+:[A-Za-z0-9_-]+/[a-fA-F0-9]{16,32}/[A-Za-z0-9_.-]+$";
#[cfg(test)]
mod range_tests {
    use super::*;
    #[test]
    fn sorted_ranges_match_full_scan_at_duplicates_and_boundaries() {
        let positions = [0, 0, 1, 3, 3, 4, 9, 100, usize::MAX];
        let mut response = Response {
            raw: vec![],
            program: Program {
                sha256: String::new(),
                byte_length: 0,
                code_length: 0,
                strings: vec![],
            },
            literals: positions
                .iter()
                .enumerate()
                .map(|(i, &start)| Literal {
                    start,
                    end: start,
                    key: None,
                    value: i.to_string(),
                    packed: None,
                    continuation: 0,
                    pool_offset: 0,
                })
                .collect(),
            packed: positions
                .iter()
                .enumerate()
                .map(|(i, &p)| (p, i.to_string()))
                .collect(),
        };
        let bounds = [0, 1, 2, 3, 4, 5, 9, 10, 99, 100, 101, usize::MAX];
        for start in bounds {
            for end in bounds {
                let expected: Vec<_> = response
                    .literals
                    .iter()
                    .filter(|l| start <= l.start && l.start < end)
                    .map(|l| &l.value)
                    .collect();
                assert_eq!(
                    response
                        .between(start, end)
                        .iter()
                        .map(|l| &l.value)
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        }
        // Reordering and element edits must invalidate the cached ordering flag.
        response.literals.reverse();
        response.literals[0].start = 3;
        for start in bounds {
            for end in bounds {
                let expected: Vec<_> = response
                    .literals
                    .iter()
                    .filter(|l| start <= l.start && l.start < end)
                    .map(|l| &l.value)
                    .collect();
                assert_eq!(
                    response
                        .between(start, end)
                        .iter()
                        .map(|l| &l.value)
                        .collect::<Vec<_>>(),
                    expected
                );
            }
        }
        response.literals.sort_by_key(|l| l.start);
        assert_eq!(response.between(3, 4).len(), 3);
        response.literals.clear();
        response.packed.clear();
        assert!(response.between(0, usize::MAX).is_empty());
    }
}
pub const TASK: &str = r"^[A-Za-z0-9_.-]+-[0-9]{10}-1\.2\.1\.1-[A-Za-z0-9_.-]+$";
pub const NONCE: &str = r"^[A-Za-z0-9_-]{43}$";
pub fn token(v: &str) -> bool {
    re(r"^[A-Za-z0-9_.-]+-[0-9]{10}-1\.[23]\.1\.1-[A-Za-z0-9_.-]+$").is_match(v)
}
pub fn one<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<String> {
    let s: HashSet<_> = values.into_iter().collect();
    require(s.len() == 1, "Expected unique protocol operand")?;
    Ok(s.into_iter().next().unwrap().into())
}
pub fn json_input(values: &[&str]) -> Result<String> {
    let parsed: Vec<_> = values
        .iter()
        .filter_map(|v| serde_json::from_str::<String>(v).ok())
        .filter(|v| token(v))
        .collect();
    one(parsed.iter().map(String::as_str))
}

pub(crate) fn framed_end(
    raw: &[u8],
    at: usize,
    key: u8,
    opcode: u8,
    f: &Format,
    salt: u32,
) -> Option<usize> {
    if at + 8 > raw.len() {
        return None;
    }
    let byte = |i| f.byte(raw[i], key);
    let len =
        ((byte(at + 1) as usize) << 16) | ((byte(at + 2) as usize) << 8) | byte(at + 3) as usize;
    let end = at + 8 + len;
    if !(4..=65536).contains(&len) || end > raw.len() {
        return None;
    }
    let expected = u32::from_be_bytes([byte(at + 4), byte(at + 5), byte(at + 6), byte(at + 7)]);
    let mut hash = at as u32 ^ salt;
    for operand in [opcode as u32, key as u32, len as u32] {
        hash = hash.wrapping_mul(16777619) ^ operand;
    }
    hash = hash.wrapping_mul(16777619);
    for b in &raw[at + 8..end] {
        hash = (hash ^ b.wrapping_sub(f.byte_offset) as u32).wrapping_mul(16777619)
    }
    if hash == expected {
        Some(end)
    } else {
        None
    }
}
#[cfg(test)]
pub(crate) fn conflicting_literals(
    raw: &[u8],
    literals: &[Literal],
    plan: &Plan,
) -> Result<HashSet<usize>> {
    let checked = checked_instruction_spans(raw, plan)?;
    conflicting_literals_with_spans(raw, literals, plan, &checked)
}
fn checked_instruction_spans(raw: &[u8], plan: &Plan) -> Result<Vec<(usize, usize)>> {
    let Some(entry) = plan.meta["input_data_profile"].get("flow-call") else {
        return Ok(Vec::new());
    };
    let mut spans = crate::wire::checked_spans(
        raw,
        &plan.format,
        uint(&entry[0])? as u8,
        uint(&entry[2]["salt"])? as u32,
    )?;
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        if let Some(previous) = merged.last_mut().filter(|(_, end)| start <= *end) {
            previous.1 = previous.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    Ok(merged)
}
fn conflicting_literals_with_spans(
    raw: &[u8],
    literals: &[Literal],
    plan: &Plan,
    checked: &[(usize, usize)],
) -> Result<HashSet<usize>> {
    let profile = &plan.meta["input_data_profile"];
    if profile.is_null() {
        return Ok(HashSet::new());
    }
    let mut conflicts = HashSet::new();
    let f = &plan.format;
    let mut budget = 60000usize;
    let commit =
        |spans: &[(usize, usize)], branches: &HashSet<usize>, conflicts: &mut HashSet<usize>| {
            for (a, b) in spans {
                for l in literals.iter().filter(|l| {
                    *a <= l.start && l.start < *b && (l.packed.is_none() || branches.contains(a))
                }) {
                    conflicts.insert(l.start);
                }
            }
        };
    commit(checked, &HashSet::new(), &mut conflicts);
    let by_offset: HashMap<_, _> = literals.iter().map(|l| (l.start, l)).collect();
    for anchor in literals {
        crate::cpu::check()?;
        if !(anchor.start < anchor.end && anchor.end < raw.len()) {
            continue;
        }
        let mut pending = vec![(
            anchor.end,
            anchor.instruction_key(raw, f),
            Vec::new(),
            HashSet::new(),
            0,
        )];
        let mut forks = 0;
        while let Some((mut position, mut key, mut spans, mut branches, depth)) = pending.pop() {
            for step in depth..24 {
                if budget == 0 {
                    return Ok(conflicts);
                }
                budget -= 1;
                if position < anchor.end || position >= raw.len().min(anchor.end + 512) {
                    break;
                }
                let at = position;
                let opcode = f.opcode(raw[position], key, position);
                key = f.key(key, opcode);
                if opcode == f.state_step {
                    let kind_slot = plan.kind_slot()?;
                    let tags = &profile["literal-empty"][2]["tags"];
                    if position + 3 <= raw.len()
                        && tags.as_array().is_some_and(|tags| {
                            tags.iter().any(|tag| {
                                tag.as_u64() == Some(f.byte(raw[position + kind_slot], key) as u64)
                            })
                        })
                    {
                        position += 3;
                        spans.push((at, position));
                        continue;
                    }
                    if !by_offset
                        .get(&position)
                        .is_some_and(|l| l.instruction_key(raw, f) == key)
                        || position + plan.kind_slot()? >= raw.len()
                        || f.byte(raw[position + plan.kind_slot()?], key) != plan.literal_tag()?
                    {
                        break;
                    }
                    if spans.len() >= 2 {
                        commit(&spans, &branches, &mut conflicts)
                    }
                    break;
                }
                let Some((name, entry)) = profile
                    .as_object()
                    .unwrap()
                    .iter()
                    .find(|(_, p)| p[0].as_u64() == Some(opcode as u64))
                else {
                    break;
                };
                let masks = array(&entry[1])?;
                let roles = &entry[2];
                match name.as_str() {
                    "flow-property-pool" => {
                        if at + uint(&roles["width"])? > raw.len()
                            || f.byte(raw[at + 1], key) as usize != uint(&roles["marker"])?
                        {
                            break;
                        }
                        position = at + uint(&roles["width"])?;
                        spans.push((at, position));
                    }
                    "flow-closure" => {
                        if at + 8 > raw.len() {
                            break;
                        }
                        let count = f.byte(raw[at + 1 + uint(&roles["count"])?], key)
                            ^ uint(&roles["countMask"])? as u8;
                        position = at + 8 + count as usize;
                        if count as usize > uint(&roles["maxCount"])? || position > raw.len() {
                            break;
                        }
                        spans.push((at, position));
                    }
                    "flow-branch-fallthrough" => {
                        if at + 6 > raw.len() {
                            break;
                        }
                        let slots = array(&roles["target"])?;
                        let target = slots.iter().try_fold(0usize, |v, s| {
                            Ok::<_, Error>((v << 8) | f.byte(raw[at + 1 + uint(s)?], key) as usize)
                        })?;
                        if target < at + 6 || target >= raw.len() {
                            break;
                        }
                        position = at + 6;
                        spans.push((at, position));
                        if forks < 24 && target < raw.len().min(anchor.end + 512) {
                            let next = f.byte(raw[at + 1 + uint(&roles["key"])?], key)
                                ^ uint(&roles["keyMask"])? as u8;
                            pending.push((target, next, spans.clone(), branches.clone(), step + 1));
                            forks += 1;
                        }
                    }
                    "flow-branch" => {
                        if at + 9 > raw.len() {
                            break;
                        }
                        let flag = f.byte(raw[at + 1 + uint(&roles["flag"])?], key)
                            ^ uint(&roles["flagMask"])? as u8;
                        let end = at + if flag == 0 { 9 } else { 13 };
                        if end > raw.len() {
                            break;
                        }
                        let target_slots = array(&roles["target"])?;
                        let target = target_slots.iter().try_fold(0usize, |v, slot| {
                            Ok::<_, Error>(
                                (v << 8) | f.byte(raw[at + 1 + uint(slot)?], key) as usize,
                            )
                        })?;
                        if target < end || target >= raw.len() {
                            break;
                        }
                        if flag != 0 {
                            let alternate = array(&roles["alternateTarget"])?;
                            let alternate = alternate.iter().try_fold(0usize, |v, slot| {
                                Ok::<_, Error>(
                                    (v << 8) | f.byte(raw[at + 1 + uint(slot)?], key) as usize,
                                )
                            })?;
                            if alternate < end || alternate >= raw.len() {
                                break;
                            }
                            if forks < 24 && alternate < raw.len().min(anchor.end + 512) {
                                let next = f.byte(raw[at + 1 + uint(&roles["alternateKey"])?], key)
                                    ^ uint(&roles["alternateKeyMask"])? as u8;
                                let mut alternate_spans = spans.clone();
                                alternate_spans.push((at, end));
                                let mut alternate_branches = branches.clone();
                                alternate_branches.insert(at);
                                pending.push((
                                    alternate,
                                    next,
                                    alternate_spans,
                                    alternate_branches,
                                    step + 1,
                                ));
                                forks += 1;
                            }
                        } else if forks < 24 {
                            let mut alternate_spans = spans.clone();
                            alternate_spans.push((at, end));
                            let mut alternate_branches = branches.clone();
                            alternate_branches.insert(at);
                            pending.push((end, key, alternate_spans, alternate_branches, step + 1));
                            forks += 1;
                        }
                        let next = f.byte(raw[at + 1 + uint(&roles["key"])?], key)
                            ^ uint(&roles["keyMask"])? as u8;
                        position = target;
                        spans.push((at, end));
                        branches.insert(at);
                        key = next;
                    }
                    "flow-jump" => {
                        if position + 5 > raw.len() {
                            break;
                        }
                        let values: Vec<_> =
                            (0..4).map(|i| f.byte(raw[position + 1 + i], key)).collect();
                        let target = ((values[uint(&roles["b16"])?] as usize) << 16)
                            | ((values[uint(&roles["b8"])?] as usize) << 8)
                            | values[uint(&roles["b0"])?] as usize;
                        if target < position + 5 {
                            break;
                        }
                        let next = values[uint(&roles["key"])?] ^ uint(&masks[3])? as u8;
                        position = target;
                        spans.push((at, at + 5));
                        key = next;
                    }
                    "flow-varint" | "flow-varint-store" => {
                        position += 1;
                        let mut complete = false;
                        for _ in 0..uint(&roles["maxBytes"])? {
                            if position >= raw.len() {
                                break;
                            }
                            let v = f.byte(raw[position], key) ^ uint(&masks[0])? as u8;
                            position += 1;
                            if v & 128 == 0 {
                                position += uint(&roles["suffix"])?;
                                spans.push((at, position));
                                complete = true;
                                break;
                            }
                        }
                        if !complete {
                            break;
                        }
                    }
                    "flow-call" => {
                        let Some(end) =
                            framed_end(raw, position, key, opcode, f, uint(&roles["salt"])? as u32)
                        else {
                            break;
                        };
                        position = end;
                        spans.push((at, position));
                        commit(&spans, &branches, &mut conflicts)
                    }
                    _ => {
                        position += 1 + masks.len();
                        spans.push((at, position));
                    }
                }
                if literals
                    .iter()
                    .any(|l| l.packed.is_some() && at <= l.start && l.start < position.min(at + 8))
                {
                    break;
                }
            }
        }
    }
    Ok(conflicts)
}
