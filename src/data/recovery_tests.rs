use super::*;

// Synthetic bytecode: a five-byte pool candidate overlaps the start of
// an inline string. No captured challenge state is used in this fixture.
fn fixture(value: &str) -> (Vec<u8>, Program, Plan) {
    let format = Format {
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
    };
    let mut raw = vec![
        0,
        0,
        19,
        11 ^ 7,
        17,
        127 ^ 7,
        11 ^ 7,
        17,
        value.len() as u8 ^ 7,
    ];
    let mut seed = format.seed(raw.len());
    for byte in value.bytes() {
        raw.push((byte.wrapping_add(seed as u8) ^ (seed >> 16) as u8) ^ 7);
        seed = seed.wrapping_add(2654435769);
    }
    let at = raw.len();
    let (opcode, key, salt) = (172u8, 7u8, 123456u32);
    raw.extend([opcode ^ key, 7, 7, 4 ^ 7, 0, 0, 0, 0]);
    raw.extend([31, 47, 59, 71]);
    let mut hash = at as u32 ^ salt;
    for operand in [opcode as u32, key as u32, 4] {
        hash = hash.wrapping_mul(16777619) ^ operand;
    }
    hash = hash.wrapping_mul(16777619);
    for byte in &raw[at + 8..] {
        hash = (hash ^ *byte as u32).wrapping_mul(16777619);
    }
    for (i, byte) in hash.to_be_bytes().iter().enumerate() {
        raw[at + 4 + i] = byte ^ key;
    }
    let mut strings = vec![String::new(); 12];
    strings[11] = "forEach".into();
    let program = Program {
        sha256: String::new(),
        byte_length: raw.len(),
        code_length: raw.len(),
        strings,
    };
    let plan = Plan {
        format,
        meta: json!({"key_material":{"literal_tag":11,"literal_kind_slot":1},"input_data_profile":{"flow-call":[opcode,[],{"salt":salt}]}}),
        alphabet: String::new(),
        modulus: String::new(),
        flow: String::new(),
    };
    (raw, program, plan)
}

#[test]
fn authenticated_call_recovers_short_punctuation_literal_hidden_by_pool_candidate() {
    for value in ["so.", "x.", "a-b"] {
        let (raw, program, plan) = fixture(value);
        let mut all = literals(&raw, &program, &plan, false, 65535, false).unwrap();
        assert!(all.iter().any(|l| l.start == 2 && l.value == "forEach"));
        assert!(!all.iter().any(|l| l.start == 5));
        recover_literals(&raw, &program, &plan, &mut all).unwrap();
        assert!(all.iter().any(|l| l.start == 5 && l.value == value));
        assert!(!all.iter().any(|l| l.start == 2));
    }
}

#[test]
fn damaged_or_unbound_following_call_cannot_admit_a_short_candidate() {
    for mode in 0..4 {
        let (mut raw, mut program, mut plan) = fixture("so.");
        match mode {
            0 => raw[16] ^= 1, // authenticated checksum
            1 => raw[12] ^= 1, // instruction does not continue the literal key
            2 => {
                raw.pop();
                program.code_length = raw.len();
            }
            _ => plan.meta["input_data_profile"] = Value::Null,
        }
        let mut all = literals(&raw, &program, &plan, false, 65535, false).unwrap();
        recover_literals(&raw, &program, &plan, &mut all).unwrap();
        assert!(!all.iter().any(|l| l.start == 5), "mode {mode}");
        assert!(all.iter().any(|l| l.start == 2));
    }
}

#[test]
fn call_anchor_uses_current_position_dependent_opcode_and_literal_key() {
    let (mut raw, program, mut plan) = fixture("so.");
    let mut literal = literals(&raw, &program, &plan, true, 64, false)
        .unwrap()
        .into_iter()
        .find(|l| l.start == 5)
        .unwrap();
    let key = literal.key.unwrap();
    let multiplier = 93u32;
    plan.format.opcode_multiplier = Some(multiplier);
    raw[literal.end] = 172u8.rotate_left((key & 7) as u32)
        ^ key
        ^ (plan.format.seed(literal.end) ^ (key as u32).wrapping_mul(multiplier)) as u8;
    assert!(literal_call_anchor(&raw, &plan, &literal, &mut 1024).unwrap());
    literal.key = Some(key ^ 1);
    assert!(!literal_call_anchor(&raw, &plan, &literal, &mut 1024).unwrap());
}

