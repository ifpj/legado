use crate::{
    compiler::{integer, operation},
    constants::{child, children, unwrap, Constant, Tables},
    data::Format,
    require, Error, Result,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;
mod boundaries;
#[cfg(test)]
mod boundaries_tests;
#[cfg(test)]
mod prefix_tests;

pub fn unknowns<'a>(
    t: &Tables<'a>,
    expression: Node<'a>,
    excluded: &[Node<'a>],
) -> HashSet<String> {
    let scope = t.scope(expression);
    let globals = t.globals.borrow();
    t.walk(expression, true)
        .into_iter()
        .filter(|n| {
            n.kind() == "identifier"
                && !excluded
                    .iter()
                    .any(|e| e.start_byte() <= n.start_byte() && n.end_byte() <= e.end_byte())
        })
        .filter_map(|n| {
            let name = t.text(n);
            if scope
                .get(name)
                .or_else(|| globals.get(name))
                .is_none_or(|v| matches!(v, Constant::Dynamic))
            {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect()
}
pub fn handler<'a>(t: &Tables<'a>, index: usize) -> Result<(Node<'a>, u8)> {
    let mut found = Vec::new();
    for class in t
        .all()
        .iter()
        .copied()
        .into_iter()
        .filter(|n| n.kind() == "class_declaration")
    {
        let Some(switch) = t.walk(class, true).into_iter().find(|n| {
            n.kind() == "switch_statement"
                && n.child_by_field_name("body")
                    .is_some_and(|b| b.named_child_count() >= 60)
        }) else {
            continue;
        };
        let functions: HashMap<_, _> = children(
            class
                .parent()
                .ok_or_else(|| Error::new("INVALID_DATA", "Class owner"))?,
        )
        .into_iter()
        .filter(|n| n.kind() == "function_declaration")
        .map(|n| Ok((t.text(child(n, "name")?).to_string(), n)))
        .collect::<Result<_>>()?;
        let cases = children(child(switch, "body")?);
        let case = *cases
            .get(index)
            .ok_or_else(|| Error::new("INVALID_DATA", "Handler index"))?;
        let names: HashSet<_> = t
            .walk(case, true)
            .into_iter()
            .filter(|n| n.kind() == "identifier" && functions.contains_key(t.text(*n)))
            .map(|n| t.text(n))
            .collect();
        if names.len() == 1 {
            let name = names.into_iter().next().unwrap();
            found.push((functions[name], integer(t, child(case, "value")?)? as u8))
        }
    }
    require(found.len() == 1, "No unique operand handler")?;
    Ok(found.remove(0))
}
pub fn raw_reads(node: Node<'_>) -> Vec<Node<'_>> {
    crate::constants::walk(node, true)
        .into_iter()
        .filter(|n| {
            n.kind() == "subscript_expression"
                && n.child_by_field_name("index")
                    .is_some_and(|n| n.kind() == "update_expression")
        })
        .collect()
}
pub fn owner(mut node: Node<'_>) -> Node<'_> {
    while !matches!(node.kind(), "assignment_expression" | "variable_declarator") {
        let Some(p) = node.parent() else { break };
        node = p;
    }
    node
}
pub fn operand<'a>(t: &Tables<'a>, read: Node<'a>, f: &Format) -> Result<(String, u8)> {
    let mut expression = read;
    let mut found = Vec::new();
    while !matches!(
        expression.kind(),
        "assignment_expression" | "variable_declarator" | "sequence_expression"
    ) && t.text(expression).len() < 1300
    {
        let names = unknowns(t, expression, &[read]);
        if names.len() == 1 {
            let key = names.into_iter().next().unwrap();
            let scope = t.scope(expression);
            let evaluate = |before: u8, value: u8| {
                let mut env = scope.clone();
                env.insert(key.clone(), Constant::Number(before as f64));
                t.bound(
                    expression,
                    &mut env,
                    HashMap::from([(
                        read.id(),
                        Constant::Number(value.wrapping_add(f.byte_offset) as f64),
                    )]),
                )
                .and_then(|v| v.integer())
                .map(|v| v as u8)
            };
            if let Ok(mask) = evaluate(0, 0) {
                if [(0, 0), (17, 9), (219, 243), (255, 255)]
                    .iter()
                    .all(|(before, value)| evaluate(*before, *value) == Ok(value ^ before ^ mask))
                {
                    found.push(mask)
                }
            }
        }
        let Some(parent) = expression.parent() else {
            break;
        };
        expression = parent;
    }
    let owner = owner(read);
    let name = owner
        .child_by_field_name(if owner.kind() == "assignment_expression" {
            "left"
        } else {
            "name"
        })
        .filter(|n| n.kind() == "identifier")
        .ok_or_else(|| Error::new("INVALID_DATA", "Operand binding"))?;
    Ok((
        t.text(name).into(),
        *found
            .last()
            .ok_or_else(|| Error::new("INVALID_DATA", "Operand XOR format"))?,
    ))
}
pub fn ordered_reads<'a>(
    t: &Tables<'a>,
    node: Node<'a>,
    count: usize,
    f: &Format,
    validate: bool,
) -> Result<Vec<Node<'a>>> {
    ordered_reads_impl(t, node, count, f, validate, false)
}
// For region widths only: inner cases stay grouped inside their outer case.
// Their individual execution order is deliberately not inferred here.
pub(super) fn outer_case_reads<'a>(
    t: &Tables<'a>,
    node: Node<'a>,
    count: usize,
    f: &Format,
) -> Result<Vec<Node<'a>>> {
    ordered_reads_impl(t, node, count, f, false, true)
}
fn ordered_reads_impl<'a>(
    t: &Tables<'a>,
    node: Node<'a>,
    count: usize,
    f: &Format,
    validate: bool,
    outer_cases: bool,
) -> Result<Vec<Node<'a>>> {
    let mut groups = HashMap::<(String, String), Vec<Node<'a>>>::new();
    for read in raw_reads(node) {
        groups
            .entry((
                t.text(child(read, "object")?).into(),
                t.text(child(read, "index")?).into(),
            ))
            .or_default()
            .push(read)
    }
    let mut groups: Vec<_> = groups
        .into_values()
        .filter(|r| r.len() == count && (!validate || r.iter().all(|r| operand(t, *r, f).is_ok())))
        .collect();
    require(groups.len() == 1, "Raw byte array ownership")?;
    execution_order(t, node, groups.remove(0), outer_cases)
}
pub(super) fn execution_order<'a>(
    t: &Tables<'a>,
    node: Node<'a>,
    mut reads: Vec<Node<'a>>,
    outer_cases: bool,
) -> Result<Vec<Node<'a>>> {
    reads.sort_by_key(|n| n.start_byte());
    let rows = t.walk(node, true);
    let mut controls = Vec::new();
    for switch in rows
        .iter()
        .copied()
        .filter(|n| n.kind() == "switch_statement")
    {
        let selector = unwrap(child(switch, "value")?);
        if selector.kind() != "subscript_expression" {
            continue;
        }
        let variable = t.text(child(selector, "object")?);
        for assignment in rows.iter().copied().filter(|n| {
            n.kind() == "assignment_expression"
                && n.child_by_field_name("left")
                    .is_some_and(|l| t.text(l) == variable)
                || n.kind() == "variable_declarator"
                    && n.child_by_field_name("name")
                        .is_some_and(|l| t.text(l) == variable)
        }) {
            let attempt = (|| -> Result<Vec<Node<'a>>> {
                let call = unwrap(child(
                    assignment,
                    if assignment.kind() == "variable_declarator" {
                        "value"
                    } else {
                        "right"
                    },
                )?);
                require(call.kind() == "call_expression", "Split call")?;
                let function = child(call, "function")?;
                require(
                    matches!(
                        function.kind(),
                        "member_expression" | "subscript_expression"
                    ),
                    "Split member",
                )?;
                let method = if let Some(p) = function.child_by_field_name("property") {
                    t.text(p).to_string()
                } else {
                    t.value(child(function, "index")?)?.text()?
                };
                require(method == "split", "Split operation")?;
                let args = children(child(call, "arguments")?);
                require(
                    args.len() == 1 && t.value(args[0])?.text()? == "|",
                    "Split delimiter",
                )?;
                let sequence = t.value(child(function, "object")?)?.text()?;
                let order: Vec<_> = sequence.split('|').collect();
                require(
                    !order.is_empty()
                        && order.len() <= 128
                        && order.iter().collect::<HashSet<_>>().len() == order.len(),
                    "Split control count",
                )?;
                let mut ranks = HashMap::new();
                for case in children(child(switch, "body")?) {
                    if let Some(label) = case.child_by_field_name("value") {
                        let label = t.value(label)?.text()?;
                        if let Some(index) = order.iter().position(|v| *v == label) {
                            ranks.insert(case.start_byte(), index);
                        }
                    }
                }
                let mut sorted = Vec::new();
                for read in &reads {
                    let mut owner = *read;
                    while if outer_cases {
                        !ranks.contains_key(&owner.start_byte())
                    } else {
                        !matches!(owner.kind(), "switch_case" | "switch_default")
                    } {
                        owner = owner
                            .parent()
                            .ok_or_else(|| Error::new("INVALID_DATA", "Control owner"))?;
                    }
                    let rank = *ranks
                        .get(&owner.start_byte())
                        .ok_or_else(|| Error::new("INVALID_DATA", "Control rank"))?;
                    sorted.push((rank, read.start_byte(), *read));
                }
                sorted.sort_by_key(|x| (x.0, x.1));
                Ok(sorted.into_iter().map(|x| x.2).collect())
            })();
            if let Ok(value) = attempt {
                controls.push(value)
            }
        }
    }
    require(controls.len() <= 1, "Ambiguous flattened operand order")?;
    Ok(controls.pop().unwrap_or(reads))
}
fn variable_slot<'a>(t: &Tables<'a>, n: Node<'a>, operands: &[(String, u8)]) -> Result<usize> {
    let names: HashSet<_> = t
        .walk(n, true)
        .into_iter()
        .filter(|n| n.kind() == "identifier")
        .map(|n| t.text(n))
        .collect();
    let positions: Vec<_> = operands
        .iter()
        .enumerate()
        .filter(|(_, v)| names.contains(v.0.as_str()))
        .map(|(i, _)| i)
        .collect();
    require(positions.len() == 1, "Operand role ambiguity")?;
    Ok(positions[0])
}
pub fn inputs(t: &Tables<'_>, f: &Format) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for (name, index, count) in [
        ("array", 9, 1),
        ("small", 11, 2),
        ("index", 39, 4),
        ("move", 27, 2),
        ("flow-index", 38, 3),
        ("flow-read", 42, 3),
        ("flow-unary", 25, 2),
    ] {
        let (handler, opcode) = handler(t, index)?;
        let reads = ordered_reads(t, handler, count, f, true)?;
        let operands: Vec<_> = reads
            .iter()
            .map(|n| operand(t, *n, f))
            .collect::<Result<_>>()?;
        let stores: Vec<_> = t
            .walk(handler, true)
            .into_iter()
            .filter(|n| {
                n.kind() == "assignment_expression"
                    && n.child_by_field_name("left")
                        .is_some_and(|l| l.kind() == "subscript_expression")
            })
            .collect();
        let roles = if name == "array" {
            json!({"destination":0})
        } else if name == "index" {
            let stores: Vec<_> = stores
                .into_iter()
                .filter(|n| {
                    child(*n, "left")
                        .and_then(|n| child(n, "object"))
                        .is_ok_and(|n| n.kind() == "subscript_expression")
                        && child(*n, "right").is_ok_and(|n| n.kind() == "subscript_expression")
                })
                .collect();
            require(stores.len() == 1, "Indexed constant write")?;
            let left = child(stores[0], "left")?;
            let right = child(stores[0], "right")?;
            json!({"receiver":variable_slot(t,child(child(left,"object")?,"index")?,&operands)?,"index":variable_slot(t,child(left,"index")?,&operands)?,"value":variable_slot(t,child(right,"index")?,&operands)?})
        } else if matches!(name, "small" | "move") {
            let mut choices = Vec::new();
            for store in stores {
                if let (Ok(dest), Ok(value)) = (
                    variable_slot(t, child(child(store, "left")?, "index")?, &operands),
                    variable_slot(t, child(store, "right")?, &operands),
                ) {
                    choices.push(json!({"destination":dest,"value":value}))
                }
            }
            require(choices.len() == 1, "Scalar input role")?;
            choices.remove(0)
        } else {
            json!({})
        };
        out.insert(
            name.into(),
            json!([
                opcode,
                operands.iter().map(|(_, m)| m).collect::<Vec<_>>(),
                roles
            ]),
        );
    }
    for (name, index) in [("flow-varint", 16), ("flow-varint-store", 14)] {
        if let Ok((handler, opcode)) = handler(t, index) {
            if let Ok(reads) = ordered_reads(t, handler, 3, f, true) {
                let masks: Vec<_> = reads
                    .into_iter()
                    .map(|n| operand(t, n, f).map(|x| x.1))
                    .collect::<Result<_>>()?;
                if masks[0] == masks[1] {
                    out.insert(name.into(), json!([opcode,masks,{"suffix":1,"maxBytes":3}]));
                }
            }
        }
    }
    if let Ok(tags) = boundaries::empty_literals(t, f) {
        out.insert(
            "literal-empty".into(),
            json!([f.state_step,[],{"tags":tags}]),
        );
    }
    if let Ok(profile) = boundaries::branch(t, f) {
        out.insert("flow-branch".into(), profile);
    }
    if let Ok(profile) = boundaries::jump(t, f) {
        out.insert("flow-jump".into(), profile);
    }
    if let Ok(profile) = boundaries::fallthrough(t, f) {
        out.insert("flow-branch-fallthrough".into(), profile);
    }
    if let Ok(profile) = boundaries::fixed_apply(t, f) {
        out.insert("flow-apply".into(), profile);
    }
    if let Ok(profile) = boundaries::fixed(t, f, 40, 4) {
        out.insert("flow-write".into(), profile);
    }
    if let Ok(profile) = boundaries::function_reference(t, f) {
        out.insert("flow-reference".into(), profile);
    }
    if let Ok(profile) = boundaries::closure(t, f) {
        out.insert("flow-closure".into(), profile);
    }
    if let Ok(profile) = boundaries::property_pool(t, f) {
        out.insert("flow-property-pool".into(), profile);
    }
    if let Ok(profiles) = boundaries::shared_binary(t, f) {
        out.extend(profiles);
    }
    if let Ok(profiles) = boundaries::shared_unary(t, f) {
        out.extend(profiles);
    }
    crate::cpu::check()?;
    Ok(json!(out))
}
pub fn immediate(t: &Tables<'_>, f: &Format) -> Result<Value> {
    let (handler, opcode) = handler(t, 40)?;
    let reads = ordered_reads(t, handler, 4, f, false)?;
    let stores: Vec<_> = t
        .walk(handler, true)
        .into_iter()
        .filter(|n| {
            n.kind() == "assignment_expression"
                && child(*n, "left")
                    .and_then(|n| child(n, "object"))
                    .is_ok_and(|n| n.kind() == "subscript_expression")
                && child(*n, "right").is_ok_and(|n| n.kind() == "identifier")
        })
        .collect();
    require(stores.len() == 1, "Immediate store ambiguity")?;
    let store = stores[0];
    let value = t.text(child(store, "right")?);
    let object = t.text(child(child(child(store, "left")?, "object")?, "index")?);
    let mut slots = HashMap::new();
    for (i, read) in reads.into_iter().enumerate() {
        if let Ok((name, mask)) = operand(t, read, f) {
            slots.insert(name, (i, mask));
        } else {
            let owner = owner(read);
            if owner
                .child_by_field_name("left")
                .or_else(|| owner.child_by_field_name("name"))
                .is_some_and(|n| n.kind() == "identifier" && t.text(n) == object)
            {
                slots.insert(object.to_string(), (i, 0));
            }
        }
    }
    require(
        slots.contains_key(value) && slots.contains_key(object),
        "Immediate value/receiver operands",
    )?;
    Ok(json!([
        opcode,
        slots[value].0,
        slots[value].1,
        slots[object].0
    ]))
}
pub fn properties(t: &Tables<'_>, f: &Format) -> Result<Value> {
    let (handler, opcode) = handler(t, 41)?;
    let mut masks = HashSet::new();
    for n in t
        .walk(handler, true)
        .into_iter()
        .filter(|n| n.kind() == "subscript_expression")
    {
        let expression = child(n, "index")?;
        if t.text(expression).len() > 600 {
            continue;
        }
        let reads = raw_reads(expression);
        if reads.len() != 1 {
            continue;
        }
        let names = unknowns(t, expression, &reads);
        if names.len() != 2 {
            continue;
        }
        for seed_name in &names {
            let key_name = names.iter().find(|n| *n != seed_name).unwrap();
            let read = reads[0];
            let evaluate = |key: u8, seed: u32, value: u8| {
                let mut scope = t.scope(expression);
                scope.insert(key_name.clone(), Constant::Number(key as f64));
                scope.insert(seed_name.clone(), Constant::Number(seed as f64));
                t.bound(
                    expression,
                    &mut scope,
                    HashMap::from([(
                        read.id(),
                        Constant::Number(value.wrapping_add(f.byte_offset) as f64),
                    )]),
                )
                .and_then(|v| v.integer())
                .map(|v| v as u8)
            };
            if let Ok(mask) = evaluate(0, 0, 0) {
                let mut format = f.clone();
                format.char_xor = mask;
                if [(0, 0, 0), (91, 0x12345678, 5), (239, 0xffffffff, 227)]
                    .iter()
                    .all(|(key, seed, value)| {
                        evaluate(*key, *seed, *value)
                            .is_ok_and(|v| v == format.char(value ^ key, *seed))
                    })
                {
                    masks.insert(mask);
                }
            }
        }
    }
    require(masks.len() == 1, "Property character transform ambiguity")?;
    Ok(json!([opcode, *masks.iter().next().unwrap()]))
}
pub fn integer_tag(t: &Tables<'_>, f: &Format) -> Result<u8> {
    let (handler, _) = handler(t, 10)?;
    let mut tags = HashSet::new();
    let mut diagnostics = Vec::new();
    for node in t
        .walk(handler, true)
        .into_iter()
        .filter(|n| n.kind() == "if_statement")
    {
        let body = child(node, "consequence")?;
        if t.text(body).len() > 1800
            || !t
                .walk(body, true)
                .iter()
                .any(|n| n.kind() == "do_statement")
        {
            continue;
        }
        let limits: HashSet<_> = t
            .walk(body, true)
            .into_iter()
            .filter(|n| operation(t, *n, ">=").is_some_and(|p| integer(t, p[1]) == Ok(35)))
            .map(|n| unwrap(n).start_byte())
            .collect();
        diagnostics.push(format!("limits={}", limits.len()));
        if limits.len() != 1 {
            continue;
        }
        let mut condition = unwrap(child(node, "condition")?);
        if condition.kind() == "unary_expression" && t.text(child(condition, "operator")?) == "!" {
            condition = unwrap(child(condition, "argument")?)
        }
        let Some(pair) = operation(t, condition, "===").or_else(|| operation(t, condition, "!=="))
        else {
            continue;
        };
        for [left, right] in [pair, [pair[1], pair[0]]] {
            let left = unwrap(left);
            if left.kind() != "identifier" {
                continue;
            }
            let Ok(tag) = t.value(right).and_then(|v| v.integer()) else {
                continue;
            };
            if !(0..=255).contains(&tag) {
                continue;
            }
            let masks: HashSet<_> = raw_reads(handler)
                .into_iter()
                .filter(|r| r.end_byte() < node.start_byte())
                .filter_map(|r| operand(t, r, f).ok())
                .filter(|(name, _)| name == t.text(left))
                .map(|(_, m)| m)
                .collect();
            if masks.len() == 1 {
                tags.insert(tag as u8 ^ *masks.iter().next().unwrap());
            }
        }
    }
    require(
        tags.len() == 1,
        &format!(
            "Integer literal kind ambiguity: {:?} {:?}",
            tags, diagnostics
        ),
    )?;
    Ok(*tags.iter().next().unwrap())
}

