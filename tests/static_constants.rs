use cf_gateway::constants::{walk, Constant, Tables};
use serde_json::Value;
fn parse(source: &str) -> tree_sitter::Tree {
    let mut p = tree_sitter::Parser::new();
    p.set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    p.parse(source, None).unwrap()
}
#[test]
fn rotation_aliases_resolve_without_executing_shadowed_or_recursive_calls() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/static-pool.json")).unwrap();
    let source=fixture["source"].as_str().unwrap().replace("function shadow","function recurse(v){return recurse(v)}function dangerous(){return process.exit(7)}function shadow");
    let tree = parse(&source);
    let t = Tables::new(tree.root_node(), &source).unwrap();
    let node = walk(tree.root_node(), true)
        .into_iter()
        .find(|n| n.kind() == "call_expression" && t.text(*n) == "a5(ix.q)")
        .unwrap();
    match t.value(node).unwrap() {
        Constant::Text(value) => assert_eq!(value, "entry-005"),
        _ => panic!("expected current pool value"),
    }
    for text in ["a5(7)", "recurse(v)", "process.exit(7)"] {
        let node = walk(tree.root_node(), true)
            .into_iter()
            .find(|n| n.kind() == "call_expression" && t.text(*n) == text)
            .unwrap();
        assert!(t.value(node).is_err(), "must leave {text} unevaluated");
    }
}
#[test]
fn tree_walk_keeps_subtree_boundaries_and_nested_function_policy() {
    let source="function outer(){ const first=1; function inner(){return 'inside'} return first; } const outside=2;";
    let tree = parse(source);
    let t = Tables::new(tree.root_node(), source).unwrap();
    let outer = walk(tree.root_node(), true)
        .into_iter()
        .find(|n| n.kind() == "function_declaration" && t.text(*n).starts_with("function outer"))
        .unwrap();
    for node in walk(tree.root_node(), true) {
        for descend in [false, true] {
            assert_eq!(t.walk(node, descend), walk(node, descend));
        }
    }
    let shallow = walk(outer, false);
    assert_eq!(shallow[0], outer);
    assert!(shallow
        .iter()
        .all(|n| n.start_byte() >= outer.start_byte() && n.end_byte() <= outer.end_byte()));
    assert!(!shallow.iter().any(|n| t.text(*n) == "'inside'"));
    assert!(walk(outer, true).iter().any(|n| t.text(*n) == "'inside'"));
    let other = parse("const other=3;");
    assert_eq!(
        walk(other.root_node(), true)
            .iter()
            .filter(|n| n.kind() == "number")
            .count(),
        1
    );
}

#[test]
fn recovered_large_current_pool_is_not_replaced_by_a_budget_failure() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/static-pool.json")).unwrap();
    let extra = ",u(400)".repeat(1500);
    let source = fixture["source"].as_str().unwrap().replace(
        "];\n    function a5",
        &format!("{extra}];\n    function a5"),
    );
    let tree = parse(&source);
    let tables = Tables::new(tree.root_node(), &source).unwrap();
    let call = tables
        .all()
        .iter()
        .copied()
        .find(|n| n.kind() == "call_expression" && tables.text(*n) == "a5(ix.q)")
        .unwrap();
    assert_eq!(tables.value(call).unwrap().text().unwrap(), "entry-005");
}

#[test]
fn lexical_constants_and_computed_wrapper_keys_remain_scope_bound() {
    let source = "(function(){var base=(x=>x).length+40;function read(){var ops={['add']:function(a,b){var pair;return pair=[b],a+pair[0]}};return ops['add'](base,1)}function shadow(){var base;return base}function parameter(base){return base}return read()})()";
    let tree = parse(source);
    let t = Tables::new(tree.root_node(), source).unwrap();
    let call = t
        .all()
        .iter()
        .copied()
        .find(|n| t.text(*n) == "ops['add'](base,1)")
        .unwrap();
    assert_eq!(t.value(call).unwrap().integer().unwrap(), 42);
    for name in ["shadow", "parameter"] {
        let owner = t
            .all()
            .iter()
            .copied()
            .find(|n| {
                n.kind() == "function_declaration"
                    && n.child_by_field_name("name")
                        .is_some_and(|n| t.text(n) == name)
            })
            .unwrap();
        let returned = walk(owner, false)
            .into_iter()
            .find(|n| n.kind() == "return_statement")
            .unwrap()
            .named_child(0)
            .unwrap();
        assert!(t.value(returned).is_err());
    }
}
