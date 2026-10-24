use crate::{
    array,
    data::{self, Literal, Plan, Response},
    re, require, string, uint, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Module {
    pub id: String,
    pub data: String,
    pub label: String,
    pub start: usize,
    pub end: usize,
    pub metadata: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct Target {
    pub name: String,
    pub at: usize,
    pub target: usize,
    pub key: u8,
}
#[derive(Clone, Debug, Serialize)]
pub struct Catalog {
    pub parameters: Value,
    pub modules: Vec<Module>,
    pub targets: Vec<Target>,
    pub order: Vec<String>,
}
fn exclude_descriptor_operands(literals: &mut Vec<Literal>, targets: &[Target]) {
    // Every descriptor is authenticated against the current name and layout.
    // Its 25 operand bytes cannot independently begin a string instruction.
    // In particular random bytes can resemble a valid constant-pool reference.
    literals.retain(|literal| {
        literal.packed.is_some()
            || !targets
                .iter()
                .any(|target| target.at <= literal.start && literal.start < target.at + 26)
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_descriptor_bytes_are_not_pool_reference_instructions() {
        let literal = |start| Literal {
            key: None,
            start,
            end: start + 5,
            value: "FakeKey".into(),
            packed: None,
            continuation: 0,
            pool_offset: 0,
        };
        let mut candidates = vec![
            literal(99),
            literal(100),
            literal(123),
            literal(125),
            literal(126),
        ];
        let targets = vec![Target {
            name: "verified".into(),
            at: 100,
            target: 500,
            key: 7,
        }];
        exclude_descriptor_operands(&mut candidates, &targets);
        assert_eq!(
            candidates.iter().map(|l| l.start).collect::<Vec<_>>(),
            [99, 126]
        );
    }
    #[test]
    fn module_local_authenticated_operands_are_excluded_with_table_operands() {
        let format = data::Format {
            byte_offset: 0,
            initial_key: 0,
            state_step: 10,
            state_multiplier: 1,
            state_increment: 1,
            hash_salt: 0,
            char_xor: 0,
            state_quadratic: 0,
            char_mode: "before-subtract".into(),
            opcode_multiplier: None,
        };
        let plan = Plan {
            format,
            meta: json!({"dispatcher_profile": {
                "opcode":20,"fnv_seed":100,"prefix":[1,2,3],
                "hash_slots":[0,1,2,3],"digest_slots":[0,1,2,3],
                "target_slots":[5,6,7],"key_slot":8,"key_mask":0
            }}),
            alphabet: String::new(),
            modulus: String::new(),
            flow: String::new(),
        };
        let literal = |start, value: String| Literal {
            key: None,
            start,
            end: start + 5,
            value,
            packed: None,
            continuation: 0,
            pool_offset: 0,
        };
        let mut response = Response {
            raw: vec![0; 6000],
            program: data::Program {
                sha256: String::new(),
                byte_length: 6000,
                code_length: 6000,
                strings: vec![],
            },
            literals: vec![],
            packed: vec![],
        };
        for i in 0..34 {
            let name = format!("State{i}");
            let at = if i < 32 {
                100 + i * 30
            } else {
                4000 + (i - 32) * 500
            };
            let mut input = vec![1, 2, 3];
            input.extend(name.encode_utf16().flat_map(u16::to_be_bytes));
            let digest = blake3::hash(&input);
            let digest = digest.as_bytes();
            let tail = u32::from_be_bytes(digest[28..].try_into().unwrap());
            response.raw[at] = 20;
            let key = plan.format.key(0, 20);
            let hash = state_hash(&name, 100).to_be_bytes();
            for (offset, byte) in hash.iter().enumerate() {
                response.raw[at + 1 + offset] = byte ^ key;
            }
            let target = (123 ^ (tail >> 8)).to_be_bytes();
            for (offset, byte) in target[1..].iter().enumerate() {
                response.raw[at + 6 + offset] = byte ^ key;
            }
            for (offset, byte) in digest[..16].iter().enumerate() {
                response.raw[at + 10 + offset] = byte ^ key;
            }
            if i == 33 {
                response.raw[at + 10] ^= 1;
            }
            response.literals.push(literal(5000 + i * 10, name));
        }
        response.literals.extend([
            literal(4015, "RTCRtpReceiver".into()),
            literal(4515, "trustedTypes".into()),
        ]);
        let descriptors = authenticated_targets(&response, &plan).unwrap();
        assert_eq!(descriptors.len(), 33);
        let table = select_state_table(descriptors.clone()).unwrap();
        assert_eq!(table.len(), 32);
        assert!(table.iter().all(|t| t.at < 4000));
        exclude_descriptor_operands(&mut response.literals, &descriptors);
        assert!(!response.literals.iter().any(|l| l.start == 4015));
        assert!(response.literals.iter().any(|l| l.start == 4515));
    }
}

impl Response {
    pub fn frame(&self) -> Result<Value> {
        let mut identifiers = Vec::new();
        let mut inputs = Vec::new();
        let (mut seen_ids, mut seen_inputs) = (HashSet::new(), HashSet::new());
        for (p, v) in &self.packed {
            if re(data::TASK).is_match(v) && seen_ids.insert(v) {
                identifiers.push((*p, v))
            }
            if let Ok(input) = serde_json::from_str::<String>(v) {
                if data::token(&input) && seen_inputs.insert(input.clone()) {
                    inputs.push((*p, input))
                }
            }
        }
        let count = identifiers.len();
        require(
            (2..=128).contains(&count) && inputs.len() == count,
            "Frame envelopes incomplete",
        )?;
        let mut rotations = Vec::new();
        for (i, (p, _)) in identifiers.iter().enumerate() {
            let literal = self
                .literals
                .iter()
                .find(|l| l.start == *p)
                .ok_or_else(|| crate::Error::new("INVALID_DATA", "Task literal boundary"))?;
            let end = p + literal.value.chars().count();
            let neighbors: Vec<_> = inputs
                .iter()
                .enumerate()
                .filter(|(_, x)| end <= x.0 && x.0 - end <= 96)
                .collect();
            if neighbors.len() == 1 {
                rotations.push((neighbors[0].0 + count - i) % count)
            }
        }
        require(
            rotations.len() > count / 2 && rotations.iter().all(|r| *r == rotations[0]),
            "Task rotation ambiguous",
        )?;
        let values = self.values();
        let extended: HashSet<_> = self
            .literals
            .iter()
            .filter(|l| l.continuation > 0)
            .map(|l| l.start)
            .collect();
        let state_values: Vec<_> = self
            .packed
            .iter()
            .filter(|(p, v)| extended.contains(p) && data::token(v))
            .map(|(_, v)| v.as_str())
            .collect();
        let submit = data::one(
            state_values
                .iter()
                .filter(|s| values.iter().filter(|x| **x == **s).count() > 1)
                .copied(),
        )?;
        let state = data::one(state_values.iter().filter(|s| **s != submit).copied())?;
        Ok(
            json!({"flow":self.one(data::FLOW)?,"vaZBm3":submit,"yVlbA2":state,"nidg0":self.one(data::NONCE)?,
            "tasks":identifiers.iter().enumerate().map(|(i,(_,id))|json!({"hmwT6":id,"nuvy8":inputs[(i+rotations[0])%count].1})).collect::<Vec<_>>()}),
        )
    }
}
pub(crate) fn state_hash(name: &str, seed: u32) -> u32 {
    name.encode_utf16()
        .fold(seed, |a, b| (a ^ b as u32).wrapping_mul(16777619))
}
pub fn state_targets(response: &Response, plan: &Plan) -> Result<Vec<Target>> {
    select_state_table(authenticated_targets(response, plan)?)
}
fn authenticated_targets(response: &Response, plan: &Plan) -> Result<Vec<Target>> {
    require(
        response.program.code_length >= 26
            && response.program.code_length <= 512 * 1024
            && response.program.code_length <= response.raw.len(),
        "State-table bounds",
    )?;
    let profile = &plan.meta["dispatcher_profile"];
    require(!profile.is_null(), "Missing dispatcher format")?;
    let names: HashSet<_> = response
        .literals
        .iter()
        .map(|l| l.value.as_str())
        .filter(|n| n.len() <= 12 && re(r"^[A-Za-z0-9_]*$").is_match(n))
        .collect();
    require(names.len() <= 4096, "State count limit")?;
    let mut lookup: HashMap<u32, Vec<&str>> = HashMap::new();
    for name in names {
        lookup
            .entry(state_hash(name, uint(&profile["fnv_seed"])? as u32))
            .or_default()
            .push(name)
    }
    let f = &plan.format;
    let opcode = uint(&profile["opcode"])? as u8;
    let keys = f.instruction_keys(opcode);
    let raw = &response.raw;
    let prefix: Vec<_> = array(&profile["prefix"])?
        .iter()
        .map(|v| v.as_u64().unwrap_or(0) as u8)
        .collect();
    let slots =
        |name: &str| -> Result<Vec<usize>> { array(&profile[name])?.iter().map(uint).collect() };
    let hashes = slots("hash_slots")?;
    let digests = slots("digest_slots")?;
    let targets = slots("target_slots")?;
    let mut found = Vec::new();
    for at in 0..=response.program.code_length - 26 {
        for &(_, key) in keys.at(f, raw[at], at) {
            let byte = |p: usize| f.byte(raw[p], key);
            let hash = hashes
                .iter()
                .fold(0u32, |a, s| (a << 8) | byte(at + 1 + s) as u32);
            let Some(names) = lookup.get(&hash) else {
                continue;
            };
            let expected: Vec<_> = (0..16)
                .step_by(4)
                .flat_map(|g| digests.iter().map(move |s| byte(at + 10 + g + s)))
                .collect();
            for name in names {
                let mut input = prefix.clone();
                for c in name.encode_utf16() {
                    input.extend(c.to_be_bytes())
                }
                let digest = blake3::hash(&input);
                let digest = digest.as_bytes();
                if digest[..16] != expected {
                    continue;
                }
                let tail = u32::from_be_bytes(digest[28..32].try_into().unwrap());
                let target = targets
                    .iter()
                    .fold(0usize, |a, s| (a << 8) | byte(at + 1 + s) as usize)
                    ^ (tail >> 8) as usize;
                require(
                    target < response.program.code_length,
                    "Authenticated state outside program",
                )?;
                let next = byte(at + 1 + uint(&profile["key_slot"])?)
                    ^ uint(&profile["key_mask"])? as u8
                    ^ tail as u8;
                found.push(Target {
                    name: (*name).into(),
                    at,
                    target,
                    key: next,
                });
            }
        }
    }
    Ok(found)
}
fn select_state_table(found: Vec<Target>) -> Result<Vec<Target>> {
    let mut groups = Vec::<Vec<Target>>::new();
    for item in found {
        if groups
            .last()
            .is_none_or(|g| item.at - g.last().unwrap().at > 512)
        {
            groups.push(vec![])
        }
        groups.last_mut().unwrap().push(item)
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    require(
        !groups.is_empty()
            && groups[0].len() >= 32
            && (groups.len() < 2 || groups[0].len() > 2 * groups[1].len()),
        "No unique authenticated state table",
    )?;
    let result = groups.remove(0);
    require(
        result.iter().map(|t| &t.name).collect::<HashSet<_>>().len() == result.len(),
        "Ambiguous state names",
    )?;
    Ok(result)
}
impl Catalog {
    pub fn read(response: &mut Response, plan: &Plan) -> Result<Self> {
        let descriptors = authenticated_targets(response, plan)?;
        let targets = select_state_table(descriptors.clone())?;
        let names: HashSet<_> = targets
            .iter()
            .map(|t| t.name.as_str())
            .filter(|name| !name.is_empty())
            .collect();
        let seen: HashSet<_> = response
            .literals
            .iter()
            .map(|l| (l.start, l.value.clone()))
            .collect();
        for l in data::literals(&response.raw, &response.program, plan, true, 12, false)? {
            if names.contains(l.value.as_str()) && !seen.contains(&(l.start, l.value.clone())) {
                response.literals.push(l)
            }
        }
        response.literals.sort_by_key(|l| l.start);
        // Module-local descriptors have the same authenticated operand layout
        // as the main table, even though they are not table entries.
        exclude_descriptor_operands(&mut response.literals, &descriptors);
        let mut result = Self::link(response, response.frame()?, targets)?;
        result.order = result.registration_order(response)?;
        Ok(result)
    }
    pub(crate) fn link(
        response: &Response,
        parameters: Value,
        targets: Vec<Target>,
    ) -> Result<Self> {
        let by_id: HashMap<_, _> = array(&parameters["tasks"])?
            .iter()
            .map(|v| {
                Ok((
                    string(&v["hmwT6"])?.to_string(),
                    string(&v["nuvy8"])?.to_string(),
                ))
            })
            .collect::<Result<_>>()?;
        require(
            by_id.len() == array(&parameters["tasks"])?.len(),
            "Duplicate frame task identifier",
        )?;
        let mut identifiers: Vec<_> = response
            .packed
            .iter()
            .filter(|(_, v)| by_id.contains_key(v))
            .collect();
        identifiers.sort_by_key(|x| x.0);
        require(
            identifiers.len() == by_id.len() && !identifiers.is_empty(),
            "Task identifier boundaries",
        )?;
        let first = identifiers[0].0;
        let mut labels = HashMap::<String, Vec<usize>>::new();
        for literal in &response.literals {
            if re(r"^[A-Za-z]{4,7}[0-9]$").is_match(&literal.value) {
                labels
                    .entry(literal.value.clone())
                    .or_default()
                    .push(literal.start)
            }
        }
        for offsets in labels.values_mut() {
            offsets.sort_unstable();
        }
        labels.retain(|_, p| p.len() == 2 && p[0] < first && first < p[1]);
        let mut linked = Vec::new();
        for (i, (offset, id)) in identifiers.iter().enumerate() {
            let end = identifiers
                .get(i + 1)
                .map_or(response.program.code_length, |x| x.0)
                .min(offset + 8192);
            let choices: Vec<_> = labels
                .iter()
                .filter(|(_, p)| *offset < p[1] && p[1] < end)
                .collect();
            require(choices.len() == 1, "Missing/ambiguous task label")?;
            let (label, positions) = choices[0];
            linked.push((positions[0], (*id).clone(), label.clone(), positions[1]));
        }
        linked.sort_by_key(|x| x.0);
        require(
            linked.iter().map(|x| &x.2).collect::<HashSet<_>>().len() == linked.len(),
            "Repeated task label",
        )?;
        let mut modules = Vec::new();
        for (i, (start, id, label, metadata)) in linked.iter().enumerate() {
            let end = linked.get(i + 1).map_or(first, |x| x.0);
            require(
                *start < end && end <= response.program.code_length,
                "Module bounds",
            )?;
            modules.push(Module {
                id: id.clone(),
                data: by_id[id].clone(),
                label: label.clone(),
                start: *start,
                end,
                metadata: *metadata,
            })
        }
        Ok(Self {
            parameters,
            modules,
            targets,
            order: vec![],
        })
    }
    pub(crate) fn registration_order(&self, response: &Response) -> Result<Vec<String>> {
        require((4..=256).contains(&self.modules.len()), "Task-chain size")?;
        let names: HashSet<_> = self.targets.iter().map(|t| t.name.as_str()).collect();
        require(
            names.contains("") && names.len() == self.targets.len(),
            "Missing or ambiguous initial state",
        )?;
        let mut targets = self.targets.clone();
        targets.sort_by_key(|t| t.target);
        require(
            targets
                .iter()
                .map(|t| t.target)
                .collect::<HashSet<_>>()
                .len()
                == targets.len(),
            "State boundary collision",
        )?;
        let mut literals: Vec<_> = response
            .literals
            .iter()
            .filter(|l| !l.value.is_empty() && names.contains(l.value.as_str()))
            .collect();
        literals.sort_by_key(|l| l.start);
        let mut adjacent = HashMap::<String, HashSet<String>>::new();
        for module in &self.modules {
            let index = literals.partition_point(|l| l.start < module.start);
            require(
                index > 0
                    && index + 1 < literals.len()
                    && literals[index].start == module.start
                    && literals[index].value == module.label,
                "Task state boundary",
            )?;
            let (before, after) = (literals[index - 1], literals[index + 1]);
            let states = if module.start - before.start <= 256
                && after.start - module.start <= 256
                && before.value != after.value
            {
                HashSet::from([before.value.clone(), after.value.clone()])
            } else {
                literals
                    .iter()
                    .filter(|l| module.start.abs_diff(l.start) <= 256 && l.value != module.label)
                    .map(|l| l.value.clone())
                    .collect()
            };
            require(states.len() == 2, "Task completion adjacency")?;
            adjacent.insert(module.label.clone(), states);
        }
        let mut common = adjacent.values().next().cloned().unwrap_or_default();
        for values in adjacent.values() {
            common.retain(|v| values.contains(v))
        }
        require(common.len() == 1, "Task return state ambiguous")?;
        let terminal = common.into_iter().next().unwrap();
        let completions = adjacent
            .values()
            .filter_map(|set| set.iter().find(|s| **s != terminal))
            .collect::<HashSet<_>>();
        require(
            completions.len() == self.modules.len()
                && !completions.iter().any(|s| adjacent.contains_key(*s)),
            "Ambiguous task completion states",
        )?;
        let mut edges = HashMap::new();
        for (i, t) in targets.iter().enumerate() {
            let end = targets
                .get(i + 1)
                .map_or(response.program.code_length, |t| t.target);
            let succ: HashSet<_> = literals
                .iter()
                .filter(|l| t.target <= l.start && l.start < end)
                .map(|l| l.value.clone())
                .collect();
            if succ.len() == 1 {
                edges.insert(t.name.clone(), succ.into_iter().next().unwrap());
            }
        }
        let mut registrations = HashMap::new();
        for module in &self.modules {
            let index = targets.partition_point(|t| t.target <= module.metadata);
            require(index > 0, "Registration block missing")?;
            let name = &targets[index - 1].name;
            require(
                !registrations.contains_key(name) && edges.get(name) == Some(&module.label),
                "Registration edge mismatch",
            )?;
            registrations.insert(name.clone(), module.id.clone());
        }
        for (label, values) in adjacent {
            let completion = values.into_iter().find(|v| *v != terminal).unwrap();
            require(
                !edges.contains_key(&label) || edges[&label] == completion,
                "Definition edge mismatch",
            )?;
            edges.insert(label, completion);
        }
        let (mut state, mut seen, mut order) = (String::new(), HashSet::new(), Vec::new());
        for _ in 0..3 * self.modules.len() + 8 {
            require(seen.insert(state.clone()), "Cyclic registration")?;
            if let Some(id) = registrations.get(&state) {
                order.push(id.clone());
                if order.len() == self.modules.len() {
                    require(
                        order.iter().collect::<HashSet<_>>().len() == order.len(),
                        "Repeated task registration",
                    )?;
                    return Ok(order);
                }
            }
            let Some(next) = edges.get(&state) else { break };
            state = next.clone();
        }
        Err(crate::Error::new(
            "UNSUPPORTED_FORMAT",
            "Incomplete registration chain",
        ))
    }
}
