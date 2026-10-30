use super::*;

fn projection<'a>(
    t: &Tables<'a>,
    expression: Node<'a>,
    reads: &[Node<'a>],
    f: &Format,
    key: u8,
    bytes: &[u8],
) -> Result<u32> {
    let mut expression = unwrap(expression);
    while expression.kind() == "sequence_expression" {
        expression = unwrap(
            *children(expression)
                .last()
                .ok_or_else(|| Error::new("UNSUPPORTED_FORMAT", "Empty boundary sequence"))?,
        );
    }
    let names = unknowns(t, expression, reads);
    require(names.len() == 1, "Boundary projection key")?;
    let fields: Vec<_> = t
        .walk(expression, true)
        .into_iter()
        .filter(|n| {
            matches!(n.kind(), "member_expression" | "subscript_expression")
                && !reads.iter().any(|r| {
                    n.start_byte() <= r.start_byte() && r.end_byte() <= n.end_byte()
                        || r.start_byte() <= n.start_byte() && n.end_byte() <= r.end_byte()
                })
                && !unknowns(t, *n, &[]).is_disjoint(&names)
        })
        .collect();
    let fields: Vec<_> = fields
        .iter()
        .copied()
        .filter(|n| {
            !fields.iter().any(|p| {
                p.id() != n.id() && p.start_byte() <= n.start_byte() && n.end_byte() <= p.end_byte()
            })
        })
        .collect();
    require(
        fields
            .iter()
            .map(|n| t.text(*n))
            .collect::<HashSet<_>>()
            .len()
            <= 1,
        "Boundary state key ownership",
    )?;
    let mut scope = t.scope(expression);
    scope.insert(
        names.into_iter().next().unwrap(),
        Constant::Number(key as f64),
    );
    Ok(t.bound(
        expression,
        &mut scope,
        reads
            .iter()
            .zip(bytes)
            .map(|(n, b)| {
                (
                    n.id(),
                    Constant::Number(b.wrapping_add(f.byte_offset) as f64),
                )
            })
            .chain(
                fields
                    .iter()
                    .map(|n| (n.id(), Constant::Number(key as f64))),
            )
            .collect(),
    )?
    .integer()? as u32)
}
fn address_slots<'a>(t: &Tables<'a>, reads: &[Node<'a>], f: &Format) -> Result<Vec<usize>> {
    let expression = child(owner(reads[0]), "right")?;
    require(
        reads.len() == 3
            && reads.iter().all(|n| {
                expression.start_byte() <= n.start_byte() && n.end_byte() <= expression.end_byte()
            }),
        "Boundary address read ownership",
    )?;
    require(
        projection(t, expression, reads, f, 0, &[0; 3])? == 0,
        "Boundary address base",
    )?;
    let mut weights = Vec::new();
    for i in 0..3 {
        let mut bytes = [0; 3];
        bytes[i] = 1;
        weights.push(projection(t, expression, reads, f, 0, &bytes)?);
    }
    let mut sorted = weights.clone();
    sorted.sort_unstable();
    require(sorted == [1, 256, 65536], "Boundary address packing")?;
    let slots: Vec<_> = [65536, 256, 1]
        .iter()
        .map(|v| weights.iter().position(|w| w == v).unwrap())
        .collect();
    for (key, bytes) in [(17, [3, 29, 209]), (255, [81, 0, 1])] {
        let b = bytes.map(|b| b ^ key);
        let expected = (b[slots[0]] as u32) << 16 | (b[slots[1]] as u32) << 8 | b[slots[2]] as u32;
        require(
            projection(t, expression, reads, f, key, &bytes)? == expected,
            "Boundary address transform",
        )?;
    }
    Ok(slots)
}
pub(super) fn jump(t: &Tables<'_>, f: &Format) -> Result<Value> {
    let (handler, opcode) = handler(t, 0)?;
    jump_in(t, handler, opcode, f)
}
pub(super) fn jump_in<'a>(
    t: &Tables<'a>,
    handler: Node<'a>,
    opcode: u8,
    f: &Format,
) -> Result<Value> {
    let reads = ordered_reads(t, handler, 3, f, false)?;
    let slots = address_slots(t, &reads, f)?;
    let object = t.text(child(reads[0], "object")?);
    let pointer = t.text(child(child(reads[0], "index")?, "argument")?);
    let trailing: Vec<_> = t
        .walk(handler, true)
        .into_iter()
        .filter(|n| {
            n.kind() == "subscript_expression"
                && child(*n, "object").is_ok_and(|n| t.text(n) == object)
                && child(*n, "index").is_ok_and(|n| t.text(n) == pointer)
        })
        .collect();
    require(trailing.len() == 1, "Boundary jump key read")?;
    let expression = child(owner(trailing[0]), "right")?;
    let mask = projection(t, expression, &trailing, f, 0, &[0])?;
    require(mask <= 255, "Boundary jump key mask")?;
    for (key, byte) in [(17, 3), (255, 81)] {
        require(
            projection(t, expression, &trailing, f, key, &[byte])? == (key ^ byte) as u32 ^ mask,
            "Boundary jump key transform",
        )?;
    }
    Ok(json!([opcode,[0,0,0,mask],{"b16":slots[0],"b8":slots[1],"b0":slots[2],"key":3}]))
}
pub(super) fn fallthrough(t: &Tables<'_>, f: &Format) -> Result<Value> {
    let (handler, opcode) = handler(t, 1)?;
    fallthrough_in(t, handler, opcode, f)
}
pub(super) fn fallthrough_in<'a>(
    t: &Tables<'a>,
    handler: Node<'a>,
    opcode: u8,
    f: &Format,
) -> Result<Value> {
    let reads = ordered_reads(t, handler, 5, f, false)?;
    let slots = address_slots(t, &reads[1..4], f)?;
    let (key, mask) = operand(t, reads[4], f)?;
    let target = t.text(child(owner(reads[1]), "left")?);
    let owned = t
        .walk(handler, true)
        .into_iter()
        .filter(|n| matches!(n.kind(), "if_statement" | "ternary_expression"))
        .any(|node| {
            ["consequence", "alternative"]
                .into_iter()
                .filter_map(|field| node.child_by_field_name(field))
                .any(|body| {
                    let names: HashSet<_> = t
                        .walk(body, true)
                        .into_iter()
                        .filter(|n| n.kind() == "assignment_expression")
                        .filter_map(|n| child(n, "right").ok().map(|n| t.text(n)))
                        .collect();
                    names.contains(target) && names.contains(key.as_str())
                })
        });
    require(owned, "Conditional fallthrough ownership")?;
    Ok(
        json!([opcode,vec![0;5],{"target":slots.iter().map(|i|i+1).collect::<Vec<_>>(),"key":4,"keyMask":mask}]),
    )
}

