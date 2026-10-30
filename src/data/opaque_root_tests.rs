use super::*;

// Synthetic constants only: no captured challenge state. With key 0 and no
// opcode multiplier the byte after each literal is its follower opcode.
fn opaque(prefix: &str, fill: char) -> String {
    format!("{prefix}{}", fill.to_string().repeat(70))
}
fn response(page_follower: u8, task_follower: u8) -> (Response, Plan, String, String) {
    let page = opaque("Cg", 'P');
    let task = opaque("Ch", 'T');
    let state = opaque("Ci", 'S');
    let repeated = opaque("Cj", 'R');
    let input = format!("\"{}\"", opaque("Ck", 'I'));
    let constants = [
        (10, "0x4AAAAAAADnPIDROrmt1Wwj".to_string(), 0),
        (20, page.clone(), 0),
        (30, "/fo/1:2:abc/0123456789abcdef/x".to_string(), 0),
        (40, "N".repeat(43), 0),
        (50, state, 1),
        (60, task.clone(), 0),
        (70, repeated.clone(), 0),
        (80, input, 0),
        (90, repeated, 0),
    ];
    let mut raw = vec![0u8; 128];
    raw[15] = 112;
    raw[25] = page_follower;
    raw[65] = task_follower;
    let literals: Vec<_> = constants
        .iter()
        .map(|(start, value, continuation)| Literal {
            key: Some(0),
            start: *start,
            end: start + 5,
            value: value.clone(),
            packed: Some(value.clone()),
            continuation: *continuation,
            pool_offset: 0,
        })
        .collect();
    let response = Response {
        raw,
        program: Program {
            sha256: String::new(),
            byte_length: 128,
            code_length: 128,
            strings: vec![],
        },
        packed: constants.iter().map(|(p, v, _)| (*p, v.clone())).collect(),
        literals: literals.into(),
    };
    let plan = Plan {
        format: Format {
            byte_offset: 0,
            initial_key: 0,
            state_step: 3,
            state_multiplier: 0,
            state_increment: 7,
            hash_salt: 0,
            char_xor: 0,
            state_quadratic: 0,
            char_mode: "before-subtract".into(),
            opcode_multiplier: None,
        },
        meta: json!({}),
        alphabet: String::new(),
        modulus: String::new(),
        flow: String::new(),
    };
    (response, plan, page, task)
}

#[test]
fn opaque_root_roles_follow_site_key_instruction() {
    let (r, plan, page, task) = response(112, 234);
    let root = r.root(&plan).unwrap();
    assert_eq!(root["pageData"], page);
    assert_eq!(root["hmwT6"], task);
    assert_eq!(root["yVlbA2"], opaque("Ci", 'S'));
    assert_eq!(root["vaZBm3"], opaque("Cj", 'R'));
    assert_eq!(root["nuvy8"], opaque("Ck", 'I'));

    let (r, plan, page, task) = response(234, 112);
    let root = r.root(&plan).unwrap();
    assert_eq!(root["pageData"], task);
    assert_eq!(root["hmwT6"], page);
}

#[test]
fn opaque_root_rejects_ambiguous_or_missing_page_role() {
    for (page, task) in [(112, 112), (234, 234)] {
        let (r, plan, _, _) = response(page, task);
        assert_eq!(
            r.root(&plan).unwrap_err().message,
            "Opaque root page-data role"
        );
    }
}

#[test]
fn opaque_token_shape_is_bounded() {
    assert!(opaque_token(&opaque("Cg", 'A')));
    assert!(!opaque_token(&opaque("Cw", 'A')));
    assert!(!opaque_token(&opaque("Dg", 'A')));
    assert!(!opaque_token(&format!("Cg{}+", "A".repeat(69))));
    assert!(!opaque_token(&format!("Cg{}", "A".repeat(40))));
    assert!(state_token("p-1792532000-1.3.1.1-x"));
    assert!(!state_token("p-1792532000-1.2.1.1-x"));
}
