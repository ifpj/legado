//! Recover the current protocol's finite data formats from its syntax.
use crate::{
    program::data::{program, Format, Plan},
    re, require,
    syntax::constants::{child, children, unwrap, Constant, Tables},
    Error, Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Parser};
mod character;
mod opcode_cache;
mod sealed;
#[cfg(test)]
mod transform_tests;
mod wire_calls;

pub fn integer<'a>(t: &Tables<'a>, n: Node<'a>) -> Result<i64> {
    let value = t.value(n)?.number()?;
    require(
        value.is_finite() && value.abs() < (1u64 << 53) as f64,
        "Static integer bounds",
    )?;
    Ok(value.trunc() as i64)
}
fn arithmetic(op: &str, a: i64, b: i64) -> Result<i64> {
    Ok(match op {
        "+" => a + b,
        "-" => a - b,
        "*" => a * b,
        "&" => a & b,
        "^" => a ^ b,
        "|" => a | b,
        ">>>" => ((a as u32) >> (b as u32 & 31)) as i64,
        ">>" => ((a as i32) >> (b as u32 & 31)) as i64,
        "<<" => (a as i32).wrapping_shl(b as u32 & 31) as i64,
        ">=" => i64::from(a >= b),
        "<" => i64::from(a < b),
        "===" => i64::from(a == b),
        "!==" => i64::from(a != b),
        _ => {
            return Err(Error::new(
                "UNKNOWN_CONSTANT",
                "Unsupported format operation",
            ))
        }
    })
}
pub fn operation<'a>(t: &Tables<'a>, n: Node<'a>, op: &str) -> Option<[Node<'a>; 2]> {
    let n = unwrap(n);
    if n.kind() == "binary_expression" && child(n, "operator").is_ok_and(|n| t.text(n) == op) {
        return Some([child(n, "left").ok()?, child(n, "right").ok()?]);
    }
    if n.kind() != "call_expression" {
        return None;
    }
    let args = children(child(n, "arguments").ok()?);
    if !(2..=8).contains(&args.len()) {
        return None;
    }
    let fn_node = child(n, "function").ok()?;
    let evaluated = t.value(fn_node);
    let f = evaluated.ok()?;
    let prefix = args[..args.len() - 2]
        .iter()
        .map(|n| t.value(*n))
        .collect::<Result<Vec<_>>>()
        .ok()?;
    let operands = &args[args.len() - 2..];
    let invoke = |a: i64, b: i64| {
        let mut inputs = prefix.clone();
        inputs.extend([Constant::Number(a as f64), Constant::Number(b as f64)]);
        t.invoke(f.clone(), inputs, 1).and_then(|v| {
            if let Constant::Bool(v) = v {
                Ok(i64::from(v))
            } else {
                v.integer()
            }
        })
    };
    let samples = [(3, 5), (7, 2), (255, 16), (-2147483648, 1)];
    if samples
        .iter()
        .all(|(a, b)| invoke(*a, *b).ok() == arithmetic(op, *a, *b).ok())
    {
        Some([operands[0], operands[1]])
    } else if samples
        .iter()
        .all(|(a, b)| invoke(*a, *b).ok() == arithmetic(op, *b, *a).ok())
    {
        Some([operands[1], operands[0]])
    } else {
        None
    }
}
pub(crate) fn script_build(t: &Tables<'_>) -> Option<String> {
    let mut values = HashSet::new();
    for call in t
        .all()
        .iter()
        .copied()
        .into_iter()
        .filter(|n| n.kind() == "call_expression")
    {
        let Some((receiver, args)) = wire_calls::method(t, call, "toString", 1).ok().flatten()
        else {
            continue;
        };
        if args.len() != 1 || t.value(args[0]).and_then(|v| v.integer()) != Ok(16) {
            continue;
        }
        if let Some(value) = t
            .value(receiver)
            .ok()
            .and_then(|v| v.integer().ok())
            .filter(|n| *n >= 0 && *n < (1i64 << 48))
        {
            values.insert(format!("{value:012x}"));
        }
    }
    if values.len() == 1 {
        values.into_iter().next()
    } else {
        None
    }
}
pub(crate) fn data_reference<'a>(t: &Tables<'a>, node: Node<'a>) -> Option<String> {
    let node = unwrap(node);
    if node.kind() == "identifier" {
        return Some(t.text(node).to_string());
    }
    if node.kind() == "subscript_expression" {
        let object = child(node, "object").ok()?;
        if object.kind() != "identifier" {
            return None;
        }
        let index = integer(t, child(node, "index").ok()?).ok()?;
        if (0..65536).contains(&index) {
            return Some(format!("{}[{index}]", t.text(object)));
        }
    }
    None
}

