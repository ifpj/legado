use crate::{
    array,
    program::catalog::Module,
    program::data::{Plan, Response},
    require, uint, Error, Result,
};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

pub fn properties(
    response: &Response,
    plan: &Plan,
    allowed: &HashSet<&str>,
) -> Result<Vec<(usize, String)>> {
    let profile = &plan.meta["property_profile"];
    if profile.is_null() {
        return Ok(vec![]);
    }
    let opcode = uint(&profile[0])? as u8;
    let mask = uint(&profile[1])? as u8;
    let mut f = plan.format.clone();
    f.char_xor = mask;
    let keys = f.instruction_keys(opcode);
    let raw = &response.raw;
    let size = response.program.code_length;
    let mut choices = Vec::new();
    for prefix in 1..=3 {
        let mut plain = Vec::new();
        let mut indexed = HashMap::new();
        for at in 0..size.saturating_sub(8) {
            for &(before, key) in keys.at(&f, raw[at], at) {
                let p = at + prefix;
                let length = f.byte(raw[p], key) as usize;
                if length == 127 {
                    let index = (f.byte(raw[p + 1], key) ^ mask) as usize;
                    if let Some(name) = response.program.strings.get(index) {
                        if allowed.contains(name.as_str()) {
                            indexed.insert(at, (name.clone(), before, key));
                        }
                    }
                    continue;
                }
                if !allowed.iter().any(|n| n.len() == length) || p + 1 + length + 3 - prefix >= size
                {
                    continue;
                }
                let mut seed = f.seed(p + 1);
                let name: String = raw[p + 1..p + 1 + length]
                    .iter()
                    .map(|b| {
                        let b = f.char(f.byte(*b, key), seed);
                        seed = seed.wrapping_add(2654435769);
                        b as char
                    })
                    .collect();
                if allowed.contains(name.as_str()) {
                    plain.push((at, name))
                }
            }
        }
        let count = plain.iter().map(|(_, n)| n).collect::<HashSet<_>>().len();
        for (at, (name, _, key)) in &indexed {
            if indexed
                .get(&(at + 5))
                .is_some_and(|(n, old, _)| n == name && old == key)
            {
                plain.push((*at, name.clone()));
                plain.push((at + 5, name.clone()))
            }
        }
        plain.sort();
        plain.dedup();
        choices.push((count, prefix, plain));
    }
    choices.sort_by_key(|(n, p, _)| std::cmp::Reverse((*n, *p)));
    if choices[0].0 < 4 || choices[0].0 == choices[1].0 {
        return Ok(vec![]);
    }
    Ok(choices.remove(0).2)
}
pub fn dimensions(response: &Response, module: &Module, plan: &Plan) -> Result<(usize, usize)> {
    let profile = &plan.meta["immediate_profile"];
    require(!profile.is_null(), "Immediate format absent")?;
    let (op, slot, mask, object_slot) = (
        uint(&profile[0])? as u8,
        uint(&profile[1])?,
        uint(&profile[2])? as u8,
        uint(&profile[3])?,
    );
    let f = &plan.format;
    let raw = &response.raw;
    let mut found = HashMap::new();
    for l in response.between(module.start, module.end) {
        if !matches!(l.value.as_str(), "height" | "width") {
            continue;
        }
        let key = l.instruction_key(raw, f);
        require(
            l.start + 3 < module.end.min(raw.len()),
            "Property literal boundary",
        )?;
        let mut pos = l.start + 3;
        let first = f.byte(raw[pos], key);
        pos += 1;
        if first == 127 {
            pos += 1
        } else {
            let (mut byte, mut value, mut shift) = (first, (first & 127) as usize, 7);
            while byte & 128 != 0 {
                require(
                    shift < 21 && pos < module.end.min(raw.len()),
                    "Literal descriptor bounds",
                )?;
                byte = f.byte(raw[pos], key);
                pos += 1;
                value |= ((byte & 127) as usize) << shift;
                shift += 7;
            }
            pos += (value & 65535) * if value & 65536 != 0 { 2 } else { 1 };
        }
        if pos + 5 > module.end.min(raw.len()) || f.opcode(raw[pos], key, pos) != op {
            continue;
        }
        let next = f.key(key, op);
        require(
            !found.contains_key(&l.value),
            "Duplicate immediate dimension",
        )?;
        found.insert(
            l.value.clone(),
            (
                (f.byte(raw[pos + 1 + slot], next) ^ mask) as usize,
                f.byte(raw[pos + 1 + object_slot], next),
            ),
        );
    }
    require(
        found.len() == 2 && found["width"].1 == found["height"].1,
        "Dimensions need shared receiver",
    )?;
    Ok((found["width"].0, found["height"].0))
}
#[derive(Clone)]
enum Constant {
    Scalar(Value),
    Array(Rc<RefCell<Vec<Constant>>>),
}
impl Constant {
    fn json(&self, seen: &mut HashSet<usize>, depth: usize) -> Result<Value> {
        require(depth <= 8, "Constant nesting bound")?;
        match self {
            Self::Scalar(v) => Ok(v.clone()),
            Self::Array(a) => {
                let p = Rc::as_ptr(a) as usize;
                require(seen.insert(p), "Cyclic constant array")?;
                let out = a
                    .borrow()
                    .iter()
                    .map(|v| v.json(seen, depth + 1))
                    .collect::<Result<Vec<_>>>()?;
                seen.remove(&p);
                Ok(json!(out))
            }
        }
    }
}
pub fn arrays(
    response: &Response,
    module: &Module,
    plan: &Plan,
    float_tag: u8,
) -> Result<Vec<(usize, Value)>> {
    arrays_bounded(response, module, plan, float_tag, 60000)
}
pub(crate) fn arrays_bounded(
    response: &Response,
    module: &Module,
    plan: &Plan,
    float_tag: u8,
    limit: usize,
) -> Result<Vec<(usize, Value)>> {
    let profile = &plan.meta["input_data_profile"];
    require(!profile.is_null(), "Constant input format absent")?;
    let f = &plan.format;
    let raw = &response.raw;
    let array_op = uint(&profile["array"][0])? as u8;
    let keys = f.instruction_keys(array_op);
    let mut output = Vec::new();
    require(
        module.start < module.end && module.end <= raw.len() && (1..=200000).contains(&limit),
        "Constant input bounds",
    )?;
    let mut budget = limit;
    let masks = array(&profile["array"][1])?;
    require(masks.len() == 1, "Array declaration format")?;
    // Candidate starts are bounded by the unique literal tag and decoded
    // operations. Unknown operations terminate a declaration rather than run it.
    for start in module.start..module.end.saturating_sub(3) {
        for &(before, next) in keys.at(f, raw[start], start) {
            let destination = f.byte(raw[start + 1], next) ^ uint(&masks[0])? as u8;
            let _ = destination;
            let mut key = before;
            let mut position = start;
            let mut bindings = HashMap::<u8, Constant>::new();
            let mut declared = Vec::new();
            for _ in 0..2048 {
                if budget == 0 {
                    return Err(Error::new("INVALID_DATA", "Constant operation budget"));
                }
                budget -= 1;
                if position >= module.end.min(start + 16384) {
                    break;
                }
                let at = position;
                let opcode = f.opcode(raw[position], key, position);
                position += 1;
                let next = f.key(key, opcode);
                let decoded = (|| -> Result<()> {
                    if opcode == f.state_step {
                        require(at + 11 <= raw.len(), "Scalar bounds")?;
                        let kind = f.byte(raw[at + plan.kind_slot()?], next);
                        let small = &profile["small"];
                        let dest_slot = uint(&small[2]["destination"])?;
                        let dest = f.byte(raw[at + 3 - plan.kind_slot()?], next)
                            ^ uint(&small[1][dest_slot])? as u8;
                        let value = if kind == plan.literal_tag()? {
                            let l = response
                                .literals
                                .iter()
                                .find(|l| l.start == at)
                                .ok_or_else(|| Error::new("INVALID_DATA", "String boundary"))?;
                            position = l.end;
                            json!(l.value)
                        } else if kind == float_tag {
                            let bytes: [u8; 8] =
                                std::array::from_fn(|i| f.byte(raw[at + 3 + i], next));
                            let number = f64::from_be_bytes(bytes);
                            require(number.is_finite(), "Finite scalar required")?;
                            position = at + 11;
                            json!(number)
                        } else if Some(kind as u64) == plan.meta["integer_input_tag"].as_u64() {
                            position = at + 3;
                            let mut v = 0u64;
                            let mut shift = 0;
                            loop {
                                require(position < module.end && shift < 35, "Integer bounds")?;
                                let b = f.byte(raw[position], next);
                                position += 1;
                                v |= ((b & 127) as u64) << shift;
                                if b & 128 == 0 {
                                    break;
                                }
                                shift += 7;
                            }
                            json!(v as u32)
                        } else {
                            return Err(Error::new("INVALID_DATA", "Unknown scalar type"));
                        };
                        require(position <= module.end, "Declaration outside module")?;
                        bindings.insert(dest, Constant::Scalar(value));
                    } else {
                        let (kind, entry) = profile
                            .as_object()
                            .unwrap()
                            .iter()
                            .find(|(_, p)| p[0].as_u64() == Some(opcode as u64))
                            .ok_or_else(|| Error::new("INVALID_DATA", "End of declaration"))?;
                        require(
                            matches!(kind.as_str(), "array" | "small" | "move" | "index"),
                            "Only finite constant declarations",
                        )?;
                        let masks = array(&entry[1])?;
                        let roles = &entry[2];
                        require(position + masks.len() <= module.end, "Operand bounds")?;
                        let values: Vec<u8> = masks
                            .iter()
                            .enumerate()
                            .map(|(i, m)| {
                                f.byte(raw[position + i], next) ^ m.as_u64().unwrap_or(0) as u8
                            })
                            .collect();
                        position += masks.len();
                        let role = |name: &str| -> Result<u8> {
                            values
                                .get(uint(&roles[name])?)
                                .copied()
                                .ok_or_else(|| Error::new("INVALID_DATA", "Operand role"))
                        };
                        match kind.as_str() {
                            "array" => {
                                let a = Rc::new(RefCell::new(Vec::new()));
                                bindings.insert(role("destination")?, Constant::Array(a.clone()));
                                declared.push((at, a));
                            }
                            "small" => {
                                bindings.insert(
                                    role("destination")?,
                                    Constant::Scalar(json!(role("value")?)),
                                );
                            }
                            "move" => {
                                let v = bindings
                                    .get(&role("value")?)
                                    .ok_or_else(|| Error::new("INVALID_DATA", "Unknown alias"))?
                                    .clone();
                                bindings.insert(role("destination")?, v);
                            }
                            _ => {
                                let receiver = bindings
                                    .get(&role("receiver")?)
                                    .ok_or_else(|| Error::new("INVALID_DATA", "Missing array"))?
                                    .clone();
                                let value = bindings
                                    .get(&role("value")?)
                                    .ok_or_else(|| Error::new("INVALID_DATA", "Missing element"))?
                                    .clone();
                                let Constant::Array(receiver) = receiver else {
                                    return Err(Error::new("INVALID_DATA", "Nonarray index"));
                                };
                                let index = role("index")? as usize;
                                require(
                                    index == receiver.borrow().len() && index < 128,
                                    "Nonsequential constant index",
                                )?;
                                let mut seen = HashSet::from([Rc::as_ptr(&receiver) as usize]);
                                value.json(&mut seen, 0)?;
                                receiver.borrow_mut().push(value);
                            }
                        }
                    }
                    Ok(())
                })();
                if decoded.is_err() {
                    break;
                }
                key = next;
            }
            for (at, a) in declared {
                if a.borrow().len() >= 2 {
                    let value = Constant::Array(a).json(&mut HashSet::new(), 0)?;
                    output.push((at, value));
                }
            }
            // A failed one-operation scan is not evidence of an array. Jump over
            // an authenticated declaration to avoid quadratic overlapping reads.
            if budget < 2048 && output.is_empty() {
                break;
            }
        }
    }
    output.sort_by_key(|x| x.0);
    output.dedup_by(|a, b| a == b);
    Ok(output)
}
