use crate::{
    constants::{child, children, unwrap, Constant, Tables},
    Result,
};
use tree_sitter::Node;
fn property<'a>(t: &Tables<'a>, n: Node<'a>) -> Option<String> {
    n.child_by_field_name("property")
        .map(|p| t.text(p).to_string())
        .or_else(|| {
            n.child_by_field_name("index")
                .and_then(|i| t.value(i).ok()?.text().ok())
        })
}
fn forwarder<'a>(t: &Tables<'a>, function: Node<'a>) -> Result<bool> {
    let params = children(child(function, "parameters")?);
    if !(3..=4).contains(&params.len()) || params.iter().any(|n| n.kind() != "identifier") {
        return Ok(false);
    }
    let body = children(child(function, "body")?);
    if body
        .iter()
        .any(|n| !matches!(n.kind(), "variable_declaration" | "return_statement"))
    {
        return Ok(false);
    }
    let returns: Vec<_> = body
        .iter()
        .filter(|n| n.kind() == "return_statement")
        .collect();
    if returns.len() != 1 {
        return Ok(false);
    }
    if body
        .iter()
        .filter(|n| n.kind() == "variable_declaration")
        .flat_map(|n| children(*n))
        .any(|n| n.child_by_field_name("value").is_some())
    {
        return Ok(false);
    }
    let expression = unwrap(returns[0].named_child(0).unwrap());
    let parts = if expression.kind() == "sequence_expression" {
        children(expression)
    } else {
        vec![expression]
    };
    let call = unwrap(*parts.last().unwrap());
    if call.kind() != "call_expression" {
        return Ok(false);
    }
    let outer = child(call, "function")?;
    if !matches!(outer.kind(), "member_expression" | "subscript_expression") {
        return Ok(false);
    }
    let inner = child(outer, "object")?;
    if !matches!(inner.kind(), "member_expression" | "subscript_expression") {
        return Ok(false);
    }
    let base = unwrap(child(inner, "object")?);
    if base.kind() != "function_expression" || !children(child(base, "body")?).is_empty() {
        return Ok(false);
    }
    let mut env = t.scope(expression);
    for (p, v) in params.iter().zip([7., 11., 13., 17.]) {
        env.insert(t.text(*p).to_string(), Constant::Number(v));
    }
    for part in &parts[..parts.len() - 1] {
        if t.eval(*part, &mut env).is_err() {
            return Ok(false);
        }
    }
    for member in [inner, outer] {
        let name = if let Some(p) = member.child_by_field_name("property") {
            Some(t.text(p).to_string())
        } else {
            member
                .child_by_field_name("index")
                .and_then(|i| t.eval(i, &mut env).ok()?.text().ok())
        };
        if name.as_deref() != Some("call") {
            return Ok(false);
        }
    }
    let args = children(child(call, "arguments")?);
    if args.len() != params.len() {
        return Ok(false);
    }
    Ok(args
        .iter()
        .zip([11, 7, 13, 17])
        .all(|(n, v)| t.eval(*n, &mut env).and_then(|c| c.integer()) == Ok(v)))
}
pub(super) fn slice_bounds<'a>(t: &Tables<'a>, call: Node<'a>) -> Result<Option<[Node<'a>; 2]>> {
    Ok(method(t, call, "slice", 2)?.map(|(_, args)| [args[0], args[1]]))
}
pub(super) fn method<'a>(
    t: &Tables<'a>,
    call: Node<'a>,
    name: &str,
    count: usize,
) -> Result<Option<(Node<'a>, Vec<Node<'a>>)>> {
    let callee = child(call, "function")?;
    let args = children(child(call, "arguments")?);
    if args.len() == count
        && matches!(callee.kind(), "member_expression" | "subscript_expression")
        && property(t, callee).as_deref() == Some(name)
    {
        return Ok(Some((child(callee, "object")?, args)));
    }
    if args.len() != count + 2
        || !matches!(args[1].kind(), "member_expression" | "subscript_expression")
        || property(t, args[1]).as_deref() != Some(name)
    {
        return Ok(None);
    }
    let Ok(Constant::Function(function)) = t.value(callee) else {
        return Ok(None);
    };
    if children(child(function, "parameters")?).len() != args.len() {
        return Ok(None);
    }
    if forwarder(t, function)? {
        Ok(Some((args[0], args[2..].to_vec())))
    } else {
        Ok(None)
    }
}