pub(super) fn empty_literals(t: &Tables<'_>, f: &Format) -> Result<Vec<u8>> {
    let (handler, _) = handler(t, 10)?;
    empty_literals_in(t, handler, f)
}
pub(super) fn empty_literals_in<'a>(
    t: &Tables<'a>,
    handler: Node<'a>,
    f: &Format,
) -> Result<Vec<u8>> {
    let mut tags = HashSet::new();
    for node in t
        .walk(handler, true)
        .into_iter()
        .filter(|n| n.kind() == "if_statement")
    {
        for (field, consequence) in [("consequence", true), ("alternative", false)] {
            let Some(body) = node.child_by_field_name(field) else {
                continue;
            };
            let rows = t.walk(body, true);
            if !raw_reads(body).is_empty()
                || rows.iter().any(|n| {
                    matches!(
                        n.kind(),
                        "if_statement"
                            | "for_statement"
                            | "while_statement"
                            | "do_statement"
                            | "new_expression"
                            | "throw_statement"
                    ) || n.kind() == "call_expression"
                        && t.value(*n).is_err()
                        && !["+", "-", "^", "&", "|", "<<", ">>", ">>>"]
                            .iter()
                            .any(|op| operation(t, *n, op).is_some())
                })
            {
                continue;
            }
            let mut condition = unwrap(child(node, "condition")?);
            while condition.kind() == "sequence_expression" {
                condition = unwrap(*children(condition).last().unwrap());
            }
            let mut negated = !consequence;
            while condition.kind() == "unary_expression"
                && t.text(child(condition, "operator")?) == "!"
            {
                negated = !negated;
                condition = unwrap(child(condition, "argument")?);
            }
            let Some(pair) = operation(t, condition, if negated { "!==" } else { "===" }) else {
                continue;
            };
            for [left, right] in [pair, [pair[1], pair[0]]] {
                if unwrap(left).kind() != "identifier" {
                    continue;
                }
                let Ok(tag) = integer(t, right) else { continue };
                if !(0..=255).contains(&tag) {
                    continue;
                }
                let masks: HashSet<_> = raw_reads(handler)
                    .into_iter()
                    .filter(|r| r.end_byte() < node.start_byte())
                    .filter_map(|r| operand(t, r, f).ok())
                    .filter(|(name, _)| name == t.text(unwrap(left)))
                    .map(|(_, mask)| mask)
                    .collect();
                if masks.len() == 1 {
                    tags.insert(tag as u8 ^ *masks.iter().next().unwrap());
                }
            }
        }
    }
    require(!tags.is_empty(), "No audited empty literal branches")?;
    let mut tags: Vec<_> = tags.into_iter().collect();
    tags.sort_unstable();
    Ok(tags)
}

