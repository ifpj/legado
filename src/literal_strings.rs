//! Decode an explicitly recognized literal-only encoding. No source is run.
use crate::{
    constants::{children, unwrap, walk},
    require, Error, Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::OnceLock,
};
use tree_sitter::{Node, Parser};

const RECIPE: &str = "(function(e){var J={__proto__:null},O=e.String;return function(B){var s=J[B];if(s===void 0){var f=e.atob(B),V=15219,s=``,p=0,A=[];while(p<f.length){V=(V<<8|V>>>8)&65535;var b=f.charCodeAt(p++)^V>>>8;if(b===255){V=(V<<8|V>>>8)&65535;b=f.charCodeAt(p++)^V>>>8;V=(V<<8|V>>>8)&65535;b|=(f.charCodeAt(p++)^V>>>8)<<8}A.push(b);if(A.length===128){s+=O.fromCharCode.apply(O,A);A=[]}}s+=O.fromCharCode.apply(O,A);J[B]=s}return s}})(globalThis)";

fn signature(node: Node<'_>, source: &str, seed: usize) -> Vec<String> {
    let mut names = HashMap::new();
    walk(node, true)
        .into_iter()
        .filter(|n| n.child_count() == 0 && n.kind() != "comment")
        .map(|n| {
            let text = &source[n.byte_range()];
            if n.id() == seed {
                return "seed".into();
            }
            if n.kind() == "identifier" && text != "globalThis" {
                let index = names.len();
                format!("identifier:{}", names.entry(text).or_insert(index))
            } else {
                format!("{}:{text}", n.kind())
            }
        })
        .collect()
}

fn parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .expect("JavaScript grammar");
    parser
}

fn recipe_seed(node: Node<'_>, source: &str) -> Option<u16> {
    if node.kind() != "call_expression"
        || node.end_byte() - node.start_byte() > 2500
        || !source[node.byte_range()].contains("atob")
    {
        return None;
    }
    static EXPECTED: OnceLock<Vec<String>> = OnceLock::new();
    let expected = EXPECTED.get_or_init(|| {
        let tree = parser().parse(RECIPE, None).expect("literal recipe");
        let root = tree
            .root_node()
            .named_child(0)
            .unwrap()
            .named_child(0)
            .unwrap();
        let seed = walk(root, true)
            .into_iter()
            .find(|n| &RECIPE[n.byte_range()] == "15219")
            .unwrap();
        signature(root, RECIPE, seed.id())
    });
    for seed in walk(node, true).into_iter().filter(|n| {
        n.kind() == "number"
            && n.parent()
                .is_some_and(|p| p.kind() == "variable_declarator")
    }) {
        if let Ok(value) = source[seed.byte_range()].parse::<u16>() {
            if signature(node, source, seed.id()) == *expected {
                return Some(value);
            }
        }
    }
    None
}

fn decode(encoded: &str, mut seed: u16) -> Result<String> {
    require(encoded.len() <= 1_400_000, "Literal encoding size limit")?;
    let bytes = STANDARD.decode(encoded)?;
    let mut position = 0;
    let mut units = Vec::new();
    fn byte(bytes: &[u8], position: &mut usize, seed: &mut u16) -> Result<u16> {
        *seed = seed.swap_bytes();
        let value = bytes
            .get(*position)
            .ok_or_else(|| Error::new("INVALID_DATA", "Truncated literal escape"))?;
        *position += 1;
        Ok((value ^ (*seed >> 8) as u8) as u16)
    }
    // Each iteration consumes at least one byte. There is no source evaluator.
    while position < bytes.len() {
        if position % 256 == 0 {
            crate::cpu::check()?;
        }
        let value = byte(&bytes, &mut position, &mut seed)?;
        units.push(if value == 255 {
            byte(&bytes, &mut position, &mut seed)? | byte(&bytes, &mut position, &mut seed)? << 8
        } else {
            value
        });
    }
    String::from_utf16(&units).map_err(|_| Error::new("INVALID_DATA", "Invalid literal UTF-16"))
}

fn scope(mut node: Node<'_>) -> Node<'_> {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "function_declaration" | "function_expression" | "arrow_function" | "program"
        ) {
            return parent;
        }
        node = parent;
    }
    node
}

fn bindings<'a>(node: Node<'a>, source: &'a str) -> HashMap<&'a str, Vec<Node<'a>>> {
    let body = node.child_by_field_name("body").unwrap_or(node);
    let mut bindings: HashMap<_, Vec<_>> = HashMap::new();
    for n in walk(body, false) {
        let target = match n.kind() {
            "variable_declarator" | "function_declaration" => n.child_by_field_name("name"),
            "assignment_expression" | "augmented_assignment_expression" => {
                n.child_by_field_name("left")
            }
            "update_expression" => n.child_by_field_name("argument"),
            _ => None,
        };
        if let Some(target) = target.filter(|n| n.kind() == "identifier") {
            bindings
                .entry(&source[target.byte_range()])
                .or_default()
                .push(n);
        }
    }
    if let Some(params) = node.child_by_field_name("parameters") {
        for param in children(params)
            .into_iter()
            .filter(|n| n.kind() == "identifier")
        {
            bindings
                .entry(&source[param.byte_range()])
                .or_default()
                .push(param);
        }
    }
    if let Some(param) = node.child_by_field_name("parameter") {
        bindings
            .entry(&source[param.byte_range()])
            .or_default()
            .push(param);
    }
    bindings
}

