//! Recognize a local, write-through memo of a finite position expression.
use crate::{
    require,
    syntax::constants::{child, children, unwrap, Tables},
    Result,
};
use tree_sitter::Node;
pub(super) fn read<'a>(
    t: &Tables<'a>,
    _function: Node<'a>,
    region: Node<'a>,
    params: &[Node<'a>],
) -> Result<Option<(Node<'a>, Node<'a>)>> {
    let nodes = t.walk(region, false);
    let mut found = Vec::new();
    for n in &nodes {
        if n.kind() != "binary_expression"
            || !child(*n, "operator").is_ok_and(|op| t.text(op) == "||")
        {
            continue;
        }
        let local = unwrap(child(*n, "left")?);
        let sequence = unwrap(child(*n, "right")?);
        let Some(local_ref) = super::data_reference(t, local) else {
            continue;
        };
        if sequence.kind() != "sequence_expression" {
            continue;
        }
        let parts = children(sequence);
        if parts.len() != 2 || parts.iter().any(|n| n.kind() != "assignment_expression") {
            continue;
        }
        if super::data_reference(t, child(parts[0], "left")?).as_ref() != Some(&local_ref)
            || super::data_reference(t, child(parts[1], "right")?).as_ref() != Some(&local_ref)
        {
            continue;
        }
        let slot = unwrap(child(parts[1], "left")?);
        if slot.kind() != "subscript_expression" {
            continue;
        }
        let table = child(slot, "object")?;
        let Some(table_ref) = super::data_reference(t, table) else {
            continue;
        };
        if t.text(unwrap(child(slot, "index")?)) != t.text(params[2]) {
            continue;
        }
        let same_slot = |n: Node<'a>| {
            n.kind() == "subscript_expression"
                && child(n, "object")
                    .is_ok_and(|o| super::data_reference(t, o).as_ref() == Some(&table_ref))
                && child(n, "index").is_ok_and(|i| t.text(unwrap(i)) == t.text(params[2]))
        };
        let writes: Vec<_> = nodes
            .iter()
            .copied()
            .filter(|n| {
                matches!(
                    n.kind(),
                    "assignment_expression"
                        | "augmented_assignment_expression"
                        | "variable_declarator"
                ) && child(
                    *n,
                    if n.kind() == "variable_declarator" {
                        "name"
                    } else {
                        "left"
                    },
                )
                .is_ok_and(|l| super::data_reference(t, l).as_ref() == Some(&local_ref))
                    && n.child_by_field_name(if n.kind() == "variable_declarator" {
                        "value"
                    } else {
                        "right"
                    })
                    .is_some()
            })
            .collect();
        if writes.len() != 2 || !writes.contains(&parts[0]) {
            continue;
        }
        let initial = writes.iter().copied().find(|n| *n != parts[0]).unwrap();
        let read = unwrap(child(
            initial,
            if initial.kind() == "variable_declarator" {
                "value"
            } else {
                "right"
            },
        )?);
        if !same_slot(read)
            || !nodes.iter().any(|n| {
                n.kind() == "variable_declarator"
                    && child(*n, "name")
                        .is_ok_and(|name| t.text(name) == local_ref.split('[').next().unwrap())
            })
        {
            continue;
        }
        let accesses: Vec<_> = nodes
            .iter()
            .filter(|n| {
                n.kind() == "subscript_expression"
                    && child(**n, "object")
                        .is_ok_and(|o| super::data_reference(t, o).as_ref() == Some(&table_ref))
            })
            .collect();
        if accesses.len() != 2 || accesses.iter().any(|n| !same_slot(**n)) {
            continue;
        }
        let fresh = child(parts[0], "right")?;
        if t.walk(fresh, true).iter().any(|n| {
            matches!(
                n.kind(),
                "assignment_expression"
                    | "augmented_assignment_expression"
                    | "update_expression"
                    | "new_expression"
            ) || n.kind() == "identifier"
                && (t.text(*n) == t.text(params[0]) || t.text(*n) == t.text(params[1]))
        }) {
            continue;
        }
        found.push((read, fresh));
    }
    require(found.len() <= 1, "Ambiguous opcode position cache")?;
    Ok(found.pop())
}
