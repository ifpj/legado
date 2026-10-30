//! Decode an explicitly recognized literal-only encoding. No source is run.
use crate::{
    constants::{children, unwrap, walk, Kinds},
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

/// Preorder scope ownership of one parse, built in a single pass. `owner`
/// follows lexical function scopes (methods are transparent, as before);
/// `shallow` additionally stops at methods, which is what a non-descending
/// walk of a function body sees. `end` is the preorder end of each scope.
struct ScopeMap {
    owner: Vec<u32>,
    shallow: Vec<u32>,
    end: Vec<u32>,
}
impl ScopeMap {
    fn new(nodes: &[Node<'_>]) -> Result<Self> {
        let kinds = Kinds::get();
        let count = nodes.len();
        let (mut owner, mut shallow, mut end) = (
            vec![0u32; count],
            vec![0u32; count],
            vec![count as u32; count],
        );
        let (mut functions, mut scopes) = (Vec::<usize>::new(), Vec::<usize>::new());
        for (index, node) in nodes.iter().enumerate() {
            if index % 1024 == 0 {
                crate::cpu::check()?;
            }
            let start = node.start_byte();
            while functions
                .last()
                .is_some_and(|&top| nodes[top].end_byte() <= start)
            {
                functions.pop();
            }
            while scopes
                .last()
                .is_some_and(|&top| nodes[top].end_byte() <= start)
            {
                end[scopes.pop().unwrap()] = index as u32;
            }
            owner[index] = functions.last().copied().unwrap_or(index) as u32;
            shallow[index] = scopes.last().copied().unwrap_or(index) as u32;
            let kind = node.kind_id();
            if kind == kinds.program || kinds.is_function(kind) {
                functions.push(index);
            }
            if kind == kinds.program || kinds.is_boundary(kind) {
                scopes.push(index);
            }
        }
        Ok(Self {
            owner,
            shallow,
            end,
        })
    }
    fn owner(&self, index: usize) -> usize {
        self.owner[index] as usize
    }
    /// Names bound directly in `scope`: declarations, function names,
    /// assignment and update targets of its body (not of nested functions or
    /// methods), plus its own parameters. Values are node indices.
    fn bindings<'a>(
        &self,
        scope: usize,
        nodes: &[Node<'a>],
        source: &'a str,
    ) -> HashMap<&'a str, Vec<usize>> {
        let kinds = Kinds::get();
        let node = nodes[scope];
        let body = node.child_by_field_name("body").unwrap_or(node);
        let (body_start, body_end) = (body.start_byte(), body.end_byte());
        let mut bindings: HashMap<_, Vec<_>> = HashMap::new();
        for index in scope..self.end[scope] as usize {
            let n = nodes[index];
            if self.shallow[index] as usize != scope
                || n.start_byte() < body_start
                || n.end_byte() > body_end
            {
                continue;
            }
            let kind = n.kind_id();
            let target = if kind == kinds.variable_declarator || kind == kinds.function_declaration
            {
                n.child_by_field_name("name")
            } else if kind == kinds.assignment_expression
                || kind == kinds.augmented_assignment_expression
            {
                n.child_by_field_name("left")
            } else if kind == kinds.update_expression {
                n.child_by_field_name("argument")
            } else {
                None
            };
            if let Some(target) = target.filter(|n| n.kind_id() == kinds.identifier) {
                bindings
                    .entry(&source[target.byte_range()])
                    .or_default()
                    .push(index);
            }
        }
        if let Some(params) = node.child_by_field_name("parameters") {
            for param in children(params)
                .into_iter()
                .filter(|n| n.kind_id() == kinds.identifier)
            {
                bindings
                    .entry(&source[param.byte_range()])
                    .or_default()
                    .push(usize::MAX);
            }
        }
        if let Some(param) = node.child_by_field_name("parameter") {
            bindings
                .entry(&source[param.byte_range()])
                .or_default()
                .push(usize::MAX);
        }
        bindings
    }
}

/// The original syntax tree plus the literal edits, applied on demand. The
/// layout-reuse path never needs the edited tree, so the edit work is only
/// paid by compilation and by the complete sealed-key fallback.
pub(crate) struct EditedSyntax {
    original: tree_sitter::Tree,
    edits: Vec<tree_sitter::InputEdit>,
}
impl EditedSyntax {
    pub(crate) fn apply(&self) -> tree_sitter::Tree {
        let mut edited = self.original.clone();
        for edit in &self.edits {
            edited.edit(edit);
        }
        edited
    }
}
pub(crate) struct Prepared<'a> {
    pub source: Cow<'a, str>,
    pub(crate) edited: Option<EditedSyntax>,
}
impl Prepared<'_> {
    /// Incrementally parsing `source` from this tree matches a complete parse.
    pub(crate) fn tree(&self) -> Option<tree_sitter::Tree> {
        self.edited.as_ref().map(EditedSyntax::apply)
    }
}
pub fn prepare(source: &str) -> Result<Cow<'_, str>> {
    Ok(prepare_inner(source, false, None)?.source)
}
pub(crate) fn prepare_for_compile(source: &str) -> Result<Prepared<'_>> {
    prepare_for_compile_reusing(source, None)
}
pub(crate) fn prepare_for_compile_reusing<'a>(
    source: &'a str,
    tree: Option<&tree_sitter::Tree>,
) -> Result<Prepared<'a>> {
    prepare_inner(source, true, tree)
}
fn prepare_inner<'a>(
    source: &'a str,
    retain_tree: bool,
    previous: Option<&tree_sitter::Tree>,
) -> Result<Prepared<'a>> {
    // Old initializers keep their original path without an extra syntax pass.
    if !source.contains("atob") || !source.contains("fromCharCode.apply") {
        return Ok(Prepared {
            source: Cow::Borrowed(source),
            edited: None,
        });
    }
    require(source.len() <= 8 * 1024 * 1024, "Initializer size limit")?;
    let tree = crate::cpu::parse_reusing(&mut parser(), source, previous)?;
    require(!tree.root_node().has_error(), "Literal syntax unsupported")?;
    let kinds = Kinds::get();
    let nodes = walk(tree.root_node(), true);
    let scopes = ScopeMap::new(&nodes)?;
    let mut recipes = HashMap::new();
    let mut owners = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        if index % 256 == 0 {
            crate::cpu::check()?;
        }
        if node.kind_id() != kinds.variable_declarator {
            continue;
        }
        let (Some(name), Some(value)) = (
            node.child_by_field_name("name"),
            node.child_by_field_name("value"),
        ) else {
            continue;
        };
        if name.kind_id() != kinds.identifier {
            continue;
        }
        if let Some(seed) = recipe_seed(unwrap(value), source) {
            let owner = scopes.owner(index);
            recipes.insert(index, seed);
            owners
                .entry(owner)
                .or_insert_with(|| scopes.bindings(owner, &nodes, source));
        }
    }
    if recipes.is_empty() {
        return Ok(Prepared {
            source: Cow::Borrowed(source),
            edited: retain_tree.then(|| EditedSyntax {
                original: tree,
                edits: Vec::new(),
            }),
        });
    }
    let names: HashSet<_> = recipes
        .keys()
        .map(|&index| {
            &source[nodes[index]
                .child_by_field_name("name")
                .unwrap()
                .byte_range()]
        })
        .collect();
    let mut replacements = Vec::new();
    let mut output_len = source.len();
    for (index, node) in nodes.iter().enumerate() {
        if index % 256 == 0 {
            crate::cpu::check()?;
        }
        if node.kind_id() != kinds.call_expression {
            continue;
        }
        let Some(callee) = node
            .child_by_field_name("function")
            .map(unwrap)
            .filter(|n| n.kind_id() == kinds.identifier)
        else {
            continue;
        };
        let name = &source[callee.byte_range()];
        if !names.contains(name) {
            continue;
        }
        let mut lexical_owner = scopes.owner(index);
        let seed = loop {
            let local = owners
                .entry(lexical_owner)
                .or_insert_with(|| scopes.bindings(lexical_owner, &nodes, source));
            if let Some(values) = local.get(name) {
                break (values.len() == 1)
                    .then(|| recipes.get(&values[0]).copied())
                    .flatten();
            }
            if nodes[lexical_owner].kind_id() == kinds.program {
                break None;
            }
            lexical_owner = scopes.owner(lexical_owner);
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
        return Ok(Prepared {
            source: Cow::Borrowed(source),
            edited: retain_tree.then(|| EditedSyntax {
                original: tree,
                edits: Vec::new(),
            }),
        });
    }
    let edited = if retain_tree {
        let starts: Vec<_> = std::iter::once(0)
            .chain(
                source
                    .bytes()
                    .enumerate()
                    .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
            )
            .collect();
        let point = |position| {
            let row = starts.partition_point(|start| *start <= position) - 1;
            tree_sitter::Point {
                row,
                column: position - starts[row],
            }
        };
        // Edits are listed from the end so every earlier byte/point keeps the
        // original coordinates, including scripts with multibyte text or CRLF.
        let mut edits = Vec::with_capacity(replacements.len());
        for (range, value) in replacements.iter().rev() {
            let start_position = point(range.start);
            let mut new_end_position = start_position;
            for byte in value.bytes() {
                if byte == b'\n' {
                    new_end_position.row += 1;
                    new_end_position.column = 0;
                } else {
                    new_end_position.column += 1;
                }
            }
            edits.push(tree_sitter::InputEdit {
                start_byte: range.start,
                old_end_byte: range.end,
                new_end_byte: range.start + value.len(),
                start_position,
                old_end_position: point(range.end),
                new_end_position,
            });
        }
        Some(EditedSyntax {
            original: tree,
            edits,
        })
    } else {
        None
    };
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
    Ok(Prepared {
        source: Cow::Owned(output),
        edited,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Manual local profiling of the decode pass on a captured initializer.
    #[test]
    #[ignore = "set CF_GATEWAY_PROFILE_SCRIPT to a local captured public initializer"]
    fn profile_prepare_stages() {
        let source = std::fs::read_to_string(
            std::env::var("CF_GATEWAY_PROFILE_SCRIPT").expect("initializer path"),
        )
        .unwrap();
        for round in 0..3 {
            let mut last = std::time::Instant::now();
            let mut rows = Vec::new();
            let mut stage = |name: &str| {
                rows.push((name.to_string(), last.elapsed().as_secs_f64() * 1000.));
                last = std::time::Instant::now();
            };
            let tree = crate::cpu::parse(&mut parser(), &source).unwrap();
            stage("parse");
            let nodes = walk(tree.root_node(), true);
            stage("walk");
            let scopes = ScopeMap::new(&nodes).unwrap();
            stage("scope_map");
            let kinds = Kinds::get();
            let owners: std::collections::BTreeSet<_> = (0..nodes.len())
                .filter(|&i| {
                    let k = nodes[i].kind_id();
                    k == kinds.program || kinds.is_function(k)
                })
                .collect();
            let bound: usize = owners
                .iter()
                .map(|&o| scopes.bindings(o, &nodes, &source).len())
                .sum();
            stage("bindings_all_scopes");
            let plain = prepare_inner(&source, false, Some(&tree)).unwrap();
            stage("prepare_without_tree");
            let prepared = prepare_for_compile_reusing(&source, Some(&tree)).unwrap();
            stage("prepare_with_lazy_edits");
            let edited = prepared.tree();
            stage("apply_edits");
            let reparsed =
                crate::cpu::parse_reusing(&mut parser(), prepared.source.as_ref(), edited.as_ref())
                    .unwrap();
            stage("incremental_reparse");
            let full = crate::cpu::parse(&mut parser(), prepared.source.as_ref()).unwrap();
            stage("full_reparse");
            assert_eq!(plain.source, prepared.source);
            assert_eq!(reparsed.root_node().to_sexp(), full.root_node().to_sexp());
            eprintln!(
                "{}",
                serde_json::to_string(&serde_json::json!({"round":round,"nodes":nodes.len(),"scopes":owners.len(),"boundNames":bound,"preparedLen":prepared.source.len(),"stagesMs":rows}))
                    .unwrap()
            );
        }
    }
    #[test]
    fn shared_original_syntax_survives_literal_rewrites_and_is_reused_by_configuration_reads() {
        let source=format!("var decode={RECIPE};var plain=decode(`LAsLWkoMQQ==`);_cf_chl_opt.TglCI7={{LgdCU7:'en-us',other:[1,2]}};");
        let document = crate::html::Document::from(source.clone());
        let tree = document.syntax().unwrap();
        let shape = tree.root_node().to_sexp();
        let prepared = prepare_for_compile_reusing(&document, Some(tree)).unwrap();
        assert!(prepared.source.contains("plain=\"_0xa972\""));
        assert_eq!(tree.root_node().to_sexp(), shape);
        assert!(std::ptr::eq(tree, document.clone().syntax().unwrap()));
        assert_eq!(
            crate::static_data::assignment_document(&document, "_cf_chl_opt.TglCI7").unwrap(),
            crate::static_data::assignment(&source, "_cf_chl_opt.TglCI7").unwrap()
        );
    }
    #[test]
    fn edited_tree_matches_full_parse_after_multiline_and_unicode_replacements() {
        for source in [
            format!("/*中文😀*/var decode={RECIPE};\r\nvar a=decode(\r\n`jBY9xPRejAarxHPl`\r\n);\nvar b=decode(`CV4BVHNXGlUWMQ==`);function shadow(decode){{return decode(`LAsLWkoMQQ==`)}}"),
            format!("var decode={RECIPE};var a=17;"),
            "var a=1;function atob(){return 'fromCharCode.apply'}".into(),
        ] {
            let prepared = prepare_for_compile(&source).unwrap();
            assert_eq!(prepare(&source).unwrap(), prepared.source);
            let incremental = crate::cpu::parse_reusing(&mut parser(), &prepared.source, prepared.tree().as_ref()).unwrap();
            let complete = crate::cpu::parse(&mut parser(), &prepared.source).unwrap();
            assert!(!incremental.root_node().has_error());
            assert_eq!(incremental.root_node().to_sexp(), complete.root_node().to_sexp());
            let positions = |tree: &tree_sitter::Tree| walk(tree.root_node(), true).into_iter().map(|n| (n.kind().to_string(), n.byte_range(), n.start_position(), n.end_position())).collect::<Vec<_>>();
            assert_eq!(positions(&incremental), positions(&complete));
        }
    }
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