pub fn dispatcher<'a>(t: &Tables<'a>, f: &Format) -> Result<Value> {
    let (handler, opcode) = handler(t, 22)?;
    let mut all = raw_reads(handler);
    all.sort_by_key(|n| n.start_byte());
    require(all.len() == 13, "Dispatcher input count")?;
    let reads = &all[..9];
    let mut owners = reads.iter().map(|n| owner(*n)).collect::<Vec<_>>();
    owners.sort_by_key(|n| n.start_byte());
    owners.dedup_by_key(|n| n.start_byte());
    require(owners.len() == 4, "Dispatcher field count")?;
    let expressions: Vec<_> = owners
        .iter()
        .map(|n| child(*n, "right"))
        .collect::<Result<_>>()?;
    let mut common = unknowns(t, expressions[0], reads);
    for expr in &expressions[1..] {
        common.retain(|n| unknowns(t, *expr, reads).contains(n));
    }
    require(common.len() == 1, "Dispatcher rolling key ambiguity")?;
    let key_name = common.into_iter().next().unwrap();
    let evaluate = |expr: Node<'a>, reads: &[Node<'a>], raw: &[u8]| -> Result<u32> {
        let mut scope = t.scope(expr);
        scope.insert(key_name.clone(), Constant::Number(0.0));
        let overrides = reads
            .iter()
            .zip(raw)
            .map(|(n, v)| (n.id(), Constant::Number(*v as f64)))
            .collect();
        Ok(t.bound(expr, &mut scope, overrides)?.integer()? as u32)
    };
    let mut fields = Vec::new();
    for expr in &expressions {
        let base = evaluate(*expr, reads, &vec![f.byte_offset; 9])?;
        let mut weights = Vec::new();
        for i in 0..9 {
            let mut raw = vec![f.byte_offset; 9];
            raw[i] = f.byte_offset.wrapping_add(1);
            weights.push(evaluate(*expr, reads, &raw)? ^ base)
        }
        fields.push((base, weights));
    }
    let singles: Vec<_> = fields
        .iter()
        .filter(|(_, weights)| {
            weights.iter().filter(|n| **n != 0).count() == 1 && weights.contains(&1)
        })
        .map(|(base, weights)| (*base, weights.iter().position(|n| *n == 1).unwrap()))
        .collect();
    let hashes: Vec<_> = fields
        .iter()
        .filter(|(base, w)| {
            let mut nonzero: Vec<_> = w.iter().copied().filter(|n| *n != 0).collect();
            nonzero.sort_unstable();
            *base == 0 && nonzero == vec![1, 256, 65536, 16777216]
        })
        .collect();
    let targets: Vec<_> = fields
        .iter()
        .filter(|(base, w)| {
            let mut nonzero: Vec<_> = w.iter().copied().filter(|n| *n != 0).collect();
            nonzero.sort_unstable();
            *base == 0 && nonzero == vec![1, 256, 65536]
        })
        .collect();
    require(
        singles.len() == 2 && hashes.len() == 1 && targets.len() == 1 && singles[1].1 == 8,
        "Dispatcher packing layout",
    )?;
    let digest_reads = &all[9..];
    let digest_expr = child(owner(digest_reads[0]), "right")?;
    require(
        digest_reads.iter().all(|n| {
            digest_expr.start_byte() <= n.start_byte() && n.end_byte() <= digest_expr.end_byte()
        }),
        "Digest read boundaries",
    )?;
    require(
        evaluate(digest_expr, digest_reads, &vec![f.byte_offset; 4])? == 0,
        "Digest packing base",
    )?;
    let mut digest_weights = Vec::new();
    for i in 0..4 {
        let mut raw = vec![f.byte_offset; 4];
        raw[i] = f.byte_offset.wrapping_add(1);
        digest_weights.push(evaluate(digest_expr, digest_reads, &raw)?)
    }
    let rows = t.walk(handler, true);
    let assignments: Vec<_> = rows
        .iter()
        .copied()
        .filter(|n| {
            n.kind() == "assignment_expression"
                && n.child_by_field_name("left")
                    .is_some_and(|l| l.kind() == "identifier")
        })
        .collect();
    let mut hash_variables = HashSet::new();
    for assignment in &assignments {
        let name = t.text(child(*assignment, "left")?).to_string();
        let rhs = child(*assignment, "right")?;
        if [1u32, 3, 0xffffffff].iter().all(|v| {
            let mut scope = t.scope(rhs);
            scope.insert(name.clone(), Constant::Number(*v as f64));
            t.eval(rhs, &mut scope)
                .and_then(|v| v.integer())
                .is_ok_and(|n| n as u32 == v.wrapping_mul(16777619))
        }) {
            hash_variables.insert(name);
        }
    }
    let seeds: HashSet<_> = assignments
        .iter()
        .filter(|n| hash_variables.contains(t.text(child(**n, "left").unwrap())))
        .filter_map(|n| integer(t, child(*n, "right").ok()?).ok())
        .map(|n| n as u32)
        .collect();
    require(seeds.len() == 1, "Dispatcher FNV seed")?;
    let functions: Vec<_> = children(
        handler
            .parent()
            .ok_or_else(|| Error::new("INVALID_DATA", "Handler owner"))?,
    )
    .into_iter()
    .filter(|n| n.kind() == "function_declaration")
    .collect();
    let prefix = dispatcher_prefix(t, &functions)?;
    let slots = |weights: &[u32], shifts: &[u32]| -> Result<Vec<usize>> {
        shifts
            .iter()
            .map(|shift| {
                weights
                    .iter()
                    .position(|v| *v == 1u32 << shift)
                    .ok_or_else(|| Error::new("INVALID_DATA", "Packing order"))
            })
            .collect()
    };
    Ok(
        json!({"opcode":opcode,"register_slot":singles[0].1,"register_mask":singles[0].0,"hash_slots":slots(&hashes[0].1,&[24,16,8,0])?,"target_slots":slots(&targets[0].1,&[16,8,0])?,"key_slot":singles[1].1,"key_mask":singles[1].0,"fnv_seed":*seeds.iter().next().unwrap(),"prefix":prefix,"digest_slots":slots(&digest_weights,&[24,16,8,0])?}),
    )
}