pub(super) fn branch(t: &Tables<'_>, f: &Format) -> Result<Value> {
    let (handler, opcode) = handler(t, 4)?;
    branch_in(t, handler, opcode, f)
}
pub(super) fn branch_in<'a>(
    t: &Tables<'a>,
    handler: Node<'a>,
    opcode: u8,
    f: &Format,
) -> Result<Value> {
    let reads = ordered_reads(t, handler, 12, f, false)?;
    let first = address_slots(t, &reads[3..6], f)?;
    let second = address_slots(t, &reads[8..11], f)?;
    let (first_key, first_mask) = operand(t, reads[6], f)?;
    let (flag, flag_mask) = operand(t, reads[7], f)?;
    let (second_key, second_mask) = operand(t, reads[11], f)?;
    let first_target = t.text(child(owner(reads[3]), "left")?);
    let second_target = t.text(child(owner(reads[8]), "left")?);
    let mut validated = false;
    for n in t
        .walk(handler, true)
        .into_iter()
        .filter(|n| matches!(n.kind(), "if_statement" | "ternary_expression"))
    {
        let consequence = child(n, "consequence")?;
        let Some(alternative) = n.child_by_field_name("alternative") else {
            continue;
        };
        for (primary, secondary) in [(consequence, alternative), (alternative, consequence)] {
            let first_assignments: HashMap<_, _> = t
                .walk(primary, true)
                .into_iter()
                .filter(|n| n.kind() == "assignment_expression")
                .filter_map(|n| {
                    Some((
                        t.text(child(n, "right").ok()?),
                        t.text(child(n, "left").ok()?),
                    ))
                })
                .collect();
            let nested = t
                .walk(secondary, true)
                .into_iter()
                .filter(|n| n.kind() == "ternary_expression")
                .find_map(|n| {
                    let mut condition = unwrap(child(n, "condition").ok()?);
                    let mut negated = false;
                    while condition.kind() == "unary_expression"
                        && t.text(child(condition, "operator").ok()?) == "!"
                    {
                        negated = !negated;
                        condition = unwrap(child(condition, "argument").ok()?);
                    }
                    (t.text(condition) == flag)
                        .then(|| {
                            n.child_by_field_name(if negated {
                                "alternative"
                            } else {
                                "consequence"
                            })
                        })
                        .flatten()
                });
            let Some(nested) = nested else { continue };
            let second_assignments: HashMap<_, _> = t
                .walk(nested, true)
                .into_iter()
                .filter(|n| n.kind() == "assignment_expression")
                .filter_map(|n| {
                    Some((
                        t.text(child(n, "right").ok()?),
                        t.text(child(n, "left").ok()?),
                    ))
                })
                .collect();
            if first_assignments
                .get(first_target)
                .is_some_and(|a| second_assignments.get(second_target) == Some(a))
                && first_assignments
                    .get(first_key.as_str())
                    .is_some_and(|a| second_assignments.get(second_key.as_str()) == Some(a))
            {
                validated = true;
                break;
            }
        }
        if validated {
            break;
        }
    }
    require(validated, "Branch target and key ownership")?;
    Ok(
        json!([opcode,vec![0;12],{"target":first.iter().map(|i|i+3).collect::<Vec<_>>(),"key":6,"keyMask":first_mask,"flag":7,"flagMask":flag_mask,"alternateTarget":second.iter().map(|i|i+8).collect::<Vec<_>>(),"alternateKey":11,"alternateKeyMask":second_mask}]),
    )
}

