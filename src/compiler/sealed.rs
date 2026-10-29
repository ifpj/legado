use super::*;
fn current_integer<'a>(t: &Tables<'a>, n: Node<'a>) -> Result<i64> {
    t.shallow(n).or_else(|_| t.value(n))?.integer()
}
fn current_reference<'a>(t: &Tables<'a>, n: Node<'a>) -> Option<String> {
    let n = unwrap(n);
    if n.kind() == "identifier" {
        return Some(t.text(n).to_owned());
    }
    if n.kind() == "subscript_expression" {
        let object = child(n, "object").ok()?;
        if object.kind() == "identifier" {
            let index = current_integer(t, child(n, "index").ok()?).ok()?;
            if (0..65536).contains(&index) {
                return Some(format!("{}[{index}]", t.text(object)));
            }
        }
    }
    None
}
fn same_reference<'a>(t: &Tables<'a>, n: Node<'a>, target: Node<'a>, reference: &str) -> bool {
    let (n, target) = (unwrap(n), unwrap(target));
    if n.kind() != target.kind() {
        return false;
    }
    if n.kind() == "identifier" {
        return t.text(n) == reference;
    }
    if n.kind() == "subscript_expression"
        && child(n, "object")
            .ok()
            .zip(child(target, "object").ok())
            .is_some_and(|(a, b)| t.text(a) == t.text(b))
    {
        return current_reference(t, n).as_deref() == Some(reference);
    }
    false
}
pub(super) fn profile(t: &Tables<'_>) -> Result<Value> {
    let mut choices = Vec::new();
    for function in t
        .all()
        .iter()
        .copied()
        .filter(|n| n.kind() == "function_declaration")
    {
        for array in t
            .walk(child(function, "body")?, false)
            .into_iter()
            .filter(|n| n.kind() == "array")
        {
            let items = children(array);
            if items.len() != 6 || items[5].kind() != "array" {
                continue;
            }
            if !matches!(
                items[0].kind(),
                "subscript_expression" | "member_expression"
            ) {
                continue;
            }
            let refs = children(items[5]);
            if refs.len() != 8 {
                continue;
            }
            let Some(cvnode) = refs
                .first()
                .filter(|n| n.kind() == "subscript_expression")
                .and_then(|n| n.child_by_field_name("object"))
            else {
                continue;
            };
            let Some(cv_reference) = current_reference(t, cvnode) else {
                continue;
            };
            if !refs.iter().enumerate().all(|(i, n)| {
                n.kind() == "subscript_expression"
                    && child(*n, "object")
                        .is_ok_and(|n| same_reference(t, n, cvnode, &cv_reference))
                    && child(*n, "index").is_ok_and(|n| current_integer(t, n) == Ok(i as i64))
            }) {
                continue;
            }
            let Ok(Constant::Array(values)) = t.shallow(cvnode).or_else(|_| t.value(cvnode)) else {
                continue;
            };
            let Some(iv) = values
                .iter()
                .map(|v| v.integer().ok().and_then(|n| u32::try_from(n).ok()))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            if iv.len() != 8 {
                continue;
            }
            let bindings = t
                .all()
                .iter()
                .filter(|n| {
                    matches!(n.kind(), "assignment_expression" | "variable_declarator")
                        && n.child_by_field_name(if n.kind() == "assignment_expression" {
                            "left"
                        } else {
                            "name"
                        })
                        .is_some_and(|n| same_reference(t, n, cvnode, &cv_reference))
                        && n.child_by_field_name(if n.kind() == "assignment_expression" {
                            "right"
                        } else {
                            "value"
                        })
                        .is_some_and(|n| n.kind() == "array")
                })
                // The obfuscator reuses short names in unrelated scopes.
                // Bind the declaration to this reference's resolved CV.
                .filter(|n| {
                    let field = if n.kind() == "assignment_expression" {
                        "right"
                    } else {
                        "value"
                    };
                    n.child_by_field_name(field)
                        .and_then(|rhs| t.shallow(rhs).or_else(|_| t.value(rhs)).ok())
                        .is_some_and(|v| {
                            if let Constant::Array(values) = v {
                                values.len() == iv.len()
                                    && values
                                        .iter()
                                        .zip(&iv)
                                        .all(|(a, b)| a.integer() == Ok(*b as i64))
                            } else {
                                false
                            }
                        })
                })
                .collect::<Vec<_>>();
            if bindings.len() != 1 {
                continue;
            }
            let signature = t.text(*bindings[0]);
            let v = json!({"iv":iv,"binding":signature,"version":1});
            if !choices.contains(&v) {
                choices.push(v);
            }
        }
    }
    require(
        choices.len() == 1,
        "Sealed literal key declaration ambiguity",
    )?;
    Ok(choices.remove(0))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn read(source: &str) -> Result<Value> {
        let mut p = Parser::new();
        p.set_language(&tree_sitter_javascript::LANGUAGE.into())
            .unwrap();
        let tree = p.parse(source, None).unwrap();
        profile(&Tables::new(tree.root_node(), source)?)
    }
    fn constructor(name: &str, key: &str) -> String {
        format!("function {name}(which,input){{switch(which){{case 4:var p=input[0],n=input[1],k=input[2],m=input[3];return [input.bytes,p,n,k,m,[{key}[0],{key}[1],{key}[2],{key}[3],{key}[4],{key}[5],{key}[6],{key}[7]]];}}}}")
    }
    #[test]
    fn current_key_declaration_survives_dispatcher_parameter_repacking() {
        let source = format!(
            "function outer(){{var key=[1,2,3,4,5,6,7,8];{}}}",
            constructor("pack", "key")
        );
        assert_eq!(
            read(&source).unwrap()["iv"],
            json!([1, 2, 3, 4, 5, 6, 7, 8])
        );
        assert!(read(&source.replace("key[4]", "key[3]")).is_err());
        assert!(read(&source.replace("[1,2,3,4,5,6,7,8]", "[1,2,3,4,5,input,7,8]")).is_err());
    }
    #[test]
    fn conflicting_current_keys_are_not_arbitrarily_selected() {
        let source = format!(
            "function outer(){{var key=[1,2,3,4,5,6,7,8],other=[8,7,6,5,4,3,2,1];{}{}}}",
            constructor("one", "key"),
            constructor("two", "other")
        );
        assert!(read(&source).is_err());
    }
    #[test]
    fn unrelated_shadowed_arrays_do_not_make_the_current_key_ambiguous() {
        let source=format!("function outer(){{var key=[1,2,3,4,5,6,7,8];{}function unrelated(){{var key=[undefined,undefined];return key;}}}}",constructor("pack","key"));
        assert_eq!(
            read(&source).unwrap()["iv"],
            json!([1, 2, 3, 4, 5, 6, 7, 8])
        );
    }
    #[test]
    fn current_key_can_be_an_obfuscated_array_slot() {
        let slots = vec!["0"; 97].join(",");
        let source = format!(
            "function outer(){{var pool=[{slots}];pool[48+48]=[1,2,3,4,5,6,7,8];{}}}",
            constructor("pack", "pool[96]")
        );
        assert_eq!(
            read(&source).unwrap()["iv"],
            json!([1, 2, 3, 4, 5, 6, 7, 8])
        );
        assert_eq!(
            read(&source.replace("pool[96][0]", "pool[24*4][0]")).unwrap()["iv"],
            json!([1, 2, 3, 4, 5, 6, 7, 8])
        );
        assert!(read(&source.replace("pool[48+48]", "pool[95]")).is_err());
    }
}
