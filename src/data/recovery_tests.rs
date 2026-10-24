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