#[test]
fn recovery_preserves_protected_strings_and_bounds_hash_work() {
    let (raw, program, plan) = fixture("so.");
    for packed in [false, true] {
        let mut all = literals(&raw, &program, &plan, false, 65535, false).unwrap();
        let blocker = all.iter_mut().find(|l| l.start == 2).unwrap();
        if packed {
            blocker.packed = Some("protected".into());
        } else {
            blocker.value = "longLiteral".into();
        }
        recover_literals(&raw, &program, &plan, &mut all).unwrap();
        assert!(!all.iter().any(|l| l.start == 5));
    }
    let candidate = literals(&raw, &program, &plan, true, 64, false)
        .unwrap()
        .into_iter()
        .find(|l| l.start == 5)
        .unwrap();
    assert_eq!(
        literal_call_anchor(&raw, &plan, &candidate, &mut 0)
            .unwrap_err()
            .message,
        "Literal recovery operation budget"
    );
}

fn append_literal(raw: &mut Vec<u8>, plan: &Plan, bytes: &[u8]) -> usize {
    let at = raw.len();
    raw.extend([
        plan.format.state_step ^ 7,
        11 ^ 7,
        17,
        bytes.len() as u8 ^ 7,
    ]);
    let mut seed = plan.format.seed(raw.len());
    for byte in bytes {
        raw.push((byte.wrapping_add(seed as u8) ^ (seed >> 16) as u8) ^ 7);
        seed = seed.wrapping_add(2654435769);
    }
    at
}
fn swallowing_fixture() -> (Vec<u8>, Program, Plan, usize) {
    let (_, _, plan) = fixture("so.");
    let mut raw = vec![
        172 ^ 7,
        7,
        7,
        4 ^ 7,
        0,
        0,
        0,
        0,
        plan.format.state_step ^ 7,
        11 ^ 7,
        17,
        10 ^ 7,
    ];
    let mut seed = plan.format.seed(12);
    for byte in [0u8, 8] {
        raw.push((byte.wrapping_add(seed as u8) ^ (seed >> 16) as u8) ^ 7);
        seed = seed.wrapping_add(2654435769);
    }
    let field = append_literal(&mut raw, &plan, b"field");
    append_literal(&mut raw, &plan, b"\0\x03abc");
    let salt = plan.meta["input_data_profile"]["flow-call"][2]["salt"]
        .as_u64()
        .unwrap() as u32;
    let mut hash = salt;
    for value in [172, 7, 4] {
        hash = hash.wrapping_mul(16777619) ^ value;
    }
    hash = hash.wrapping_mul(16777619);
    for byte in &raw[8..12] {
        hash = (hash ^ *byte as u32).wrapping_mul(16777619);
    }
    for (i, byte) in hash.to_be_bytes().iter().enumerate() {
        raw[4 + i] = byte ^ 7;
    }
    let program = Program {
        sha256: String::new(),
        byte_length: raw.len(),
        code_length: raw.len(),
        strings: Vec::new(),
    };
    (raw, program, plan, field)
}

#[test]
fn checked_operands_cannot_claim_a_packed_literal_and_hide_later_fields() {
    let (raw, program, plan, field) = swallowing_fixture();
    let unfiltered = literals(&raw, &program, &plan, false, 65535, false).unwrap();
    assert!(unfiltered
        .iter()
        .any(|l| l.start == 8 && l.packed.is_some() && l.end > field));
    assert!(!unfiltered.iter().any(|l| l.start == field));
    let all = response_literals(&raw, &program, &plan).unwrap();
    assert!(!all.iter().any(|l| l.start == 8));
    assert!(all.iter().any(|l| l.start == field && l.value == "field"));
    assert!(all.iter().any(|l| l.packed.as_deref() == Some("abc")));
}