pub fn prepare(source: &str) -> Result<Cow<'_, str>> {
    // Old initializers keep their original path without an extra syntax pass.
    if !source.contains("atob") || !source.contains("fromCharCode.apply") {
        return Ok(Cow::Borrowed(source));
    }
    require(source.len() <= 8 * 1024 * 1024, "Initializer size limit")?;
    let tree = crate::cpu::parse(&mut parser(), source)?;
    require(!tree.root_node().has_error(), "Literal syntax unsupported")?;
    let nodes = walk(tree.root_node(), true);
    let mut recipes = HashMap::new();
    let mut owners = HashMap::new();
    for node in &nodes {
        crate::cpu::check()?;
        if node.kind() != "variable_declarator" {
            continue;
        }
        let (Some(name), Some(value)) = (
            node.child_by_field_name("name"),
            node.child_by_field_name("value"),
        ) else {
            continue;
        };
        if name.kind() != "identifier" {
            continue;
        }
        if let Some(seed) = recipe_seed(unwrap(value), source) {
            let owner = scope(*node);
            recipes.insert(node.id(), seed);
            owners
                .entry(owner.id())
                .or_insert_with(|| bindings(owner, source));
        }
    }
    if recipes.is_empty() {
        return Ok(Cow::Borrowed(source));
    }
    let names: HashSet<_> = nodes
        .iter()
        .filter(|n| recipes.contains_key(&n.id()))
        .map(|n| &source[n.child_by_field_name("name").unwrap().byte_range()])
        .collect();
    let mut replacements = Vec::new();
    let mut output_len = source.len();
    for node in nodes.iter().filter(|n| n.kind() == "call_expression") {
        crate::cpu::check()?;
        let Some(callee) = node
            .child_by_field_name("function")
            .map(unwrap)
            .filter(|n| n.kind() == "identifier")
        else {
            continue;
        };
        let name = &source[callee.byte_range()];
        if !names.contains(name) {
            continue;
        }
        let mut owner = scope(*node);
        let seed = loop {
            let local = owners
                .entry(owner.id())
                .or_insert_with(|| bindings(owner, source));
            if let Some(values) = local.get(name) {
                break (values.len() == 1)
                    .then(|| recipes.get(&values[0].id()).copied())
                    .flatten();
            }
            if owner.kind() == "program" {
                break None;
            }
            owner = scope(owner);
        };
        let Some(seed) = seed else {
            continue;
        };
        let args = children(node.child_by_field_name("arguments").unwrap());
        if args.len() != 1 || !matches!(args[0].kind(), "string" | "template_string") {
            continue;
        }
        let encoded = crate::static_data::unquote(&source[args[0].byte_range()])?;
        let decoded = decode(&encoded, seed)?;
        let value = serde_json::to_string(&decoded)?;
        output_len = output_len
            .checked_sub(node.byte_range().len())
            .and_then(|n| n.checked_add(value.len()))
            .ok_or_else(|| Error::new("INVALID_DATA", "Decoded initializer size limit"))?;
        require(
            output_len <= 16 * 1024 * 1024,
            "Decoded initializer size limit",
        )?;
        replacements.push((node.byte_range(), value));
    }
    if replacements.is_empty() {
        return Ok(Cow::Borrowed(source));
    }
    let mut output = String::with_capacity(output_len);
    let mut copied = 0;
    for (range, value) in replacements {
        crate::cpu::check()?;
        require(range.start >= copied, "Overlapping literal replacements")?;
        output.push_str(&source[copied..range.start]);
        output.push_str(&value);
        copied = range.end;
    }
    output.push_str(&source[copied..]);
    require(
        output.len() <= 16 * 1024 * 1024,
        "Decoded initializer size limit",
    )?;
    Ok(Cow::Owned(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_recipe_is_native_bounded_and_scope_sensitive() {
        let source = format!("var decode={RECIPE};var plain=decode(`LAsLWkoMQQ==`);function shadow(decode){{return decode(`LAsLWkoMQQ==`)}}");
        let output = prepare(&source).unwrap();
        assert!(output.contains("var plain=\"_0xa972\""), "{output}");
        assert!(output.contains("return decode(`LAsLWkoMQQ==`)"));
        let changed = source.replace("b===255", "b===254");
        assert_eq!(prepare(&changed).unwrap(), changed);
        assert!(decode("jA==", 15219).is_err());
        assert_eq!(decode("jBY9xPRejAarxHPl", 15219).unwrap(), "中文😀");
        assert_eq!(decode("CV4BVHNXGlUWMQ==", 15219).unwrap(), "zero\0line\n");
    }

    #[test]
    fn many_literals_preserve_unicode_boundaries_and_shadowed_calls() {
        let mut source = format!("/*中文😀*/var decode={RECIPE};");
        let mut expected = source.clone();
        for index in 0..2000 {
            source.push_str(&format!("var v{index}=decode(`jBY9xPRejAarxHPl`);/*尾*/"));
            expected.push_str(&format!("var v{index}=\"中文😀\";/*尾*/"));
        }
        let shadow = "function shadow(decode){return decode(`LAsLWkoMQQ==`)}";
        source.push_str(shadow);
        expected.push_str(shadow);
        assert_eq!(prepare(&source).unwrap(), expected);
        assert!(matches!(prepare(&expected).unwrap(), Cow::Borrowed(_)));
    }
}