fn call_tail(mut value: Node<'_>) -> Option<Node<'_>> {
    value = unwrap(value);
    while value.kind() == "sequence_expression" {
        value = unwrap(*children(value).last()?);
    }
    (value.kind() == "call_expression").then_some(value)
}
pub(crate) fn transform_name(t: &Tables<'_>) -> Result<String> {
    let left_field = |n: Node<'_>| {
        if n.kind() == "variable_declarator" {
            "name"
        } else {
            "left"
        }
    };
    let right_field = |n: Node<'_>| {
        if n.kind() == "variable_declarator" {
            "value"
        } else {
            "right"
        }
    };
    let mut candidates = HashSet::new();
    let assignments = t
        .all()
        .iter()
        .copied()
        .filter(|n| {
            matches!(n.kind(), "assignment_expression" | "variable_declarator")
                && n.child_by_field_name(right_field(*n))
                    .is_some_and(|r| call_tail(r).is_some())
        })
        .collect::<Vec<_>>();
    let mut writes = HashMap::<String, Vec<Node<'_>>>::new();
    for assignment in &assignments {
        if let Some(reference) = data_reference(t, child(*assignment, left_field(*assignment))?) {
            writes.entry(reference).or_default().push(*assignment);
        }
    }
    let mut slices = HashMap::new();
    for node in assignments {
        let left = child(node, left_field(node))?;
        let Some(right) = call_tail(child(node, right_field(node))?) else {
            continue;
        };
        let Some(target_reference) = data_reference(t, left) else {
            continue;
        };
        let mut owner = node.parent();
        while owner
            .is_some_and(|n| !matches!(n.kind(), "function_declaration" | "function_expression"))
        {
            owner = owner.unwrap().parent();
        }
        let Some(owner) = owner else { continue };
        let body = child(owner, "body")?;
        let mut slice_candidates = Vec::new();
        for assignment in writes
            .get(&target_reference)
            .into_iter()
            .flatten()
            .copied()
            .filter(|n| n.start_byte() >= body.start_byte() && n.end_byte() <= body.end_byte())
        {
            let Some(value) = call_tail(child(assignment, right_field(assignment))?) else {
                continue;
            };
            let bounds = slices
                .entry(assignment.id())
                .or_insert_with(|| wire_calls::slice_bounds(t, value));
            match bounds {
                Ok(None) => (),
                _ => slice_candidates.push(bounds.clone()),
            }
        }
        // A helper requires a same-owner slice of this exact target. Reject
        // unrelated assignments before building their scopes and bindings.
        // Preserve any slice error until the helper itself is a candidate.
        if slice_candidates.is_empty() {
            continue;
        }
        let target = t.text(left);
        let args = children(child(right, "arguments")?);
        let function = child(right, "function")?;
        let helper = if args.len() == 1
            && function.kind() == "identifier"
            && data_reference(t, args[0]).as_ref() == Some(&target_reference)
        {
            Some(t.text(function).to_string())
        } else {
            let argument_nodes = args
                .iter()
                .flat_map(|n| t.walk(*n, true))
                .collect::<Vec<_>>();
            let references = argument_nodes
                .iter()
                .copied()
                .filter(|n| data_reference(t, *n).as_ref() == Some(&target_reference))
                .collect::<Vec<_>>();
            if references.is_empty() {
                continue;
            }
            let local = t.scope(right);
            let globals = t.globals.borrow();
            let identifiers = argument_nodes
                .iter()
                .copied()
                .filter(|n| n.kind() == "identifier")
                .map(|n| t.text(n))
                .collect::<HashSet<_>>();
            let unknown = identifiers
                .into_iter()
                .filter(|n| {
                    local
                        .get(*n)
                        .or_else(|| globals.get(*n))
                        .is_none_or(|v| matches!(v, Constant::Dynamic))
                        && !matches!(*n, "undefined" | "Number" | "String" | "Math" | "Reflect")
                        && *n != target
                        && !references.iter().any(|r| {
                            t.walk(*r, true)
                                .iter()
                                .any(|i| i.kind() == "identifier" && t.text(*i) == *n)
                        })
                })
                .collect::<HashSet<_>>();
            if unknown.len() == 1 {
                unknown.into_iter().next().map(String::from)
            } else {
                None
            }
        };
        let Some(helper) = helper.filter(|s| (1..=64).contains(&s.len())) else {
            continue;
        };
        for bounds in slice_candidates {
            let Some([start, end]) = bounds? else {
                continue;
            };
            let (start, end) = (unwrap(start), unwrap(end));
            let Some(start_reference) = data_reference(t, start) else {
                continue;
            };
            if [0., 17., 255.].into_iter().all(|v| {
                let mut env = t.scope(end);
                let overrides = t
                    .walk(end, true)
                    .into_iter()
                    .filter(|node| data_reference(t, *node).as_ref() == Some(&start_reference))
                    .map(|node| (node.id(), Constant::Number(v)))
                    .collect();
                t.bound(end, &mut env, overrides).and_then(|n| n.number()) == Ok(v + 16.)
            }) {
                candidates.insert(helper.clone());
            }
        }
    }
    require(candidates.len() == 1, "No unique wire-key transform")?;
    Ok(candidates.into_iter().next().unwrap())
}
fn polynomial<'a>(t: &Tables<'a>, value: Node<'a>, name: &str) -> Result<(u8, u32, u32, u32)> {
    let mut steps = HashSet::new();
    let mut aliases = HashSet::new();
    for node in t.walk(value, true) {
        if let Some(pair) = operation(t, node, "+") {
            for [a, b] in [pair, [pair[1], pair[0]]] {
                if data_reference(t, a).as_deref() == Some(name) {
                    if let Ok(v) = integer(t, b) {
                        steps.insert(v);
                    }
                }
            }
        }
    }
    if steps.is_empty() {
        let mut owner = value.parent();
        while owner
            .is_some_and(|n| !matches!(n.kind(), "function_declaration" | "function_expression"))
        {
            owner = owner.unwrap().parent();
        }
        if let Some(owner) = owner {
            let nodes = t.walk(child(owner, "body")?, false);
            for reference in t.walk(value, true) {
                let Some(reference) = data_reference(t, reference) else {
                    continue;
                };
                let writes = nodes
                    .iter()
                    .filter(|n| {
                        n.end_byte() <= value.start_byte()
                            && matches!(n.kind(), "assignment_expression" | "variable_declarator")
                            && n.child_by_field_name(if n.kind() == "variable_declarator" {
                                "name"
                            } else {
                                "left"
                            })
                            .is_some_and(|l| data_reference(t, l).as_ref() == Some(&reference))
                            && n.child_by_field_name(if n.kind() == "variable_declarator" {
                                "value"
                            } else {
                                "right"
                            })
                            .is_some()
                    })
                    .collect::<Vec<_>>();
                if writes.len() != 1 {
                    continue;
                }
                let rhs = child(
                    *writes[0],
                    if writes[0].kind() == "variable_declarator" {
                        "value"
                    } else {
                        "right"
                    },
                )?;
                let Some(pair) = operation(t, rhs, "+") else {
                    continue;
                };
                for [a, b] in [pair, [pair[1], pair[0]]] {
                    if data_reference(t, a).as_deref() == Some(name) {
                        if let Ok(step) = integer(t, b) {
                            steps.insert(step);
                            aliases.insert(reference.clone());
                        }
                    }
                }
            }
        }
    }
    let references = |root: Node<'a>| {
        t.walk(root, true)
            .into_iter()
            .filter_map(|n| {
                if data_reference(t, n).as_deref() == Some(name) {
                    Some((n.id(), false))
                } else if data_reference(t, n).is_some_and(|reference| aliases.contains(&reference))
                {
                    Some((n.id(), true))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    let overrides_for = |references: &[(usize, bool)], x: i64, step: i64| {
        references
            .iter()
            .map(|(id, alias)| {
                (
                    *id,
                    Constant::Number(if *alias { x } else { x - step } as f64),
                )
            })
            .collect()
    };
    require(
        steps.len() == 1,
        &format!(
            "State polynomial opcode ambiguity: {} / {} / {:?}",
            name,
            t.text(value),
            steps
        ),
    )?;
    let step = *steps.iter().next().unwrap();
    let scope = t.scope(value);
    let value_references = references(value);
    let mut values = Vec::new();
    for x in 0..256i64 {
        let mut scope = scope.clone();
        scope.insert(name.into(), Constant::Number((x - step) as f64));
        let overrides = overrides_for(&value_references, x, step);
        values.push(t.bound(value, &mut scope, overrides)?.integer()?)
    }
    let c = values[0];
    let difference = (values[2] - 2 * values[1] + c) & 255;
    require(difference & 1 == 0, "State polynomial parity")?;
    let q = difference / 2;
    let m = (values[1] - c - q) & 255;
    require(
        values
            .iter()
            .enumerate()
            .all(|(i, v)| *v == (q * i as i64 * i as i64 + m * i as i64 + c) & 255),
        "State polynomial verification",
    )?;
    // Recover the original coefficients when the source retains them. A
    // reduced polynomial is byte-equivalent, but loses layout metadata.
    if let Some(pair) = operation(t, value, "&") {
        for [unmasked, mask] in [pair, [pair[1], pair[0]]] {
            if integer(t, mask) != Ok(255) {
                continue;
            }
            let unmasked_references = references(unmasked);
            let evaluate = |x: i64| {
                let mut env = scope.clone();
                env.insert(name.into(), Constant::Number((x - step) as f64));
                let overrides = overrides_for(&unmasked_references, x, step);
                t.bound(unmasked, &mut env, overrides)?.integer()
            };
            if let (Ok(v0), Ok(v1), Ok(v2)) = (evaluate(0), evaluate(1), evaluate(2)) {
                let full_q = (v2 - 2 * v1 + v0) / 2;
                let full_m = v1 - v0 - full_q;
                if full_q == 0
                    && [full_m, v0]
                        .iter()
                        .all(|v| (0..=u32::MAX as i64).contains(v))
                    && (0..8).all(|x| evaluate(x) == Ok(full_q * x * x + full_m * x + v0))
                {
                    return Ok((step as u8, full_m as u32, v0 as u32, full_q as u32));
                }
            }
        }
    }
    Ok((step as u8, m as u32, c as u32, q as u32))
}
pub(crate) fn format(t: &Tables<'_>) -> Result<Format> {
    let mut reasons = Vec::new();
    let candidates = t
        .all()
        .iter()
        .copied()
        .into_iter()
        .filter(|n| {
            n.kind() == "function_declaration"
            && (t.text(*n).contains("2654435769") || t.text(*n).contains("return[]"))
            // Cloudflare occasionally ships a larger obfuscated dispatcher
            // around the same finite recipe.  Current scripts also moved the
            // unrelated `return[]` marker outside the format reader, so the
            // arithmetic constant is the stable prefilter.  Keep a bound to
            // avoid scanning an unbounded bootstrap.
            && t.text(*n).len() < 1_048_576
        })
        .collect::<Vec<_>>();
    let candidate_info = candidates
        .iter()
        .map(|function| {
            json!({
                "name": child(*function, "name").map(|name| t.text(name)).unwrap_or(""),
                "bytes": t.text(*function).len(),
                "hasEmptyReturn": t.text(*function).contains("return[]"),
            })
        })
        .collect::<Vec<_>>();
    for function in candidates {
        let attempt = (|| -> Result<Format> {
            let params = children(child(function, "parameters")?);
            require(!params.is_empty(), "Reader parameter")?;
            let param_names: HashSet<_> = params
                .iter()
                .filter(|n| n.kind() == "identifier")
                .map(|n| t.text(*n))
                .collect();
            let mut reader = None;
            for arrow in t.walk(function, true).into_iter().filter(|n| {
                matches!(
                    n.kind(),
                    "arrow_function" | "function_expression" | "function_declaration"
                )
            }) {
                for expr in t.walk(child(arrow, "body")?, false) {
                    let Some(pair) = operation(t, expr, "-") else {
                        continue;
                    };
                    let left = unwrap(pair[0]);
                    if left.kind() != "subscript_expression"
                        || !param_names.contains(t.text(child(left, "object")?))
                    {
                        continue;
                    }
                    let Ok(offset) = integer(t, pair[1]) else {
                        continue;
                    };
                    let mut returned = expr;
                    while returned
                        .parent()
                        .is_some_and(|p| p.kind() != "return_statement")
                    {
                        returned = returned.parent().unwrap()
                    }
                    returned = unwrap(returned);
                    if returned.kind() == "sequence_expression" {
                        returned = unwrap(*children(returned).last().unwrap())
                    }
                    if let Some(pair) = operation(t, returned, "^") {
                        if let Some(state) = pair
                            .into_iter()
                            .map(unwrap)
                            .find(|n| data_reference(t, *n).is_some())
                        {
                            let reader_name = arrow
                                .parent()
                                .filter(|p| p.kind() == "variable_declarator")
                                .and_then(|parent| child(parent, "name").ok())
                                .map(|name| t.text(name).to_string())
                                .or_else(|| {
                                    (arrow.kind() == "function_declaration")
                                        .then(|| child(arrow, "name").ok())
                                        .flatten()
                                        .map(|name| t.text(name).to_string())
                                });
                            if let Some(reader_name) = reader_name {
                                let mut region = function;
                                let mut ancestor = arrow.parent();
                                while let Some(node) = ancestor {
                                    if node == function {
                                        break;
                                    }
                                    if node.kind() == "switch_case" {
                                        let writes = t
                                            .walk(node, false)
                                            .iter()
                                            .filter(|n| {
                                                matches!(
                                                    n.kind(),
                                                    "assignment_expression" | "variable_declarator"
                                                ) && n
                                                    .child_by_field_name(
                                                        if n.kind() == "assignment_expression" {
                                                            "left"
                                                        } else {
                                                            "name"
                                                        },
                                                    )
                                                    .is_some_and(|target| {
                                                        data_reference(t, target)
                                                            == data_reference(t, state)
                                                    })
                                            })
                                            .count();
                                        if writes >= 2 {
                                            region = node;
                                            break;
                                        }
                                    }
                                    ancestor = node.parent();
                                }
                                reader = Some((
                                    reader_name,
                                    data_reference(t, state).unwrap(),
                                    offset,
                                    region,
                                ));
                                break;
                            }
                        }
                    }
                }
                if reader.is_some() {
                    break;
                }
            }
            let (reader, state, offset, region) =
                reader.ok_or_else(|| Error::new("UNKNOWN_CONSTANT", "Data reader identity"))?;
            require((0..=255).contains(&offset), "Data byte offset")?;
            let mut seeds = HashSet::new();
            let mut transition = None;
            let mut transition_errors = Vec::new();
            for node in t.walk(region, false) {
                if !matches!(node.kind(), "assignment_expression" | "variable_declarator") {
                    continue;
                }
                let target = child(
                    node,
                    if node.kind() == "assignment_expression" {
                        "left"
                    } else {
                        "name"
                    },
                )?;
                if data_reference(t, target).as_ref() != Some(&state) {
                    continue;
                }
                let Some(value) =
                    node.child_by_field_name(if node.kind() == "assignment_expression" {
                        "right"
                    } else {
                        "value"
                    })
                else {
                    continue;
                };
                let reads_state = t
                    .walk(value, true)
                    .iter()
                    .any(|n| data_reference(t, *n).as_ref() == Some(&state));
                if let Ok(value) = integer(t, value).and_then(|v| {
                    require(!reads_state, "State transition is not a seed")?;
                    Ok(v)
                }) {
                    seeds.insert(value);
                } else {
                    match polynomial(t, value, &state) {
                        Ok(profile) => transition = Some(profile),
                        Err(error) => transition_errors.push(format!(
                            "{}; expression={}; state={}",
                            error.message,
                            t.text(value),
                            state
                        )),
                    }
                }
            }
            require(
                seeds.len() == 1 && transition.is_some(),
                &format!(
                    "Data seed/transition: seeds={}, transition={}, errors={:?}",
                    seeds.len(),
                    transition.is_some(),
                    transition_errors
                ),
            )?;
            let mut salts = HashSet::new();
            let changed_names: HashSet<_> = t
                .walk(region, true)
                .into_iter()
                .filter_map(|node| {
                    let name = match node.kind() {
                        "update_expression" => node.child_by_field_name("argument"),
                        "augmented_assignment_expression" => node.child_by_field_name("left"),
                        _ => None,
                    }?;
                    data_reference(t, name)
                })
                .collect();
            for node in t.walk(region, true) {
                if let Some(pair) = operation(t, node, "^") {
                    for [a, b] in [pair, [pair[1], pair[0]]] {
                        if let Some(reference) = data_reference(t, b) {
                            if integer(t, b).is_err() || changed_names.contains(&reference) {
                                if let Ok(v) = integer(t, a) {
                                    if (65536..(1i64 << 32)).contains(&v) && v != 2654435769 {
                                        salts.insert(v);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            require(
                salts.len() == 1,
                &format!("Character hash salt ambiguity: {:?}", salts),
            )?;
            let mut xors = HashSet::new();
            let mut after = false;
            for arrow in t.walk(region, true).into_iter().filter(|n| {
                matches!(
                    n.kind(),
                    "arrow_function" | "function_expression" | "function_declaration"
                )
            }) {
                for expr in t.walk(child(arrow, "body")?, false) {
                    if let Some(pair) = operation(t, expr, "^") {
                        for [a, b] in [pair, [pair[1], pair[0]]] {
                            if t.walk(a, true)
                                .iter()
                                .any(|n| n.kind() == "identifier" && t.text(*n) == reader)
                            {
                                if let Ok(v) = integer(t, b) {
                                    if (0..=255).contains(&v)
                                        && !t
                                            .walk(b, true)
                                            .into_iter()
                                            .any(|node| operation(t, node, ">>>").is_some())
                                    {
                                        xors.insert(v);
                                    }
                                }
                                if t.walk(a, true)
                                    .iter()
                                    .any(|n| operation(t, *n, "-").is_some())
                                    && t.walk(b, true)
                                        .iter()
                                        .any(|n| operation(t, *n, ">>>").is_some())
                                {
                                    after = true;
                                }
                            }
                        }
                    }
                }
            }
            require(
                xors.len() == 1,
                &format!("Character XOR ambiguity: {xors:?}"),
            )?;
            {
                let orders = t
                    .walk(region, true)
                    .into_iter()
                    .filter(|n| {
                        matches!(
                            n.kind(),
                            "arrow_function" | "function_expression" | "function_declaration"
                        )
                    })
                    .filter_map(|function| {
                        character::order(t, function, &reader, *xors.iter().next().unwrap() as u8)
                    })
                    .collect::<HashSet<_>>();
                require(orders.len() <= 1, "Character byte order ambiguity")?;
                if let Some(order) = orders.iter().next() {
                    after = *order == "after-subtract";
                }
            }
            let (step, mult, increment, q) = transition.unwrap();
            Ok(Format {
                byte_offset: offset as u8,
                initial_key: *seeds.iter().next().unwrap() as u8,
                state_step: step,
                state_multiplier: mult,
                state_increment: increment,
                state_quadratic: q,
                hash_salt: *salts.iter().next().unwrap() as u32,
                char_xor: *xors.iter().next().unwrap() as u8,
                char_mode: if after {
                    "after-subtract"
                } else {
                    "before-subtract"
                }
                .into(),
                opcode_multiplier: None,
            })
        })();
        match attempt {
            Ok(value) => return Ok(value),
            Err(error) => reasons.push(error.message),
        }
    }
    Err(Error {
        details: json!({"candidateErrors":reasons,"candidates":candidate_info}),
        ..Error::new("MANAGED_FORMAT_UNSUPPORTED", "未能静态恢复当前数据格式")
    })
}
fn opcode_encoding(t: &Tables<'_>, format: &Format) -> Result<Option<u32>> {
    let mut models = HashSet::new();
    let mut attempts = Vec::new();
    let mut candidates = Vec::new();
    for function in t.all().iter().copied().filter(|node| {
        node.kind() == "function_declaration" && node.end_byte() - node.start_byte() < 12000
    }) {
        let params = children(child(function, "parameters")?);
        if !params.iter().all(|p| p.kind() == "identifier") {
            continue;
        }
        let body = child(function, "body")?;
        if params.len() == 3 {
            candidates.push((function, body, params, Vec::new()));
            continue;
        }
        if params.len() != 4 {
            continue;
        }
        for switch in t
            .walk(body, false)
            .into_iter()
            .filter(|n| n.kind() == "switch_statement")
        {
            let Some(pair) = operation(t, unwrap(child(switch, "value")?), "^") else {
                continue;
            };
            let offset = if t.text(unwrap(pair[0])) == t.text(params[0]) {
                integer(t, pair[1]).ok()
            } else if t.text(unwrap(pair[1])) == t.text(params[0]) {
                integer(t, pair[0]).ok()
            } else {
                None
            };
            let Some(offset) = offset else {
                continue;
            };
            for case in children(child(switch, "body")?)
                .into_iter()
                .filter(|n| n.kind() == "switch_case")
            {
                let Ok(label) = integer(t, child(case, "value")?) else {
                    continue;
                };
                candidates.push((
                    function,
                    case,
                    params[1..].to_vec(),
                    vec![Constant::Number(((label as i32) ^ (offset as i32)) as f64)],
                ));
            }
        }
    }
    for (function, region, params, prefix) in candidates {
        if params.len() != 3 {
            continue;
        }
        let nodes = t.walk(region, false);
        let input = t.text(params[0]);
        let key = t.text(params[1]);
        let has_xor = nodes.iter().any(|n| {
            n.kind() == "augmented_assignment_expression"
                && child(*n, "left").is_ok_and(|n| t.text(n) == input)
                && child(*n, "operator").is_ok_and(|n| t.text(n) == "^=")
        });
        let has_mask = nodes.iter().any(|n| {
            n.kind() == "augmented_assignment_expression"
                && child(*n, "left").is_ok_and(|n| t.text(n) == key)
                && child(*n, "operator").is_ok_and(|n| t.text(n) == "&=")
                && child(*n, "right").is_ok_and(|n| integer(t, n) == Ok(7))
        });
        if !has_xor || !has_mask {
            continue;
        }
        let multipliers: HashSet<_> = nodes
            .iter()
            .filter_map(|n| operation(t, *n, "*"))
            .flat_map(|pair| [pair, [pair[1], pair[0]]])
            .filter(|pair| t.text(unwrap(pair[0])) == key)
            .filter_map(|pair| integer(t, pair[1]).ok())
            .filter(|v| (0..=u32::MAX as i64).contains(v))
            .collect();
        if multipliers.len() != 1 {
            attempts.push("Opcode multiplier ambiguity".to_string());
            continue;
        }
        let multiplier = *multipliers.iter().next().unwrap() as u32;
        let mut candidate = format.clone();
        candidate.opcode_multiplier = Some(multiplier);
        let cache = opcode_cache::read(t, function, region, &params)?;
        let mut valid = true;
        'samples: for position in [0, 1, 17, 2354, 65535] {
            let overrides = if let Some((read, fresh)) = cache {
                let mut env = t.scope(fresh);
                env.insert(t.text(params[2]).into(), Constant::Number(position as f64));
                let value = t.eval(fresh, &mut env)?;
                require(
                    value.truth()? && value.integer().is_ok(),
                    "Opcode cache value",
                )?;
                std::collections::HashMap::from([(read.id(), value)])
            } else {
                std::collections::HashMap::new()
            };
            for key in [0, 1, 9, 127, 255] {
                for value in [0, 1, 63, 128, 255] {
                    let expected = candidate.opcode(
                        (value as u8 ^ key as u8).wrapping_add(candidate.byte_offset),
                        key as u8,
                        position,
                    );
                    let mut args = prefix.clone();
                    args.extend([
                        Constant::Number(value as f64),
                        Constant::Number(key as f64),
                        Constant::Number(position as f64),
                    ]);
                    let actual = t
                        .invoke_bound(function, args, overrides.clone())
                        .and_then(|v| v.integer());
                    if actual != Ok(expected as i64) {
                        attempts.push(format!("Opcode finite model: {actual:?}"));
                        valid = false;
                        break 'samples;
                    }
                }
            }
        }
        if valid {
            models.insert(multiplier);
        }
    }
    if models.is_empty() && attempts.is_empty() {
        return Ok(None);
    }
    require(
        models.len() == 1,
        &format!("Opcode transform unsupported: {attempts:?}"),
    )?;
    Ok(models.into_iter().next())
}

#[cfg(test)]
pub(crate) fn key_material(t: &Tables<'_>, encoded: &str, f: &Format) -> Result<Value> {
    // Every helper candidate is a current source identifier, and must have
    // a verified adjacent 16-byte literal under the rolling instruction key.
    let name = transform_name(t)?;
    recover_material(encoded, f, &HashSet::from([name.as_str()]))
}
pub(crate) fn recover_material(encoded: &str, f: &Format, names: &HashSet<&str>) -> Result<Value> {
    let raw = STANDARD.decode(encoded)?;
    let p = program(&raw, f, false)?;
    let short = |position, key, len| short_literal(&raw, position, key, f, len);
    let mut found = Vec::new();
    let mut helper_matches = 0;
    let mut mask_matches = 0;
    let mut lengths = names
        .iter()
        .map(|name| name.len())
        .filter(|n| (1..=64).contains(n))
        .collect::<Vec<_>>();
    lengths.sort_unstable();
    lengths.dedup();
    for position in 3..p.code_length.saturating_sub(20) {
        for &len in &lengths {
            let key = f.byte(raw[position], len as u8);
            let Some(name) = short(position, key, len).and_then(|n| String::from_utf8(n).ok())
            else {
                continue;
            };
            if !names.contains(name.as_str()) {
                continue;
            }
            helper_matches += 1;
            let next = position + len + 1;
            if next + 20 > p.code_length {
                continue;
            }
            let opcode = f.opcode(raw[next], key, next);
            let next_key = f.key(key, opcode);
            let Some(mask) = short(next + 3, next_key, 16) else {
                continue;
            };
            if !mask.iter().all(|c| (33..=126).contains(c)) {
                continue;
            }
            mask_matches += 1;
            for slot in 1..=2 {
                let tag = f.byte(raw[position - 3 + slot], key);
                if tag == f.byte(raw[next + slot], next_key) {
                    found.push(json!({"helper":name,"mask":mask,"program_sha256":p.sha256,"literal_offset":next+3,"literal_tag":tag,"literal_kind_slot":slot}))
                }
            }
        }
    }
    if found.len() != 1 {
        return Err(Error {
            details: json!({"helperMatches":helper_matches,"maskMatches":mask_matches,"matches":found.len(),"found":found.iter().map(|v|json!({"helper":v["helper"],"offset":v["literal_offset"]})).collect::<Vec<_>>(),"codeLength":p.code_length,"format":f}),
            ..Error::new(
                "UNSUPPORTED_FORMAT",
                "Adjacent key mask missing or ambiguous",
            )
        });
    }
    Ok(found.remove(0))
}
pub fn compile(source: &str, frame: bool) -> Result<Plan> {
    let prepared = crate::syntax::literal_strings::prepare_for_compile(source)?;
    compile_prepared(prepared.source.as_ref(), frame, prepared.tree())
}
pub(crate) fn compile_prepared(
    source: &str,
    frame: bool,
    previous: Option<tree_sitter::Tree>,
) -> Result<Plan> {
    crate::cpu::check()?;
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .map_err(|e| Error::new("PARSER_ERROR", e.to_string()))?;
    let tree = crate::cpu::parse_reusing(&mut parser, source, previous.as_ref())?;
    require(
        !tree.root_node().has_error(),
        "Challenge syntax unsupported",
    )?;
    let tables = Tables::new(tree.root_node(), source)?;
    let values = tables.strings();
    let mut f = format(&tables)?;
    f.opcode_multiplier = opcode_encoding(&tables, &f)?;
    let one = |pattern: &str| -> Result<String> {
        let found: HashSet<_> = values
            .iter()
            .filter(|v| re(pattern).is_match(v))
            .cloned()
            .collect();
        require(found.len() == 1, "Wire parameter ambiguous")?;
        Ok(found.into_iter().next().unwrap())
    };
    let alphabet = one(r"^[A-Za-z0-9+$-]{65}$")?;
    let modulus = one(r"^0x[0-9a-fA-F]{258}$")?;
    let flow = one(r"^/fo/[0-9]+:[0-9]+:[A-Za-z0-9_-]+/$")?;
    let mut material = Vec::new();
    let mut programs = Vec::new();
    let mut material_errors = Vec::new();
    let mut checked_programs = HashSet::new();
    let mut helper = None;
    for value in &values {
        crate::cpu::check()?;
        if value.len() > 64
            && checked_programs.insert(value.as_str())
            && re(r"^[A-Za-z0-9+/]+={0,2}$").is_match(value)
        {
            if let Ok(raw) = STANDARD.decode(value) {
                if let Ok(p) = program(&raw, &f, false) {
                    let recovered = match helper.get_or_insert_with(|| transform_name(&tables)) {
                        Ok(name) => recover_material(value, &f, &HashSet::from([name.as_str()])),
                        Err(error) => Err(error.clone()),
                    };
                    match recovered {
                        Ok(k) => {
                            if !material
                                .iter()
                                .any(|m: &Value| m["program_sha256"] == k["program_sha256"])
                            {
                                material.push(k);
                                programs.push(json!(p));
                            }
                        }
                        Err(error) => {
                            if material_errors.len() < 16 {
                                material_errors.push(error.reply());
                            }
                        }
                    }
                }
            }
        }
    }
    if material.len() != 1 {
        return Err(Error {
            details: json!({"candidates":material.len(),"errors":material_errors,"format":f}),
            ..Error::new("UNSUPPORTED_FORMAT", "Initializer key source ambiguous")
        });
    }
    let build = script_build(&tables)
        .ok_or_else(|| Error::new("UNSUPPORTED_FORMAT", "Protocol build missing"))?;
    let mut meta = json!({"format":serde_json::to_value(&f)?,"key_material":material.remove(0),"programs":programs,"build":build,"bootstrap":null,"dispatcher_profile":null,"property_profile":null,"immediate_profile":null,"input_data_profile":null,"integer_input_tag":null,"declared_properties":null});
    if frame {
        use crate::syntax::compiler_profiles as profiles;
        meta["dispatcher_profile"] = profiles::dispatcher(&tables, &f)?;
        meta["property_profile"] = profiles::properties(&tables, &f)?;
        meta["immediate_profile"] = profiles::immediate(&tables, &f)?;
        meta["input_data_profile"] = profiles::inputs(&tables, &f)?;
        meta["boundaryProfileVersion"] = json!(4);
        if let Ok((opcode, salt)) = profiles::framed_salt(&tables) {
            meta["input_data_profile"]["flow-call"] = json!([opcode,[],{"salt":salt}]);
        }
        meta["integer_input_tag"] = json!(profiles::integer_tag(&tables, &f)?);
        meta["input_data_profile"]["flow-dispatch"] =
            json!([meta["dispatcher_profile"]["opcode"], vec![0; 25], {}]);
        meta["declared_properties"] = declarations(&tables)?;
    }
    crate::cpu::check()?;
    let mut plan = Plan {
        format: f,
        meta,
        alphabet,
        modulus,
        flow,
    };
    if !frame && plan.meta["build"] == "c787ce48cf6b" {
        plan.meta["sealed_literals"] = sealed::profile(&tables)?;
        let encoded = values
            .iter()
            .find(|s| {
                STANDARD.decode(s).is_ok_and(|raw| {
                    crate::tasks::models::sha(raw)
                        == plan.meta["key_material"]["program_sha256"]
                            .as_str()
                            .unwrap_or("")
                })
            })
            .ok_or_else(|| Error::new("UNSUPPORTED_FORMAT", "Bootstrap program absent"))?;
        let response = crate::program::data::Response::read(encoded, &plan, false)?;
        let names = response
            .literals
            .iter()
            .map(|l| l.value.as_str())
            .collect::<HashSet<_>>();
        let missing = [
            "prototype",
            "iframe",
            "sandbox",
            "allow-same-origin",
            "toString",
            "get",
            "configurable",
            "enumerable",
        ]
        .iter()
        .filter(|v| !names.contains(**v))
        .collect::<Vec<_>>();
        require(missing.is_empty(), "Bootstrap declaration signature")?;
        let assignments = response
            .literals
            .windows(2)
            .filter(|pair| {
                pair[0].value == "TRcGA3"
                    && pair[0].end == pair[1].start
                    && re(r"^[A-Za-z]{4,7}[0-9]$").is_match(&pair[1].value)
            })
            .collect::<Vec<_>>();
        require(
            assignments.len() == 2 && assignments[0][1].value != assignments[1][1].value,
            "Bootstrap declaration marker ambiguity",
        )?;
        plan.meta["bootstrapFlags"] = json!({"TRcGA3":assignments[1][1].value});
        // This protocol's bootstrap declares the JSON conversion marker in
        // its first small return function, immediately after the eval name.
        // Recover its current literal rather than borrowing browser state.
        let marker = response
            .literals
            .windows(2)
            .filter(|pair| {
                pair[0].value == "eval" && re(r"^[A-Za-z]{4,7}[0-9]$").is_match(&pair[1].value)
            })
            .collect::<Vec<_>>();
        require(
            marker.len() == 1 && names.contains("GesyF4") && names.contains("toJSON"),
            "Bootstrap JSON declaration",
        )?;
        plan.meta["bootstrapFlags"]["GesyF4"] = json!(marker[0][1].value);
    }
    Ok(plan)
}
pub(crate) fn current_sealed(source: &str, previous: Option<&tree_sitter::Tree>) -> Result<Value> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .map_err(|e| Error::new("PARSER_ERROR", e.to_string()))?;
    let tree = crate::cpu::parse_reusing(&mut parser, source, previous)?;
    require(
        !tree.root_node().has_error(),
        "Challenge syntax unsupported",
    )?;
    let syntax = Tables::syntax(tree.root_node(), source)?;
    if let Ok(profile) = sealed::profile(&syntax) {
        return Ok(profile);
    }
    sealed::profile(&Tables::new(tree.root_node(), source)?)
}
/// Reads the sealed key from the original syntax tree without the decoded
/// literal pass or the complete constant recovery fallback.
pub(crate) fn current_sealed_original(source: &str, tree: &tree_sitter::Tree) -> Result<Value> {
    require(
        !tree.root_node().has_error(),
        "Challenge syntax unsupported",
    )?;
    sealed::profile(&Tables::syntax(tree.root_node(), source)?)
}
pub(crate) fn constant_json(v: Constant<'_>) -> Option<Value> {
    match v {
        Constant::Number(n) => serde_json::Number::from_f64(n).map(Value::Number),
        Constant::Text(s) => Some(json!(s)),
        Constant::Bool(b) => Some(json!(b)),
        Constant::Null => Some(Value::Null),
        Constant::Array(items) => items
            .iter()
            .cloned()
            .map(constant_json)
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        _ => None,
    }
}

pub(crate) fn short_literal(
    raw: &[u8],
    position: usize,
    key: u8,
    f: &Format,
    length: usize,
) -> Option<Vec<u8>> {
    if !(1..127).contains(&length)
        || position.checked_add(length)? >= raw.len()
        || f.byte(raw[position], key) as usize != length
    {
        return None;
    }
    let mut seed = f.seed(position + 1);
    Some(
        raw[position + 1..position + 1 + length]
            .iter()
            .map(|b| {
                let v = f.char(f.byte(*b, key), seed);
                seed = seed.wrapping_add(2654435769);
                v
            })
            .collect(),
    )
}

pub(crate) fn declarations(t: &Tables<'_>) -> Result<Value> {
    let assignments: Vec<_> = t
        .all()
        .iter()
        .copied()
        .into_iter()
        .filter(|n| n.kind() == "assignment_expression")
        .collect();
    let mut aliases = HashSet::from(["window".to_string(), "self".to_string()]);
    for node in &assignments {
        let left = unwrap(child(*node, "left")?);
        let right = unwrap(child(*node, "right")?);
        if left.kind() == "subscript_expression"
            && matches!(right.kind(), "function_expression" | "arrow_function")
        {
            if t.value(child(left, "index")?)
                .and_then(|v| v.text())
                .is_ok_and(|v| v == "vUAjb0")
            {
                aliases.insert(t.text(child(left, "object")?).into());
            }
        }
    }
    let mut declarations = serde_json::Map::new();
    for node in assignments {
        let left = unwrap(child(node, "left")?);
        let right = unwrap(child(node, "right")?);
        if !matches!(left.kind(), "member_expression" | "subscript_expression")
            || !aliases.contains(t.text(child(left, "object")?))
        {
            continue;
        }
        let key = if let Some(n) = left.child_by_field_name("property") {
            t.text(n).to_string()
        } else {
            let Ok(key) = t.value(child(left, "index")?).and_then(|v| v.text()) else {
                continue;
            };
            key
        };
        let value = match right.kind() {
            "function_expression" | "arrow_function" => Some(json!({"declarationKind":"N"})),
            "object" => Some(json!({"declarationKind":"o"})),
            _ => t.value(right).ok().and_then(constant_json),
        };
        if let Some(value) = value {
            declarations.insert(key, value);
        }
    }
    Ok(json!(declarations))
}

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn indexed_wire_assignments_preserve_owner_bounds_and_ambiguity() {
        let read = |source: &str| {
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_javascript::LANGUAGE.into())
                .unwrap();
            let tree = parser.parse(source, None).unwrap();
            transform_name(&Tables::new(tree.root_node(), source).unwrap())
        };
        let source = "function first(bytes,start){var key;key=bytes.slice(start,start+16);key=encode(key);}function other(bytes,start){var key;key=bytes.slice(start,start+17);key=unrelated(key);}";
        assert_eq!(read(source).unwrap(), "encode");
        assert_eq!(
            read(&source.replace(
                "key=bytes.slice(start,start+16)",
                "key=(startZero=0,bytes.slice(start,start+16))"
            ))
            .unwrap(),
            "encode"
        );
        assert!(read(&source.replace("start+17", "start+16")).is_err());
        assert!(read(&source.replace("start+16", "start+15")).is_err());
    }

    #[test]
    fn polynomial_array_aliases_use_the_definition_before_the_transition() {
        let source="function reader(){var cells=[0,0,0,59];cells[1]=cells[3]+1;cells[3]=(cells[1]*cells[1]*37+cells[1]*215+222)&255;cells[1]=0;}";
        let check = |source: &str| {
            let mut p = Parser::new();
            p.set_language(&tree_sitter_javascript::LANGUAGE.into())
                .unwrap();
            let tree = p.parse(source, None).unwrap();
            let tables = Tables::new(tree.root_node(), source).unwrap();
            let value = tables
                .all()
                .iter()
                .find(|n| {
                    n.kind() == "assignment_expression"
                        && child(**n, "left").is_ok_and(|n| tables.text(n) == "cells[3]")
                })
                .and_then(|n| n.child_by_field_name("right"))
                .unwrap();
            polynomial(&tables, value, "cells[3]")
        };
        assert_eq!(check(source).unwrap(), (1, 215, 222, 37));
        assert_eq!(
            check(&source.replace("cells[1]*cells[1]", "cells[0+1]*cells[1]")).unwrap(),
            (1, 215, 222, 37)
        );
        assert!(check(&source.replace("cells[3]+1", "cells[2]+1")).is_err());
        assert!(check(&source.replace("cells[3]=(cells", "cells[1]=99;cells[3]=(cells")).is_err());
    }

    #[test]
    fn position_hash_cache_recomputes_the_current_expression() {
        let hash="function hash(position){position^=1612195754;position=(position^(position>>>16))*40503|0;position=(position^(position>>>13))*13849|0;return position^(position>>>16)}";
        let body="function decode(value,key,position){var mask;var bias;var cached;return mask=255,bias=1,cached=table[position],cached||(cached=(hash(position)&mask)+bias,table[position]=cached),value^=(cached-bias^key*403)&mask,key&=7,(value>>>key|value<<8-key)&mask}";
        for (changed, valid) in [
            (body.to_string(), true),
            (
                body.replace("table[position]=cached", "table[key]=cached"),
                false,
            ),
            (body.replace("cached-bias^key", "cached+bias^key"), false),
            (
                body.replace("table[position]=cached", "table[position]=cached+1"),
                false,
            ),
            (
                body.replace("hash(position)&mask", "hash(position+1)&mask"),
                false,
            ),
        ] {
            let source = format!("{hash}{changed}");
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_javascript::LANGUAGE.into())
                .unwrap();
            let tree = parser.parse(&source, None).unwrap();
            let t = Tables::new(tree.root_node(), &source).unwrap();
            let f:Format=serde_json::from_value(json!({"byte_offset":37,"initial_key":191,"state_step":244,"state_multiplier":48294,"state_increment":31651,"hash_salt":1612195754u32,"char_xor":133,"state_quadratic":0,"char_mode":"before-subtract"})).unwrap();
            if valid {
                assert_eq!(opcode_encoding(&t, &f).unwrap(), Some(403));
            } else {
                assert!(opcode_encoding(&t, &f).is_err(), "{changed}");
            }
        }
    }
    #[test]
    fn position_key_opcode_model_is_verified_and_inverts_all_candidates() {
        let source="function hash(position){position^=1612195754;position=(position^(position>>>16))*40503|0;position=(position^(position>>>13))*13849|0;return position^(position>>>16)}function decode(value,key,position){value^=(hash(position)^key*403)&255;key&=7;return (value>>>key|value<<8-key)&255}";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let tables = Tables::new(tree.root_node(), source).unwrap();
        let mut f: Format=serde_json::from_value(json!({"byte_offset":37,"initial_key":191,"state_step":244,"state_multiplier":48294,"state_increment":31651,"hash_salt":1612195754u32,"char_xor":133,"state_quadratic":0,"char_mode":"before-subtract"})).unwrap();
        assert_eq!(opcode_encoding(&tables, &f).unwrap(), Some(403));
        f.opcode_multiplier = Some(403);
        for opcode in [0, 1, 64, 127, 244, 255] {
            let keys = f.instruction_keys(opcode);
            for position in [0, 1, 2354] {
                for key in 0..=255u8 {
                    let raw = (opcode.rotate_left((key & 7) as u32)
                        ^ (f.seed(position) ^ key as u32 * 403) as u8
                        ^ key)
                        .wrapping_add(f.byte_offset);
                    assert_eq!(f.opcode(raw, key, position), opcode);
                    assert!(keys
                        .at(&f, raw, position)
                        .contains(&(key, f.key(key, opcode))));
                }
            }
        }
        let changed = source.replace("8-key", "7-key");
        let tree = parser.parse(&changed, None).unwrap();
        let tables = Tables::new(tree.root_node(), &changed).unwrap();
        assert!(opcode_encoding(&tables, &f).is_err());
    }

    #[test]
    fn format_does_not_require_a_literal_hash_increment_or_arrow_reader() {
        for (reader, character) in [
            (
                "let next=()=>{return ((bytes[position++]-37)&255)^state};",
                "let character=()=>{return ((next()^133^(seed>>>16&255))-seed)&255};",
            ),
            (
                "function next(){return ((bytes[position++]-37)&255)^state}",
                "function character(){return ((next()^133^(seed>>>16&255))-seed)&255}",
            ),
            (
                "let next=function(){return ((bytes[position++]-37)&255)^state};",
                "let character=function(){return ((next()^133^(seed>>>16&255))-seed)&255};",
            ),
        ] {
            let source=format!("function constants(bytes,position){{let state=191;for(let entry=0;entry<1;entry++){{state=((state+244)*48294+31651)&255;{reader}let seed=position^1612195754;{character}}}return[]}}");
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_javascript::LANGUAGE.into())
                .unwrap();
            let tree = parser.parse(&source, None).unwrap();
            let tables = Tables::new(tree.root_node(), &source).unwrap();
            let recovered =
                format(&tables).unwrap_or_else(|error| panic!("{reader} {character}: {error:?}"));
            assert_eq!(recovered.byte_offset, 37);
            assert_eq!(recovered.initial_key, 191);
            assert_eq!(recovered.state_step, 244);
            assert_eq!(recovered.state_multiplier, 48294);
            assert_eq!(recovered.state_increment, 31651);
            assert_eq!(recovered.hash_salt, 1612195754);
            assert_eq!(recovered.char_xor, 133);
            assert_eq!(recovered.char_mode, "before-subtract");
        }
    }
}