#[test]
fn unverified_or_truncated_frames_do_not_suppress_literal_candidates() {
    for mode in 0..3 {
        let (mut raw, mut program, mut plan, field) = swallowing_fixture();
        match mode {
            0 => raw[4] ^= 1,
            1 => plan.meta["input_data_profile"] = Value::Null,
            _ => {
                raw[3] = 63 ^ 7;
                program.byte_length = raw.len();
            }
        }
        let all = response_literals(&raw, &program, &plan).unwrap();
        assert!(
            all.iter().any(|l| l.start == 8 && l.packed.is_some()),
            "mode {mode}"
        );
        assert!(!all.iter().any(|l| l.start == field), "mode {mode}");
    }
}

fn branch_fixture() -> (Vec<u8>, Vec<Literal>, Plan) {
    let (_, _, mut plan) = fixture("so.");
    plan.meta["input_data_profile"] = json!({"literal-empty":[3,[],{"tags":[55]}],"flow-branch":[40,vec![0;12],{"target":[3,4,5],"key":6,"keyMask":127,"flag":7,"flagMask":0,"alternateTarget":[8,9,10],"alternateKey":11,"alternateKeyMask":127}],"flow-jump":[41,[0,0,0,127],{"b16":0,"b8":1,"b0":2,"key":3}]});
    let mut raw = vec![0; 48];
    raw[4..7].copy_from_slice(&[3 ^ 7, 55 ^ 7, 0]);
    raw[7] = 40 ^ 7;
    for (i, byte) in [0u8, 0, 0, 0, 0, 20, 7 ^ 127, 1, 0, 0, 20, 7 ^ 127]
        .iter()
        .enumerate()
    {
        raw[8 + i] = byte ^ 7;
    }
    raw[20..25].copy_from_slice(&[41 ^ 7, 0 ^ 7, 0 ^ 7, 32 ^ 7, 127]);
    raw[32] = 3 ^ 7;
    raw[33] = 11 ^ 7;
    let make = |start, end, packed: Option<String>| Literal {
        key: Some(7),
        start,
        end,
        value: if packed.is_some() {
            "binary operand".into()
        } else {
            "boundary".into()
        },
        packed,
        continuation: 0,
        pool_offset: 0,
    };
    (
        raw,
        vec![
            make(0, 4, None),
            make(18, 27, Some("binary operand".into())),
            make(32, 36, None),
        ],
        plan,
    )
}
#[test]
fn branch_operands_are_removed_only_after_a_second_string_boundary_agrees() {
    let (raw, literals, plan) = branch_fixture();
    let conflicts = conflicting_literals(&raw, &literals, &plan).unwrap();
    assert!(conflicts.contains(&18));
    assert!(!conflicts.contains(&32));
    for mode in 0..5 {
        let (mut bytes, mut literals, mut plan) = branch_fixture();
        match mode {
            0 => bytes[33] ^= 1,
            1 => {
                literals.pop();
            }
            2 => plan.meta["input_data_profile"]["flow-branch"] = Value::Null,
            3 => bytes[13] = 2 ^ 7,
            _ => bytes[24] ^= 1,
        }
        assert!(
            !conflicting_literals(&bytes, &literals, &plan)
                .unwrap()
                .contains(&18),
            "mode {mode}"
        );
    }
}

