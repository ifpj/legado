use base64::{engine::general_purpose::STANDARD, Engine};
use cf_gateway::{
    data::{program, Format},
    wire::Cipher,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
#[test]
fn independent_wire_vectors() {
    let v: Value =
        serde_json::from_str(include_str!("fixtures/managed-wire-vectors.json")).unwrap();
    let c = Cipher::new(
        v["modulus"].as_str().unwrap(),
        v["alphabet"].as_str().unwrap(),
        b"vector-mask-2026".to_vec(),
        Some((0..128).collect()),
    )
    .unwrap();
    for row in v["vectors"].as_array().unwrap() {
        assert_eq!(
            c.encode(&row["input"]).unwrap(),
            row["encoded"].as_str().unwrap()
        );
    }
}
#[test]
fn independent_program_constants() {
    let v: Value = serde_json::from_str(include_str!("fixtures/program-constants.json")).unwrap();
    let mut raw = vec![0; v["prefixLength"].as_u64().unwrap() as usize];
    raw.extend(STANDARD.decode(v["tailBase64"].as_str().unwrap()).unwrap());
    let f = Format {
        opcode_multiplier: None,
        byte_offset: 37,
        initial_key: 191,
        state_step: 244,
        state_multiplier: 48294,
        state_increment: 31651,
        hash_salt: 1612195754,
        char_xor: 133,
        state_quadratic: 0,
        char_mode: "before-subtract".into(),
    };
    assert_eq!(
        serde_json::json!(program(&raw, &f, false).unwrap().strings),
        v["expectedStrings"]
    );
    assert!(program(&raw[..raw.len() - 1], &f, false).is_err());
}
#[test]
fn large_packet_matches_independent_original_digest_and_secrets_are_fresh() {
    let parameters: Value =
        serde_json::from_str(include_str!("fixtures/managed-wire-vectors.json")).unwrap();
    let bulk: Value =
        serde_json::from_str(include_str!("fixtures/managed-bulk-vector.json")).unwrap();
    let cipher = Cipher::new(
        parameters["modulus"].as_str().unwrap(),
        parameters["alphabet"].as_str().unwrap(),
        b"vector-mask-2026".to_vec(),
        Some((0..128).collect()),
    )
    .unwrap();
    let text = (0..700)
        .map(|i| hex::encode(Sha256::digest(i.to_string().as_bytes())))
        .collect::<String>();
    let encoded = cipher.encode(&serde_json::json!({"text":text})).unwrap();
    assert_eq!(
        encoded.len() as u64,
        bulk["encodedLength"].as_u64().unwrap()
    );
    assert_eq!(
        hex::encode(Sha256::digest(encoded.as_bytes())),
        bulk["encodedSha256"]
    );
    let fresh = || {
        Cipher::new(
            parameters["modulus"].as_str().unwrap(),
            parameters["alphabet"].as_str().unwrap(),
            b"vector-mask-2026".to_vec(),
            None,
        )
        .unwrap()
    };
    assert_ne!(
        fresh().encode(&serde_json::json!({})).unwrap(),
        fresh().encode(&serde_json::json!({})).unwrap()
    );
    let wrong = Cipher::new(
        parameters["modulus"].as_str().unwrap(),
        parameters["alphabet"].as_str().unwrap(),
        b"wrong-mask".to_vec(),
        Some((0..128).collect()),
    )
    .unwrap();
    assert_ne!(
        wrong.encode(&parameters["vectors"][0]["input"]).unwrap(),
        parameters["vectors"][0]["encoded"]
    );
}