pub(super) fn fixed_apply(t: &Tables<'_>, f: &Format) -> Result<Value> {
    fixed(t, f, 30, 4)
}
pub(super) fn fixed(t: &Tables<'_>, f: &Format, index: usize, count: usize) -> Result<Value> {
    let (handler, opcode) = handler(t, index)?;
    let reads = ordered_reads(t, handler, count, f, true)?;
    let masks = reads
        .into_iter()
        .map(|n| operand(t, n, f).map(|v| v.1))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!([opcode, masks, {}]))
}
pub(super) fn function_reference(t: &Tables<'_>, f: &Format) -> Result<Value> {
    let (node, opcode) = handler(t, 24)?;
    let reads = ordered_reads(t, node, 5, f, true)?;
    address_slots(t, &reads[1..4], f)?;
    Ok(json!([opcode, vec![0; 5], {}]))
}
pub(super) fn property_pool(t: &Tables<'_>, f: &Format) -> Result<Value> {
    let (node, opcode) = handler(t, 41)?;
    property_pool_in(t, node, opcode, f)
}
pub(super) fn property_pool_in<'a>(
    t: &Tables<'a>,
    node: Node<'a>,
    opcode: u8,
    f: &Format,
) -> Result<Value> {
    let mut groups = HashMap::<_, Vec<Node<'a>>>::new();
    for read in raw_reads(node) {
        groups
            .entry((
                t.text(child(read, "object")?),
                t.text(child(read, "index")?),
            ))
            .or_default()
            .push(read);
    }
    let mut matches = 0;
    for branch in t
        .walk(node, true)
        .into_iter()
        .filter(|n| n.kind() == "if_statement")
    {
        let mut condition = unwrap(child(branch, "condition")?);
        while condition.kind() == "sequence_expression" {
            condition = unwrap(*children(condition).last().unwrap());
        }
        let names = unknowns(t, condition, &[]);
        if names.len() != 1 {
            continue;
        }
        let name = names.into_iter().next().unwrap();
        let evaluate = |value| -> Result<bool> {
            let mut scope = t.scope(condition);
            scope.insert(name.clone(), Constant::Number(value));
            t.eval(condition, &mut scope)?.truth()
        };
        let Ok(pool) = evaluate(127.) else { continue };
        if ![0., 1., 126., 128.]
            .into_iter()
            .all(|v| evaluate(v) == Ok(!pool))
        {
            continue;
        }
        let Some(body) =
            branch.child_by_field_name(if pool { "consequence" } else { "alternative" })
        else {
            continue;
        };
        for reads in groups.values().filter(|r| (4..=32).contains(&r.len())) {
            let Ok(reads) = outer_case_reads(t, node, reads.len(), f) else {
                continue;
            };
            if reads
                .iter()
                .filter(|r| body.start_byte() <= r.start_byte() && r.end_byte() <= body.end_byte())
                .count()
                == 1
                && reads
                    .iter()
                    .rposition(|r| {
                        branch.start_byte() <= r.start_byte() && r.end_byte() <= branch.end_byte()
                    })
                    .is_some_and(|last| last + 3 == reads.len())
                && reads.iter().any(|r| r.end_byte() < condition.start_byte())
            {
                matches += 1;
            }
        }
    }
    require(matches == 1, "Property pool boundary")?;
    Ok(json!([opcode,[],{"marker":127,"width":5}]))
}
// Only recover the byte extent of this declaration. Its callback is never run.
pub(super) fn closure(t: &Tables<'_>, f: &Format) -> Result<Value> {
    let (node, opcode) = handler(t, 20)?;
    closure_in(t, node, opcode, f)
}
pub(super) fn byte_helper<'a>(t: &Tables<'a>, node: Node<'a>, f: &Format) -> Result<()> {
    let reads = raw_reads(node);
    require(reads.len() == 1, "Closure byte helper width")?;
    let returns: Vec<_> = t
        .walk(node, false)
        .into_iter()
        .filter(|n| n.kind() == "return_statement")
        .collect();
    require(returns.len() == 1, "Closure byte helper return")?;
    let mut expression = unwrap(
        returns[0]
            .named_child(0)
            .ok_or_else(|| Error::new("UNSUPPORTED_FORMAT", "Byte helper return"))?,
    );
    while expression.kind() == "sequence_expression" {
        expression = unwrap(*children(expression).last().unwrap());
    }
    let read = reads[0];
    require(
        t.text(child(child(read, "index")?, "operator")?) == "++",
        "Byte helper advancement",
    )?;
    for (k, b) in [(0u8, 0u8), (17, 39), (255, 81)] {
        require(
            projection(t, expression, &reads, f, k, &[b])? == (k ^ b) as u32,
            "Byte helper transform",
        )?;
    }
    Ok(())
}
pub(super) fn closure_in<'a>(
    t: &Tables<'a>,
    node: Node<'a>,
    opcode: u8,
    f: &Format,
) -> Result<Value> {
    let reads = ordered_reads(t, node, 3, f, false)?;
    address_slots(t, &reads, f)?;
    let functions: HashMap<_, _> = children(
        node.parent()
            .ok_or_else(|| Error::new("UNSUPPORTED_FORMAT", "Closure owner"))?,
    )
    .into_iter()
    .filter(|n| n.kind() == "function_declaration")
    .filter_map(|n| Some((t.text(child(n, "name").ok()?), n)))
    .collect();
    let mut candidates = Vec::new();
    let state = t.text(
        child(node, "parameters")?
            .named_child(0)
            .ok_or_else(|| Error::new("UNSUPPORTED_FORMAT", "Closure state input"))?,
    );
    for (name, helper) in functions {
        if byte_helper(t, helper, f).is_err() {
            continue;
        }
        let mut calls: Vec<_> = t
            .walk(node, true)
            .into_iter()
            .filter(|n| {
                if n.kind() != "call_expression" {
                    return false;
                }
                let Ok(function) = child(*n, "function") else {
                    return false;
                };
                let Ok(args) = child(*n, "arguments") else {
                    return false;
                };
                let args = children(args);
                (t.text(function) == name && args.len() == 1 && t.text(args[0]) == state)
                    || (args.len() == 2 && t.text(args[0]) == name && t.text(args[1]) == state)
                    || (args.len() == 3
                        && t.text(args[1]) == name
                        && (t.text(args[2]) == state
                            || (args[2].kind() == "array"
                                && children(args[2]).len() == 1
                                && t.text(children(args[2])[0]) == state))
                        && integer(t, args[0]).is_ok())
            })
            .collect();
        if calls.len() != 5 {
            continue;
        }
        let execution = super::execution_order(
            t,
            node,
            calls.iter().chain(&reads).copied().collect(),
            false,
        )?;
        let ranks = execution
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id(), i))
            .collect::<HashMap<_, _>>();
        calls.sort_by_key(|n| ranks[&n.id()]);
        require(
            ranks[&calls[0].id()] < ranks[&reads[0].id()]
                && ranks[&reads[2].id()] < ranks[&calls[1].id()],
            "Closure header order",
        )?;
        let loops: Vec<_> = t
            .walk(node, true)
            .into_iter()
            .filter(|n| {
                matches!(
                    n.kind(),
                    "for_statement" | "while_statement" | "do_statement"
                ) && n.start_byte() < calls[4].start_byte()
                    && calls[4].end_byte() < n.end_byte()
                    && !calls[..4].iter().chain(&reads).any(|header| {
                        n.start_byte() < header.start_byte() && header.end_byte() < n.end_byte()
                    })
            })
            .collect();
        require(
            loops.len() == 1 && loops[0].kind() == "for_statement",
            "Closure capture loop",
        )?;
        let loop_node = loops[0];
        require(
            ranks[&calls[3].id()] < ranks[&calls[4].id()]
                && loop_node.start_byte() < calls[4].start_byte()
                && calls[4].end_byte() < loop_node.end_byte(),
            "Closure capture input ownership",
        )?;
        let count_owner = owner(calls[3]);
        let count = t.text(child(count_owner, "left")?);
        let mut update = unwrap(child(loop_node, "increment")?);
        while update.kind() == "sequence_expression" {
            update = unwrap(*children(update).last().unwrap());
        }
        require(
            update.kind() == "update_expression" && t.text(child(update, "operator")?) == "++",
            "Closure capture increment",
        )?;
        let index = t.text(child(update, "argument")?);
        let initial = child(loop_node, "initializer")?;
        let init: Vec<_> = t
            .walk(initial, true)
            .into_iter()
            .filter(|n| n.kind() == "variable_declarator")
            .collect();
        require(
            init.len() == 1
                && t.text(child(init[0], "name")?) == index
                && integer(t, child(init[0], "value")?)? == 0,
            "Closure capture start",
        )?;
        let condition = child(loop_node, "condition")?;
        for (i, n) in [(0, 0), (0, 2), (1, 2), (2, 2), (3, 2)] {
            let mut scope = t.scope(condition);
            scope.insert(index.into(), Constant::Number(i as f64));
            scope.insert(count.into(), Constant::Number(n as f64));
            require(
                t.eval(condition, &mut scope)?.truth()? == (i < n),
                "Closure capture bound",
            )?;
        }
        let expression = child(count_owner, "right")?;
        let mask = t
            .bound(
                expression,
                &mut t.scope(expression),
                HashMap::from([(calls[3].id(), Constant::Number(0.))]),
            )?
            .integer()?;
        require((0..=255).contains(&mask), "Closure capture mask")?;
        for b in [1u8, 17, 255] {
            require(
                t.bound(
                    expression,
                    &mut t.scope(expression),
                    HashMap::from([(calls[3].id(), Constant::Number(b as f64))]),
                )?
                .integer()?
                    == (b ^ mask as u8) as i64,
                "Closure capture transform",
            )?;
        }
        candidates.push(json!([opcode,vec![0;7],{"count":6,"countMask":mask,"maxCount":64}]));
    }
    require(candidates.len() == 1, "Closure byte reader ownership")?;
    Ok(candidates.pop().unwrap())
}
pub(super) fn shared_binary(t: &Tables<'_>, f: &Format) -> Result<Vec<(String, Value)>> {
    let mut entries = Vec::new();
    let mut shared = None;
    for index in 46..=65 {
        let (node, opcode) = handler(t, index)?;
        if let Some(id) = shared {
            require(id == node.id(), "Binary handler sharing")?;
        } else {
            binary_groups(t, node, f, 20)?;
            shared = Some(node.id());
        }
        entries.push((
            format!("flow-binary-{index}"),
            json!([opcode, [0, 0, 0], {}]),
        ));
    }
    Ok(entries)
}
pub(super) fn binary_groups<'a>(
    t: &Tables<'a>,
    handler: Node<'a>,
    f: &Format,
    count: usize,
) -> Result<()> {
    uniform_groups(t, handler, f, count, 3)
}
pub(super) fn shared_unary(t: &Tables<'_>, f: &Format) -> Result<Vec<(String, Value)>> {
    let mut entries = Vec::new();
    let mut shared = None;
    for index in 66..=70 {
        let Ok((node, opcode)) = handler(t, index) else {
            continue;
        };
        if let Some(id) = shared {
            require(id == node.id(), "Unary handler sharing")?;
        } else {
            uniform_groups(t, node, f, 5, 2)?;
            shared = Some(node.id());
        }
        entries.push((format!("flow-unary-{index}"), json!([opcode, [0, 0], {}])));
    }
    require(entries.len() >= 4, "Unary opcode coverage")?;
    Ok(entries)
}
pub(super) fn uniform_groups<'a>(
    t: &Tables<'a>,
    handler: Node<'a>,
    f: &Format,
    count: usize,
    width: usize,
) -> Result<()> {
    let reads = ordered_reads(t, handler, count * width, f, false)?;
    let mut groups = HashMap::new();
    for read in reads {
        let mut node = read;
        loop {
            let group = raw_reads(node);
            if group.len() >= width {
                require(group.len() == width, "Nonuniform operation input width")?;
                for read in group {
                    operand(t, read, f)?;
                }
                groups.insert(node.id(), node);
                break;
            }
            node = node
                .parent()
                .ok_or_else(|| Error::new("UNSUPPORTED_FORMAT", "Binary input ownership"))?;
            require(
                handler.start_byte() <= node.start_byte() && node.end_byte() <= handler.end_byte(),
                "Binary input boundary",
            )?;
        }
    }
    require(groups.len() == count, "Binary operation count")?;
    let groups: Vec<_> = groups.into_values().collect();
    let contains = |outer: Node<'_>, inner: Node<'_>| {
        outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
    };
    for (i, a) in groups.iter().enumerate() {
        for b in &groups[i + 1..] {
            let mut node = a.parent();
            let mut exclusive = false;
            while let Some(n) = node {
                if matches!(n.kind(), "ternary_expression" | "if_statement") {
                    if let (Some(c), Some(d)) = (
                        n.child_by_field_name("consequence"),
                        n.child_by_field_name("alternative"),
                    ) {
                        if contains(c, *a) && contains(d, *b) || contains(d, *a) && contains(c, *b)
                        {
                            exclusive = true;
                            break;
                        }
                    }
                }
                if n.id() == handler.id() {
                    break;
                }
                node = n.parent();
            }
            require(exclusive, "Binary reads are not mutually exclusive")?;
        }
    }
    Ok(())
}
