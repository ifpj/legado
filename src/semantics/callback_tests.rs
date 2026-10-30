use super::*;
use crate::{
    catalog::{Catalog, Module, Target},
    data::{Format, Literal, Plan, Program, Response},
};

// Synthetic layout only. Three callback modules; the last one's span runs
// past its constant pool into the shared root-operand block.
const SUBMIT: &str = "submit-1793000000-1.3.1.1-SSSS";
const NONCE: &str = "NNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNNN";
const STATE: &str = "state-1793000000-1.3.1.1-TTTT";
const FIELDS: [(&str, usize); 13] = [
    ("qrKd7", 5000),
    ("vaZBm3", 5100),
    ("nidg0", 5200),
    ("gRYZ0", 5300),
    ("yVlbA2", 5400),
    ("TYrU7", 5500),
    ("Hoqd1", 5510),
    ("vvfHL3", 5520),
    ("lffN9", 5530),
    ("bkOl5", 5540),
    ("gfOgY8", 5550),
    ("Bxtcq6", 5560),
    ("jLEVv0", 5600),
];

fn literal(start: usize, value: &str, packed: Option<&str>) -> Literal {
    Literal {
        key: Some(0),
        start,
        end: start + 5,
        value: value.into(),
        packed: packed.map(String::from),
        continuation: 0,
        pool_offset: 0,
    }
}
fn callback_body(start: usize, label: &str) -> Vec<Literal> {
    ["qotUd2", "", "eval", "this", "wOEFN7"]
        .iter()
        .enumerate()
        .map(|(i, v)| literal(start + 10 * (i + 1), v, None))
        .chain([literal(start, label, None)])
        .collect()
}
fn layout(fields: &[(&str, usize)], extra: Option<(usize, &str)>) -> Vec<Literal> {
    let mut all = callback_body(1000, "Abcd1");
    all.extend(callback_body(2000, "Bcde2"));
    all.extend(callback_body(3000, "Cdef3"));
    let pool = "x".repeat(4098);
    all.push(literal(3800, &pool, Some("pool")));
    for (name, at) in fields {
        all.push(literal(*at, name, None));
    }
    for (at, value) in [
        (5010, "[]"),
        (5108, SUBMIT),
        (5210, NONCE),
        (5305, "0"),
        (5405, STATE),
    ] {
        all.push(literal(at, value, Some(value)));
    }
    if let Some((at, value)) = extra {
        all.push(literal(at, value, None));
    }
    all.sort_by_key(|l| l.start);
    all
}
fn run(literals: Vec<Literal>, nonce: &str) -> usize {
    let packed = literals
        .iter()
        .filter_map(|l| l.packed.clone().map(|p| (l.start, p)))
        .collect();
    let response = Response {
        raw: vec![],
        program: Program {
            sha256: String::new(),
            byte_length: 0,
            code_length: 9000,
            strings: vec![],
        },
        literals: literals.into(),
        packed,
    };
    let plan = Plan {
        format: Format {
            byte_offset: 0,
            initial_key: 0,
            state_step: 0,
            state_multiplier: 0,
            state_increment: 0,
            hash_salt: 0,
            char_xor: 0,
            state_quadratic: 0,
            char_mode: "after-subtract".into(),
            opcode_multiplier: None,
        },
        meta: json!({"build":"d2d3316b729e"}),
        alphabet: String::new(),
        modulus: String::new(),
        flow: String::new(),
    };
    let module = |id: &str, label: &str, start, end| Module {
        id: id.into(),
        data: String::new(),
        label: label.into(),
        start,
        end,
        metadata: 0,
    };
    let catalog = Catalog {
        parameters: json!({"vaZBm3":SUBMIT,"nidg0":nonce,"yVlbA2":STATE}),
        modules: vec![
            module("a", "Abcd1", 1000, 1900),
            module("b", "Bcde2", 2000, 2900),
            module("c", "Cdef3", 3000, 9000),
        ],
        targets: vec![Target {
            name: "entry".into(),
            at: 4500,
            target: 8000,
            key: 0,
        }],
        order: vec![],
    };
    let mut results = Results::default();
    results.insert("x", "code-point-error", json!({"KOmHB7":21}), 0);
    results.insert("y", "summarizer", json!({"ZLSD0":"unavailable"}), 0);
    callbacks(
        &Context {
            response: &response,
            plan: &plan,
            catalog: &catalog,
            html: &"".into(),
            parent_html: &"".into(),
            frame_url: "https://challenges.cloudflare.com/",
            page_url: "https://www.69shuba.com/",
            mode: "managed",
        },
        &mut results,
    )
    .unwrap();
    results
        .kinds
        .iter()
        .filter(|k| *k == "native-protocol-empty-callback")
        .count()
}

#[test]
fn last_callback_may_span_the_bound_root_operand_block() {
    assert_eq!(run(layout(&FIELDS, None), NONCE), 3);
}

#[test]
fn operand_block_must_bind_current_values_and_exact_fields() {
    // Different nonce: the block is not this round's operand block.
    assert_eq!(run(layout(&FIELDS, None), &"M".repeat(43)), 0);
    // A missing field name.
    assert_eq!(run(layout(&FIELDS[..12], None), NONCE), 0);
    // An unexpected literal inside the block, or after it before the registry.
    assert_eq!(run(layout(&FIELDS, Some((5450, "evil"))), NONCE), 0);
    assert_eq!(run(layout(&FIELDS, Some((6000, "evil"))), NONCE), 0);
}
