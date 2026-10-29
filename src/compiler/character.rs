//! Verify the finite byte decoder's order through local aliases and wrappers.
use super::*;

fn forwarded_reader<'a>(t: &Tables<'a>, call: Node<'a>, reader: &str) -> bool {
    let Ok(arguments) = child(call, "arguments") else {
        return false;
    };
    let args = children(arguments);
    if args.len() > 8 {
        return false;
    }
    let readers = args
        .iter()
        .filter(|n| n.kind() == "identifier" && t.text(**n) == reader)
        .collect::<Vec<_>>();
    if readers.len() != 1
        || args
            .iter()
            .filter(|n| n.id() != readers[0].id())
            .any(|n| t.value(*n).is_err())
    {
        return false;
    }
    let Ok(Constant::Function(function)) = t.value(*readers[0]) else {
        return false;
    };
    if !child(function, "parameters").is_ok_and(|n| n.named_child_count() == 0) {
        return false;
    }
    [0, 1, 127, 255].into_iter().all(|byte| {
        t.bound_once(
            call,
            &mut t.scope(call),
            function.id(),
            Constant::Number(byte as f64),
        )
        .and_then(|v| v.integer())
            == Ok(byte)
    })
}

pub(super) fn order<'a>(
    t: &Tables<'a>,
    function: Node<'a>,
    reader: &str,
    xor: u8,
) -> Option<&'static str> {
    let Ok(body) = child(function, "body") else {
        return None;
    };
    if t.text(body).len() > 2048
        || body.kind() != "statement_block" && function.kind() != "arrow_function"
    {
        return None;
    }
    if !child(function, "parameters").is_ok_and(|n| n.named_child_count() == 0) {
        return None;
    }
    let nodes = t.walk(body, true);
    if nodes.len() > 256
        || nodes.iter().any(|n| {
            matches!(
                n.kind(),
                "if_statement"
                    | "ternary_expression"
                    | "switch_statement"
                    | "for_statement"
                    | "while_statement"
                    | "do_statement"
                    | "new_expression"
                    | "await_expression"
                    | "update_expression"
                    | "augmented_assignment_expression"
                    | "function_declaration"
                    | "function_expression"
                    | "arrow_function"
            )
        })
    {
        return None;
    }
    if !nodes.iter().any(|n| {
        n.kind() == "binary_expression" && child(*n, "operator").is_ok_and(|op| t.text(op) == "^")
    }) {
        return None;
    }
    let calls = nodes
        .iter()
        .filter(|n| n.kind() == "call_expression")
        .copied()
        .collect::<Vec<_>>();
    let reads = calls
        .iter()
        .copied()
        .filter(|call| {
            (child(*call, "function")
                .is_ok_and(|n| n.kind() == "identifier" && t.text(n) == reader)
                && child(*call, "arguments").is_ok_and(|n| n.named_child_count() == 0))
                || forwarded_reader(t, *call, reader)
        })
        .collect::<Vec<_>>();
    if reads.len() != 1
        || calls.len() > 8
        || calls.iter().any(|call| {
            call.id() != reads[0].id()
                && !["+", "-", "^", "&", "|", "<<", ">>", ">>>"]
                    .iter()
                    .any(|op| operation(t, *call, op).is_some())
        })
    {
        return None;
    }
    let seeds = nodes
        .iter()
        .filter_map(|n| {
            let pair = operation(t, *n, ">>>")?;
            (integer(t, pair[1]) == Ok(16))
                .then(|| data_reference(t, pair[0]))
                .flatten()
        })
        .collect::<HashSet<_>>();
    if seeds.len() != 1 {
        return None;
    }
    let seed = seeds.iter().next().unwrap();
    let locals = nodes
        .iter()
        .filter(|n| n.kind() == "variable_declarator")
        .filter_map(|n| n.child_by_field_name("name"))
        .filter(|n| n.kind() == "identifier")
        .map(|n| t.text(n))
        .collect::<HashSet<_>>();
    if nodes
        .iter()
        .filter(|n| n.kind() == "assignment_expression")
        .any(|n| {
            let Ok(left) = child(*n, "left") else {
                return true;
            };
            match left.kind() {
                "identifier" => !locals.contains(t.text(left)) && t.text(left) != seed,
                "subscript_expression" if data_reference(t, left).as_ref() == Some(seed) => false,
                "subscript_expression" => !child(left, "object")
                    .is_ok_and(|n| n.kind() == "identifier" && locals.contains(t.text(n))),
                _ => true,
            }
        })
    {
        return None;
    }
    let seed_reads = nodes
        .iter()
        .filter(|n| data_reference(t, **n).as_ref() == Some(seed))
        .map(|n| n.id())
        .collect::<Vec<_>>();
    let mut before = true;
    let mut after = true;
    for value in [0u32, 1, 255, 256, 65535, 65536, 0x12345678, u32::MAX] {
        for byte in [0u8, 1, 2, 63, 127, 128, 254, 255] {
            let mut overrides = seed_reads
                .iter()
                .map(|id| (*id, Constant::Number(value as f64)))
                .collect::<HashMap<_, _>>();
            overrides.insert(reads[0].id(), Constant::Number(byte as f64));
            let evaluated = if body.kind() == "statement_block" {
                t.invoke_bound(function, vec![], overrides)
            } else {
                t.bound(body, &mut t.scope(body), overrides)
            };
            let Ok(actual) = evaluated.and_then(|v| v.integer()) else {
                return None;
            };
            before &=
                actual == ((byte ^ xor ^ (value >> 16) as u8).wrapping_sub(value as u8)) as i64;
            after &=
                actual == ((byte ^ xor).wrapping_sub(value as u8) ^ (value >> 16) as u8) as i64;
            if !before && !after {
                return None;
            }
        }
    }
    match (before, after) {
        (true, false) => Some("before-subtract"),
        (false, true) => Some("after-subtract"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn classify(body: &str) -> Option<&'static str> {
        let source=format!("function outer(){{var seed;var slots=[0,0,0,0];function shift(a,b){{return a>>>b}}var reader=function(){{return unknown}};var decode=()=>{{{body}}};}}");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let tables = Tables::new(tree.root_node(), &source).unwrap();
        let function = *tables
            .all()
            .iter()
            .find(|n| n.kind() == "arrow_function")
            .unwrap();
        order(&tables, function, "reader", 117)
    }
    fn check(body: &str) -> bool {
        classify(body) == Some("after-subtract")
    }
    #[test]
    fn zero_argument_reader_forwarding_is_proved_and_must_execute_once() {
        let helper = "function forward(selector,fn){switch(selector){case 7:return fn()}}";
        let body = format!("{helper}{}", BODY.replace("reader()", "forward(7,reader)"));
        // The helper is outside the decoder, matching the production shape.
        let classify_with = |helper: &str| {
            let source=format!("function outer(){{var seed;var reader=function(){{return unknown}};{helper}var decode=()=>{{{}}};}}",BODY.replace("reader()","forward(7,reader)"));
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_javascript::LANGUAGE.into())
                .unwrap();
            let tree = parser.parse(&source, None).unwrap();
            let t = Tables::new(tree.root_node(), &source).unwrap();
            let f = *t
                .all()
                .iter()
                .find(|n| n.kind() == "arrow_function")
                .unwrap();
            order(&t, f, "reader", 117)
        };
        assert_eq!(classify_with(helper), Some("after-subtract"));
        for bad in [
            helper.replace("fn()", "fn()+fn()"),
            helper.replace("fn()", "fn()+1"),
            helper.replace("fn()", "0"),
            helper.replace("fn()", "host(fn())"),
        ] {
            assert!(classify_with(&bad).is_none());
        }
        assert!(classify(&body).is_none());
    }
    const BODY:&str="var box;var lo;var hi;var shifted;return box=[void 0,void 0],lo=(reader()^117)-seed&255,shifted=seed>>>16,box[0]=255,hi=(0|shifted)+(0|box[0])-(box[0]|shifted)|0,box[1]=0|(hi|lo)-(lo&hi),seed=seed+-2654435769|0,box[1]";
    #[test]
    fn aliased_or_minus_and_decoder_has_a_unique_byte_order() {
        assert!(check(BODY));
        assert!(check(&BODY.replace("seed>>>16", "shift(seed,16)")));
        assert!(!check(
            &BODY
                .replace(
                    "(reader()^117)-seed&255",
                    "(reader()^117^(seed>>>16))-seed&255"
                )
                .replace("box[1]=0|(hi|lo)-(lo&hi)", "box[1]=lo")
        ));
        assert!(!check(&BODY.replace("seed>>>16", "seed>>>15")));
        assert!(!check(&BODY.replace("(lo&hi)", "(lo&hi)+1")));
    }
    #[test]
    fn branches_extra_calls_and_nonlocal_writes_are_not_byte_models() {
        for body in [
            format!("if(seed===1234)return 0;{BODY}"),
            BODY.replace("reader()", "reader(1)"),
            BODY.replace("reader()", "other()"),
            format!("other[0]=1;{BODY}"),
        ] {
            assert_eq!(classify(&body), None);
        }
    }
    #[test]
    fn both_orders_and_array_seed_bindings_are_explicitly_verified() {
        assert_eq!(
            classify("return ((reader()^117^(seed>>>16))-seed)&255"),
            Some("before-subtract")
        );
        assert_eq!(
            classify("return ((reader()^117)-seed)&255^(seed>>>16&255)"),
            Some("after-subtract")
        );
        assert_eq!(
            classify(&BODY.replace("seed", "slots[3]")),
            Some("after-subtract")
        );
    }
}
