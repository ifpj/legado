// Protocol port of the retained GPL-3.0 Fanqie source, originally based on
// https://github.com/naiyQAQ/fanqie-assistant . No original APK runtime is used.
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use anyhow::{Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use rand::RngCore;
use sha2::Digest;
use sm3::Sm3;
use std::collections::BTreeMap;

pub const MASTER_KEY: [u8; 16] = [
    0xac, 0x25, 0xc6, 0x7d, 0xdd, 0x8f, 0x38, 0xc1, 0xb3, 0x7a, 0x23, 0x48, 0x82, 0x8e, 0x22, 0x2e,
];
const SIGN_KEY: [u8; 32] = [
    0xac, 0x1a, 0xda, 0xae, 0x95, 0xa7, 0xaf, 0x94, 0xa5, 0x11, 0x4a, 0xb3, 0xb3, 0xa9, 0x7d, 0xd8,
    0, 0x50, 0xaa, 0xa, 0x39, 0x31, 0x4c, 0x40, 0x52, 0x8c, 0xae, 0xc9, 0x52, 0x56, 0xc2, 0x8c,
];

pub fn encrypt(key: &[u8], iv: &[u8], input: &[u8]) -> Result<Vec<u8>> {
    Ok(cbc::Encryptor::<aes::Aes128>::new_from_slices(key, iv)
        .map_err(|_| anyhow!("Invalid AES key or IV"))?
        .encrypt_padded_vec_mut::<Pkcs7>(input))
}
pub fn decrypt(key: &[u8], envelope: &str) -> Result<Vec<u8>> {
    let input = STANDARD.decode(envelope)?;
    ensure!(input.len() >= 32, "Invalid encrypted envelope");
    cbc::Decryptor::<aes::Aes128>::new_from_slices(key, &input[..16])
        .map_err(|_| anyhow!("Invalid AES key"))?
        .decrypt_padded_vec_mut::<Pkcs7>(&input[16..])
        .map_err(|_| anyhow!("Invalid encrypted padding"))
}
fn pad(data: &[u8]) -> Vec<u8> {
    let n = 16 - data.len() % 16;
    let mut result = data.to_vec();
    result.resize(result.len() + n, n as u8);
    result
}
#[derive(Default)]
struct Proto(Vec<u8>);
impl Proto {
    fn uint(&mut self, mut n: u32) {
        while n >= 128 {
            self.0.push((n as u8 & 127) | 128);
            n >>= 7;
        }
        self.0.push(n as u8);
    }
    fn var(&mut self, field: u32, n: u32) {
        self.uint(field << 3);
        self.uint(n);
    }
    fn bytes(&mut self, field: u32, b: &[u8]) {
        self.uint((field << 3) | 2);
        self.uint(b.len() as u32);
        self.0.extend_from_slice(b);
    }
    fn text(&mut self, field: u32, s: &str) {
        self.bytes(field, s.as_bytes());
    }
}
fn sm3(data: &[u8]) -> Vec<u8> {
    Sm3::digest(data).to_vec()
}
fn simon(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut k: Vec<u64> = key
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
        .collect();
    for i in 4..72 {
        let mut t = k[i - 1].rotate_right(3) ^ k[i - 3];
        t ^= t.rotate_right(1);
        k.push(!k[i - 4] ^ t ^ ((0x3dc94c3a046d678b_u64 >> ((i - 4) % 62)) & 1) ^ 3);
    }
    let mut output = Vec::with_capacity(data.len());
    for block in data.chunks_exact(16) {
        let mut x = u64::from_le_bytes(block[..8].try_into().unwrap());
        let mut y = u64::from_le_bytes(block[8..].try_into().unwrap());
        for &round in &k {
            let next = x ^ (y.rotate_left(1) & y.rotate_left(8)) ^ y.rotate_left(2) ^ round;
            x = y;
            y = next;
        }
        output.extend_from_slice(&x.to_le_bytes());
        output.extend_from_slice(&y.to_le_bytes());
    }
    output
}
fn speck(key: &[u8], data: &[u8]) -> Vec<u8> {
    let words: Vec<u64> = key
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let mut k = vec![words[0]];
    let mut l = words[1..].to_vec();
    for i in 0..33 {
        l.push(l[i].rotate_right(8).wrapping_add(k[i]) ^ i as u64);
        k.push(l[i + 3] ^ k[i].rotate_left(3));
    }
    let mut out = Vec::with_capacity(data.len());
    for b in data.chunks_exact(16) {
        let mut y = u64::from_le_bytes(b[..8].try_into().unwrap());
        let mut x = u64::from_le_bytes(b[8..].try_into().unwrap());
        for &round in &k {
            x = x.rotate_right(8).wrapping_add(y) ^ round;
            y = x ^ y.rotate_left(3);
        }
        out.extend_from_slice(&y.to_le_bytes());
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}
pub fn sign_fixed(
    query: &str,
    body: Option<&str>,
    now_ms: u64,
    random: u32,
    ladon_random: [u8; 4],
) -> Result<BTreeMap<String, String>> {
    let timestamp = (now_ms / 1000) as u32;
    let params: std::collections::HashMap<_, _> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let stub = body
        .filter(|s| !s.is_empty())
        .map(|s| format!("{:x}", md5::compute(s.as_bytes())));
    let body_hash = sm3(&stub
        .as_ref()
        .map(|s| hex::decode(s).unwrap())
        .unwrap_or_else(|| vec![0; 16]));
    let query_hash = sm3(if query.is_empty() {
        &[0; 16]
    } else {
        query.as_bytes()
    });
    let mut p = Proto::default();
    p.var(1, 538970409 * 2);
    p.var(2, 2);
    p.var(3, random % 2147483647);
    p.text(4, "1967");
    p.text(5, params.get("device_id").map(String::as_str).unwrap_or(""));
    p.text(6, "1611921764");
    p.text(
        7,
        params.get("version_name").map(String::as_str).unwrap_or(""),
    );
    p.text(8, "v04.04.05-ov-android");
    p.var(9, 134744640);
    p.bytes(10, &[0; 8]);
    p.var(11, 0);
    p.var(12, timestamp.wrapping_mul(2));
    p.bytes(13, &body_hash[..6]);
    p.bytes(14, &query_hash[..6]);
    let mut sub = Proto::default();
    sub.var(1, 1);
    sub.var(2, 1);
    sub.var(3, 1);
    sub.var(7, 3348294860);
    p.bytes(15, &sub.0);
    p.text(16, "");
    p.text(20, "none");
    p.var(21, 738);
    let mut sub = Proto::default();
    sub.text(1, "NX551J");
    sub.var(2, 8196);
    sub.var(4, 2162219008);
    p.bytes(23, &sub.0);
    p.var(25, 2);
    let mut key_input = SIGN_KEY.to_vec();
    key_input.extend_from_slice(&[242, 129, 97, 111]);
    key_input.extend_from_slice(&SIGN_KEY);
    let mut data = vec![242, 247, 252, 255, 242, 247, 252, 255];
    data.extend(simon(&sm3(&key_input), &pad(&p.0)));
    for i in 8..data.len() {
        data[i] ^= data[i % 8];
    }
    data.reverse();
    let mut plaintext = vec![166, 110, 173, 159, 119, 1, 208, 12, 24];
    plaintext.extend(data);
    plaintext.extend([97, 111]);
    let mut argus = vec![242, 129];
    argus.extend(encrypt(
        &md5::compute(&SIGN_KEY[..16]).0,
        &md5::compute(&SIGN_KEY[16..]).0,
        &plaintext,
    )?);
    let mut key_input = ladon_random.to_vec();
    key_input.extend_from_slice(b"1967");
    let key = format!("{:x}", md5::compute(key_input));
    let mut ladon = ladon_random.to_vec();
    ladon.extend(speck(
        key.as_bytes(),
        &pad(format!("{timestamp}-1611921764-1967").as_bytes()),
    ));
    let mut headers = BTreeMap::from([
        ("x-argus".into(), STANDARD.encode(argus)),
        ("x-ladon".into(), STANDARD.encode(ladon)),
        ("x-khronos".into(), timestamp.to_string()),
        ("x-ss-req-ticket".into(), now_ms.to_string()),
    ]);
    if let Some(stub) = stub {
        headers.insert("X-SS-STUB".into(), stub);
    }
    Ok(headers)
}
pub fn sign(query: &str, body: Option<&str>, now_ms: u64) -> Result<BTreeMap<String, String>> {
    let mut rng = rand::rngs::OsRng;
    let random = rng.next_u32();
    let mut bytes = [0; 4];
    // The original JS fills a Uint8Array with the low bytes of four nextInt calls.
    for b in &mut bytes {
        *b = rng.next_u32() as u8;
    }
    sign_fixed(query, body, now_ms, random, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_original_source_vectors() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../tests/signature-vectors.json")).unwrap();
        for v in vectors.as_array().unwrap() {
            let bytes = v["ladonRandom"].as_array().unwrap();
            let actual = sign_fixed(
                v["query"].as_str().unwrap(),
                v["body"].as_str(),
                v["now"].as_u64().unwrap(),
                v["random"].as_u64().unwrap() as u32,
                [
                    bytes[0].as_u64().unwrap() as u8,
                    bytes[1].as_u64().unwrap() as u8,
                    bytes[2].as_u64().unwrap() as u8,
                    bytes[3].as_u64().unwrap() as u8,
                ],
            )
            .unwrap();
            for (key, value) in v["expected"].as_object().unwrap() {
                assert_eq!(actual.get(key).map(String::as_str), value.as_str(), "{key}");
            }
        }
    }
    #[test]
    fn aes_envelope_roundtrip() {
        let iv = [7; 16];
        let mut b = iv.to_vec();
        b.extend(encrypt(&MASTER_KEY, &iv, b"test text").unwrap());
        assert_eq!(
            decrypt(&MASTER_KEY, &STANDARD.encode(b)).unwrap(),
            b"test text"
        );
    }
    #[test]
    fn signature_has_required_headers() {
        let h = sign_fixed(
            "device_id=123&version_name=7.3.9.32",
            None,
            1791302400000,
            123,
            [1, 2, 3, 4],
        )
        .unwrap();
        assert_eq!(h["x-khronos"], "1791302400");
        assert!(STANDARD.decode(&h["x-argus"]).unwrap().len() > 100);
    }
}