#[test]
fn conditional_target_path_needs_its_current_key_and_a_second_literal() {
    let (_, _, mut plan) = fixture("so.");
    plan.meta["input_data_profile"] = json!({"flow-branch-fallthrough":[42,vec![0;5],{"target":[1,2,3],"key":4,"keyMask":127}],"flow-read":[43,vec![0;4],{}],"flow-unary":[44,[0],{}]});
    let mut raw = vec![0; 40];
    raw[4..10].copy_from_slice(&[42 ^ 7, 0 ^ 7, 0 ^ 7, 0 ^ 7, 18 ^ 7, 127]);
    raw[18] = 43 ^ 7;
    raw[23] = 44 ^ 7;
    raw[25..27].copy_from_slice(&[3 ^ 7, 11 ^ 7]);
    let make = |start, end| Literal {
        key: Some(7),
        start,
        end,
        value: "candidate".into(),
        packed: None,
        continuation: 0,
        pool_offset: 0,
    };
    let literals = vec![make(0, 4), make(20, 25), make(25, 30)];
    assert!(conflicting_literals(&raw, &literals, &plan)
        .unwrap()
        .contains(&20));
    for mode in 0..4 {
        let mut bytes = raw.clone();
        let mut literals = literals.clone();
        let mut plan = plan.clone();
        match mode {
            0 => bytes[9] ^= 1,
            1 => {
                literals.pop();
            }
            2 => literals[2].key = Some(8),
            _ => plan.meta["input_data_profile"]["flow-read"] = Value::Null,
        }
        assert!(
            !conflicting_literals(&bytes, &literals, &plan)
                .unwrap()
                .contains(&20),
            "mode {mode}"
        );
    }
}

#[test]
fn branch_alternative_must_also_reach_an_independent_literal_boundary() {
    let (mut raw, literals, plan) = branch_fixture();
    raw[13] = 40 ^ 7;
    assert!(conflicting_literals(&raw, &literals, &plan)
        .unwrap()
        .contains(&18));
    raw[19] ^= 1;
    assert!(!conflicting_literals(&raw, &literals, &plan)
        .unwrap()
        .contains(&18));
}

#[test]
fn closure_capture_count_is_bounded_and_cannot_skip_an_unknown_tail() {
    let (_, _, mut plan) = fixture("so.");
    plan.meta["input_data_profile"] = json!({"flow-closure":[45,vec![0;7],{"count":6,"countMask":80,"maxCount":64}],"flow-unary":[46,[0],{}]});
    let mut raw = vec![0; 40];
    raw[4] = 45 ^ 7;
    raw[11] = 2 ^ 80 ^ 7;
    raw[14] = 46 ^ 7;
    raw[16..18].copy_from_slice(&[3 ^ 7, 11 ^ 7]);
    let make = |start, end| Literal {
        key: Some(7),
        start,
        end,
        value: "candidate".into(),
        packed: None,
        continuation: 0,
        pool_offset: 0,
    };
    let literals = vec![make(0, 4), make(11, 16), make(16, 22)];
    assert!(conflicting_literals(&raw, &literals, &plan)
        .unwrap()
        .contains(&11));
    for count in [3u8, 65, 255] {
        let mut bytes = raw.clone();
        bytes[11] = count ^ 80 ^ 7;
        assert!(!conflicting_literals(&bytes, &literals, &plan)
            .unwrap()
            .contains(&11));
    }
}

#[test]
fn property_boundary_accepts_only_the_supported_pool_form_and_verified_tail() {
    let (_, _, mut plan) = fixture("so.");
    plan.meta["input_data_profile"] =
        json!({"flow-property-pool":[47,[],{"marker":127,"width":5}],"flow-unary":[46,[0],{}]});
    let mut raw = vec![0; 24];
    raw[4] = 47 ^ 7;
    raw[5] = 127 ^ 7;
    raw[9] = 46 ^ 7;
    raw[11..13].copy_from_slice(&[3 ^ 7, 11 ^ 7]);
    let make = |start, end| Literal {
        key: Some(7),
        start,
        end,
        value: "candidate".into(),
        packed: None,
        continuation: 0,
        pool_offset: 0,
    };
    let literals = vec![make(0, 4), make(7, 12), make(11, 18)];
    assert!(conflicting_literals(&raw, &literals, &plan)
        .unwrap()
        .contains(&7));
    for mode in 0..3 {
        let mut bytes = raw.clone();
        let mut literals = literals.clone();
        match mode {
            0 => bytes[5] = 126 ^ 7,
            1 => {
                literals.pop();
            }
            _ => bytes.truncate(8),
        }
        assert!(!conflicting_literals(&bytes, &literals, &plan)
            .unwrap()
            .contains(&7));
    }
}
