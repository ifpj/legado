use super::*;
#[test]
fn indexed_scope_owners_match_parent_walk_for_nested_functions_and_methods() {
    let source="var top=1;function outer(a){const f=(x)=>x+a;class C{method(y){function inner(z){return z+y;}return inner(y)}}return function named(q){return f(q)}}var tail=2;";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let nodes = walk(tree.root_node(), true);
    for methods in [false, true] {
        let indexed = scope_index(&nodes, methods);
        for node in &nodes {
            let mut parent = node.parent();
            while parent.is_some_and(|n| {
                !matches!(
                    n.kind(),
                    "program" | "function_declaration" | "function_expression" | "arrow_function"
                ) && !(methods && n.kind() == "method_definition")
            }) {
                parent = parent.unwrap().parent();
            }
            assert_eq!(
                indexed.get(&node.id()).copied(),
                parent,
                "{} / {}",
                methods,
                &source[node.byte_range()]
            );
        }
    }
}

#[test]
fn cursor_walk_preserves_preorder_function_boundaries_and_leaf_subtrees() {
    let source = "var a=1;/*skip*/function f(){var b=2;function g(){return 3;}}var c=4;";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let nodes = walk(tree.root_node(), true);
    assert!(nodes.iter().all(|n| n.is_named() && n.kind() != "comment"));
    let numbers = |nodes: Vec<Node<'_>>| {
        nodes
            .iter()
            .filter(|n| n.kind() == "number")
            .map(|n| source[n.byte_range()].to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(numbers(nodes.clone()), ["1", "2", "3", "4"]);
    for leaf in nodes.iter().filter(|n| n.named_child_count() == 0) {
        assert_eq!(walk(*leaf, true), [*leaf]);
    }
    let f = *nodes
        .iter()
        .find(|n| n.kind() == "function_declaration")
        .unwrap();
    assert_eq!(numbers(walk(f, false)), ["2"]);
    assert_eq!(numbers(walk(f, true)), ["2", "3"]);
}
fn check(source: &str) -> Result<i64> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let t = Tables::new(tree.root_node(), source)?;
    let result = t
        .all()
        .iter()
        .find(|n| {
            n.kind() == "variable_declarator"
                && child(**n, "name").is_ok_and(|n| t.text(n) == "result")
        })
        .unwrap();
    t.value(child(*result, "value")?)?.integer()
}
fn fixture(changes: &str) -> String {
    format!(
        r#"function outer(){{var key;var ops={{}};ops[key||(key="plus")]=function(a,b){{return a+b}};{changes}var result=ops[key||(key="plus")](3,5);}}"#
    )
}
#[test]
fn identical_lazy_text_initializers_recover_table_helpers() {
    assert_eq!(check(&fixture("")), Ok(8));
    assert_eq!(
        check(&fixture(r#"function nested(){return key||(key="plus")}"#)),
        Ok(8)
    );
}
#[test]
fn changed_or_dynamic_cache_writes_are_rejected() {
    for change in [
        r#"key||(key="minus");"#,
        r#"key='plus';"#,
        r#"key++;"#,
        r#"function nested(key){return key}"#,
        r#"var {key}=input;"#,
        r#"[key]=input;"#,
        r#"key||=(input);"#,
    ] {
        assert!(check(&fixture(change)).is_err(), "{change}");
    }
}
#[test]
fn initialized_parameter_empty_and_lexical_bindings_are_rejected() {
    for source in [
        fixture("").replace("outer()", "outer(key)"),
        fixture("").replace("var key;", "var key='other';"),
        fixture("").replace("var key;", "let key;"),
        fixture("").replace("plus", ""),
    ] {
        assert!(check(&source).is_err(), "{source}");
    }
}
#[test]
fn flattened_static_order_preserves_case_actions_and_rejects_mutated_order() {
    let source = r#"function f(a,b){var order;var i;for(order="c|a|b".split("|"),i=0;true;){switch(order[i++]){case "a":a+=7;continue;case "b":return a^b;case "c":b&=3;continue;}break;}}var result=f(5,7);"#;
    assert_eq!(check(source), Ok(15));
    for bad in [
        source.replace("a+=7;", "order[0]='b';a+=7;"),
        source.replace("a+=7;", "i++;a+=7;"),
        source.replace("c|a|b", "c|missing|b"),
    ] {
        assert!(check(&bad).is_err());
    }
}
#[test]
fn finite_counter_identity_is_proved_without_running_a_loop() {
    let source="function f(input){var counter=input&255,total=0;for(;counter;counter--){total++}return (input&-256)|total}var result=f(123456);";
    assert_eq!(check(source), Ok(123456));
    for bad in [
        source.replace("total++", "total+=2"),
        source.replace("-256", "-255"),
        source.replace("total", "input"),
    ] {
        assert!(check(&bad).is_err());
    }
}
#[test]
fn selected_dispatch_branch_can_return_a_block() {
    let source="function f(selector,a,b){switch(selector^19){case 7:{var x=a+b;return x^3}case 8:return;}}var result=f(20,6,7);";
    assert_eq!(check(source), Ok(14));
    assert!(check(&source.replace("f(20,6,7)", "f(27,6,7)")).is_err());
}
#[test]
fn flattened_dispatch_block_can_store_its_order_and_counter_in_array_slots() {
    let source = r#"function f(selector,a,b){switch(selector^19){case 7:{var box;for(box=["c|a|b".split("|"),0];true;){switch(box[0][box[1]++]){case "a":a+=7;continue;case "b":return a^b;case "c":b&=3;continue;}break;}}return;}}var result=f(20,5,7);"#;
    assert_eq!(check(source), Ok(15));
    for bad in [
        source.replace("a+=7;", "box[1]++;a+=7;"),
        source.replace("a+=7;", "box[0][0]='b';a+=7;"),
        source.replace("a+=7;", "box=[];a+=7;"),
    ] {
        assert!(check(&bad).is_err());
    }
}

#[test]
fn literal_arithmetic_keeps_js_semantics_and_dynamic_binding_rejection() {
    for (expression, expected) in [
        ("24*4", 96),
        ("(~-1025)^1", 1025),
        ("(-1>>>0)&255", 255),
        ("false||17", 17),
        ("(255+2)&255", 1),
    ] {
        assert_eq!(
            check(&format!("function f(input){{var result={expression};}}")),
            Ok(expected)
        );
    }
    assert!(check("var input=96;function f(input){var result=input+1;}").is_err());
    assert!(check("function f(){var result=1/0;}").is_err());
    let mut large = String::from("1");
    for _ in 0..11 {
        large = format!("({large})+({large})");
    }
    assert!(check(&format!("function f(){{var result={large};}}")).is_err());
}

#[test]
fn table_construction_publishes_current_bindings_and_keeps_detached_views_immutable() {
    let source = "function outer(){var table=[1,2];table[0]=9;function peek(){return table[0]}table[1]=peek()+5;function mutate(){table[0]=27;}var result=table[0]*100+table[1];}";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let tables = Tables::new(tree.root_node(), source).unwrap();
    let result = tables
        .all()
        .iter()
        .find(|n| {
            n.kind() == "variable_declarator"
                && child(**n, "name").is_ok_and(|n| tables.text(n) == "result")
        })
        .unwrap();
    let value = child(*result, "value").unwrap();
    assert_eq!(tables.value(value).unwrap().integer(), Ok(914));
    let saved = tables.scope(value);
    let mut detached = saved.clone();
    let mutation = *tables
        .all()
        .iter()
        .find(|n| n.kind() == "assignment_expression" && tables.text(**n) == "table[0]=27")
        .unwrap();
    tables.eval(mutation, &mut detached).unwrap();
    let first = |env: &Env<'_>| match &env["table"] {
        Constant::Array(values) => values[0].integer().unwrap(),
        _ => panic!("expected array"),
    };
    assert_eq!(first(&detached), 27);
    assert_eq!(first(&saved), 9);
    assert_eq!(tables.value(value).unwrap().integer(), Ok(914));
}