fn dispatcher_prefix<'a>(t: &Tables<'a>, functions: &[Node<'a>]) -> Result<Vec<u8>> {
    const IV: [u32; 8] = [
        1779033703, 3144134277, 1013904242, 2773480762, 1359893119, 2600822924, 528734635,
        1541459225,
    ];
    let mut prefixes = HashSet::new();
    let mut helpers = 0;
    for function in functions {
        crate::cpu::check()?;
        let rows = t.walk(*function, false);
        // Obfuscation can spell the chunk size as ~-1025 or XOR arithmetic,
        // and IV words as expressions. Match their static values and shape,
        // not decimal substrings that may disappear or occur in unrelated code.
        let scalar_binding = |expected: f64| {
            rows.iter().any(|n| {
                let fields = match n.kind() {
                    "variable_declarator" => ("name", "value"),
                    "assignment_expression" => ("left", "right"),
                    _ => return false,
                };
                let (Ok(name), Ok(value)) = (child(*n, fields.0), child(*n, fields.1)) else {
                    return false;
                };
                name.kind() == "identifier"
                    && matches!(
                        unwrap(value).kind(),
                        "number" | "unary_expression" | "binary_expression"
                    )
                    && t.value(value).and_then(|v| v.number()) == Ok(expected)
            })
        };
        let has_iv = rows.iter().filter(|n| n.kind() == "array").any(|n| {
            let values = children(*n);
            values.len() == IV.len()
                && values.iter().zip(IV).all(|(n, word)| {
                    t.value(*n).and_then(|v| v.integer()).is_ok_and(|v| {
                        (i32::MIN as i64..=u32::MAX as i64).contains(&v) && v as u32 == word
                    })
                })
        });
        // Older supported helper forms declare the IV anchor and block size
        // as scalar bindings rather than an inline IV array/chunk multiply.
        if !has_iv && !scalar_binding(IV[0] as f64) {
            continue;
        }
        let has_chunk_stride = rows.iter().any(|n| {
            operation(t, *n, "*").is_some_and(|pair| {
                pair.into_iter()
                    .any(|n| t.value(n).and_then(|v| v.number()) == Ok(1024.0))
            })
        });
        if !has_chunk_stride && !scalar_binding(1024.0) {
            continue;
        }
        helpers += 1;
        for n in rows
            .into_iter()
            .filter(|n| n.kind() == "ternary_expression")
        {
            let (Ok(yes), Ok(no)) = (
                integer(t, child(n, "consequence")?),
                integer(t, child(n, "alternative")?),
            ) else {
                continue;
            };
            if !(65536..1i64 << 32).contains(&yes) || !(65536..1i64 << 32).contains(&no) {
                continue;
            }
            let condition = child(n, "condition")?;
            let unknown = unknowns(t, condition, &[]);
            if unknown.len() != 1 {
                continue;
            }
            let variable = unknown.into_iter().next().unwrap();
            let mut scope = t.scope(condition);
            scope.insert(variable.clone(), Constant::Number(0.0));
            let Ok(first) = t.eval(condition, &mut scope).and_then(|v| v.truth()) else {
                continue;
            };
            scope = t.scope(condition);
            scope.insert(variable, Constant::Number(1.0));
            let Ok(second) = t.eval(condition, &mut scope).and_then(|v| v.truth()) else {
                continue;
            };
            if first == second {
                continue;
            }
            let a = if first { yes } else { no } as u32;
            let b = if second { yes } else { no } as u32;
            let mut prefix = a.to_be_bytes().to_vec();
            prefix.extend(b.to_be_bytes());
            prefixes.insert(prefix);
        }
    }
    crate::cpu::check()?;
    if prefixes.len() != 1 {
        return Err(Error {
            details: json!({"stage":"dispatcher-profile","hashHelpers":helpers,"prefixCandidates":prefixes.len()}),
            ..Error::new("UNSUPPORTED_FORMAT", "Dispatcher BLAKE3 prefix")
        });
    }
    Ok(prefixes.into_iter().next().unwrap())
}
pub fn framed_salt(t: &Tables<'_>) -> Result<(u8, u32)> {
    let (handler, opcode) = handler(t, 36)?;
    Ok((opcode, framed_salt_for(t, handler, opcode)?))
}
pub(crate) fn framed_salt_for<'a>(t: &Tables<'a>, handler: Node<'a>, opcode: u8) -> Result<u32> {
    let functions: HashMap<_, _> = children(
        handler
            .parent()
            .ok_or_else(|| Error::new("INVALID_DATA", "Framed handler owner"))?,
    )
    .into_iter()
    .filter(|n| n.kind() == "function_declaration")
    .map(|n| Ok((t.text(child(n, "name")?).to_string(), n)))
    .collect::<Result<_>>()?;
    let mut salts = HashSet::new();
    for call in t
        .walk(handler, true)
        .into_iter()
        .filter(|n| n.kind() == "call_expression" && t.text(*n).len() <= 300)
    {
        let arguments = child(call, "arguments")?;
        let mut operands = children(arguments);
        for array in t
            .walk(arguments, true)
            .into_iter()
            .filter(|n| n.kind() == "array")
        {
            operands.extend(children(array))
        }
        if !operands
            .into_iter()
            .any(|n| integer(t, n).is_ok_and(|n| n as u32 == opcode as u32))
        {
            continue;
        }
        let names: HashSet<_> = t
            .walk(call, true)
            .into_iter()
            .filter(|n| n.kind() == "identifier" && functions.contains_key(t.text(*n)))
            .map(|n| t.text(n))
            .collect();
        for name in names {
            let helper = functions[name];
            for n in t.walk(helper, true).into_iter().filter(|n| {
                n.kind() == "augmented_assignment_expression"
                    && n.child_by_field_name("operator")
                        .is_some_and(|n| t.text(n) == "^=")
            }) {
                let left = child(n, "left")?;
                if left.kind() != "identifier" {
                    continue;
                }
                let Ok(salt) = integer(t, child(n, "right")?) else {
                    continue;
                };
                let name = t.text(left);
                let mut multiply = 0;
                for assignment in t.walk(helper, true).into_iter().filter(|n| {
                    n.kind() == "assignment_expression"
                        && n.child_by_field_name("left")
                            .is_some_and(|n| t.text(n) == name)
                }) {
                    let rhs = child(assignment, "right")?;
                    let unknown = unknowns(t, rhs, &[]);
                    let scope = t.scope(rhs);
                    if [1u32, 17, 0xfedcba98].iter().all(|value| {
                        let mut scope = scope.clone();
                        for name in &unknown {
                            scope.insert(name.clone(), Constant::Number(0.));
                        }
                        scope.insert(name.into(), Constant::Number(*value as f64));
                        t.eval(rhs, &mut scope)
                            .and_then(|v| v.number())
                            .is_ok_and(|n| n.trunc() as i64 as u32 == value.wrapping_mul(16777619))
                    }) {
                        multiply += 1
                    }
                }
                if multiply >= 4 {
                    salts.insert(salt as u32);
                }
            }
        }
    }
    require(salts.len() == 1, "Framed checksum salt ambiguity")?;
    Ok(*salts.iter().next().unwrap())
}
