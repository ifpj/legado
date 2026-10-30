//! Native finite wire formats; no script interpreter or external solver.
use crate::{require, Error, Result};
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use num_bigint::BigUint;
use rand::RngCore;
use serde_json::Value;

#[allow(dead_code)]
pub(crate) mod kernel;

pub fn deflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut output = vec![0; data.len() * 2 + 64];
    let count = unsafe {
        kernel::cf_deflate_fixed(data.as_ptr(), data.len(), output.as_mut_ptr(), output.len())
    };
    require(count >= 0, "DEFLATE bounds")?;
    output.truncate(count as usize);
    Ok(output)
}
pub fn flow(encoded: &str, ray: &str) -> Result<Vec<u8>> {
    crate::cpu::check()?;
    require(
        !ray.is_empty() && ray.len() <= 128 && ray.is_ascii() && encoded.len() <= 12 * 1024 * 1024,
        "Flow input bounds",
    )?;
    let key = ray
        .bytes()
        .chain(b"_0".iter().copied())
        .fold(32u8, |a, b| a ^ b) as i32;
    flow_bytes(&STANDARD.decode(encoded)?, key)
}
pub(crate) fn flow_bytes(raw: &[u8], key: i32) -> Result<Vec<u8>> {
    crate::cpu::check()?;
    let mut decoded = Vec::with_capacity(raw.len());
    for (i, b) in raw.iter().enumerate() {
        if i % 16384 == 0 {
            crate::cpu::check()?;
        }
        decoded.push(((*b as i32 - key - (i % 65535) as i32 + 65535).rem_euclid(255)) as u8);
    }
    Ok(decoded)
}
pub fn checked_spans(
    raw: &[u8],
    format: &crate::program::data::Format,
    opcode: u8,
    salt: u32,
) -> Result<Vec<(usize, usize)>> {
    if format.opcode_multiplier.is_some() {
        require(raw.len() <= 1024 * 1024, "Checked span bounds")?;
        let keys = format.instruction_keys(opcode);
        let mut output = Vec::new();
        let mut budget = 64 * 1024 * 1024usize;
        for at in 0..raw.len().saturating_sub(7) {
            if at % 256 == 0 {
                crate::cpu::check()?;
            }
            for &(_, key) in keys.at(format, raw[at], at) {
                let size = (format.byte(raw[at + 1], key) as usize) << 16
                    | (format.byte(raw[at + 2], key) as usize) << 8
                    | format.byte(raw[at + 3], key) as usize;
                if !(4..=65536).contains(&size) || at + 8 + size > raw.len() {
                    continue;
                }
                require(size <= budget, "Checked span operation budget")?;
                budget -= size;
                if let Some(end) =
                    crate::program::data::framed_end(raw, at, key, opcode, format, salt)
                {
                    require(output.len() < 8192, "Checked span capacity")?;
                    output.push((at, end));
                }
            }
        }
        return Ok(output);
    }
    let keys = format.keys(opcode);
    let mut output = vec![0u32; 16384];
    let count = unsafe {
        kernel::cf_checked_instruction_spans(
            raw.as_ptr(),
            raw.len(),
            keys.as_ptr(),
            format.byte_offset,
            opcode,
            salt,
            output.as_mut_ptr(),
            8192,
        )
    };
    crate::cpu::check()?;
    require(count >= 0, "Checked span bounds")?;
    Ok(output[..count as usize * 2]
        .chunks_exact(2)
        .map(|x| (x[0] as usize, x[1] as usize))
        .collect())
}
pub fn work(prefix: &str, suffix: &str, budget_ms: u64, first: u64, step: u64) -> Result<[u64; 4]> {
    work_limited(prefix, suffix, budget_ms as f64, first, step, 2_000_000)
}
pub(crate) fn work_limited(
    prefix: &str,
    suffix: &str,
    budget_ms: f64,
    first: u64,
    step: u64,
    attempts: u64,
) -> Result<[u64; 4]> {
    require(
        crate::re(r"^[a-fA-F0-9]{16,32}\|[0-9]{13}\|[0-9]+\|$").is_match(prefix)
            && !suffix.is_empty()
            && suffix.len() <= 4096
            && suffix.is_ascii()
            && budget_ms.is_finite()
            && budget_ms > 0.
            && budget_ms <= 1000.
            && (1..=2_000_000).contains(&attempts)
            && (1..=64).contains(&step)
            && first <= 9_000_000_000_000_000,
        "Proof input bounds",
    )?;
    let mut output = [0u64; 4];
    let status = unsafe {
        kernel::cf_sha256_work_strided(
            prefix.as_ptr(),
            prefix.len(),
            suffix.as_ptr(),
            suffix.len(),
            ((budget_ms * 1000.) as u64).max(1),
            first,
            attempts,
            step,
            output.as_mut_ptr(),
        )
    };
    crate::cpu::check()?;
    require(status == 0, "SHA-256 work bounds")?;
    Ok(output)
}
pub(crate) fn ordered(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut numeric: Vec<_> = map
                .keys()
                .filter_map(|k| {
                    k.parse::<u32>()
                        .ok()
                        .filter(|n| *n < u32::MAX && n.to_string() == *k)
                        .map(|n| (n, k))
                })
                .collect();
            numeric.sort_unstable_by_key(|x| x.0);
            let mut out = serde_json::Map::new();
            for (_, k) in numeric {
                out.insert(k.clone(), ordered(&map[k]));
            }
            for (k, v) in map {
                if !out.contains_key(k) {
                    out.insert(k.clone(), ordered(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(v) => Value::Array(v.iter().map(ordered).collect()),
        _ => v.clone(),
    }
}
pub struct Cipher {
    secret: Vec<u8>,
    encrypted: Vec<u8>,
    alphabet: Vec<u8>,
    mask: Vec<u8>,
}
impl Cipher {
    pub fn new(
        modulus: &str,
        alphabet: &str,
        mask: Vec<u8>,
        secret: Option<Vec<u8>>,
    ) -> Result<Self> {
        let mut symbols = alphabet.as_bytes().to_vec();
        symbols.sort_unstable();
        let mut expected =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-$".to_vec();
        expected.sort_unstable();
        require(symbols == expected, "Invalid wire alphabet")?;
        let modulus = BigUint::parse_bytes(modulus.trim_start_matches("0x").as_bytes(), 16)
            .ok_or_else(|| Error::new("INVALID_DATA", "RSA modulus"))?;
        require(modulus.bits() == 1024, "RSA size")?;
        let mut secret = secret.unwrap_or_else(|| {
            let mut v = vec![0u8; 128];
            rand::rng().fill_bytes(&mut v);
            v
        });
        require(
            secret.len() == 128 && !mask.is_empty(),
            "Secret/mask bounds",
        )?;
        secret[0] = 2;
        let encrypted = BigUint::from_bytes_be(&secret)
            .modpow(&BigUint::from(65537u32), &modulus)
            .to_bytes_be();
        let mut padded = vec![0; 128 - encrypted.len()];
        padded.extend(encrypted);
        secret[0] = 0;
        Ok(Self {
            secret,
            encrypted: padded,
            alphabet: alphabet.as_bytes().to_vec(),
            mask,
        })
    }
    pub fn encode(&self, value: &Value) -> Result<String> {
        let mut data = vec![b' '];
        data.extend(serde_json::to_vec(&ordered(value))?);
        data.push(b' ');
        let compressed = if data.len() >= 128 {
            deflate(&data)?
        } else {
            Vec::new()
        };
        let use_compressed = !compressed.is_empty() && compressed.len() < data.len();
        let mut content = vec![253, 1, u8::from(use_compressed)];
        content.extend(if use_compressed { compressed } else { data });
        let padding = (8 - content.len() % 8) % 8;
        content.resize(content.len() + padding, 0);
        let offset = 9 * padding + 40;
        let key: Vec<u8> = self.secret[offset..offset + 16]
            .iter()
            .enumerate()
            .map(|(i, v)| v ^ self.mask[i % self.mask.len()])
            .collect();
        let mut encrypted = vec![0; content.len()];
        let count = unsafe {
            kernel::cf_managed_blocks(
                content.as_ptr(),
                content.len(),
                key.as_ptr(),
                encrypted.as_mut_ptr(),
                encrypted.len(),
            )
        };
        require(count == content.len() as isize, "Managed block bounds")?;
        let mut output = self.encrypted.clone();
        output.push(padding as u8);
        output.extend(encrypted);
        let std = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        Ok(STANDARD_NO_PAD
            .encode(output)
            .bytes()
            .map(|b| self.alphabet[std.iter().position(|v| *v == b).unwrap()] as char)
            .collect())
    }
}
