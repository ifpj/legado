use super::*;
// The current initializer declares the per-layout CV. Domain words and the
// authenticated UTF-16 envelope are the audited protocol recipe, not answers.
const IV: [u32; 8] = [
    1779033703, 3144134277, 1013904242, 2773480762, 1359893119, 2600822924, 528734635, 1541459225,
];
fn compress(cv: [u32; 8], mut words: [u32; 16], length: u32, flags: u32) -> [u32; 8] {
    fn g(v: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, x: u32, y: u32) {
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
        v[d] = (v[d] ^ v[a]).rotate_right(16);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(12);
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
        v[d] = (v[d] ^ v[a]).rotate_right(8);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(7);
    }
    let mut v = [
        cv[0], cv[1], cv[2], cv[3], cv[4], cv[5], cv[6], cv[7], IV[0], IV[1], IV[2], IV[3], 0, 0,
        length, flags,
    ];
    for _ in 0..7 {
        g(&mut v, 0, 4, 8, 12, words[0], words[1]);
        g(&mut v, 1, 5, 9, 13, words[2], words[3]);
        g(&mut v, 2, 6, 10, 14, words[4], words[5]);
        g(&mut v, 3, 7, 11, 15, words[6], words[7]);
        g(&mut v, 0, 5, 10, 15, words[8], words[9]);
        g(&mut v, 1, 6, 11, 12, words[10], words[11]);
        g(&mut v, 2, 7, 8, 13, words[12], words[13]);
        g(&mut v, 3, 4, 9, 14, words[14], words[15]);
        words = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8].map(|i| words[i]);
    }
    std::array::from_fn(|i| v[i] ^ v[i + 8])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn format(mode: &str) -> Format {
        Format {
            byte_offset: 17,
            initial_key: 0,
            state_step: 3,
            state_multiplier: 0,
            state_increment: 37,
            hash_salt: 123456789,
            char_xor: 61,
            state_quadratic: 0,
            char_mode: mode.into(),
            opcode_multiplier: None,
        }
    }
    fn vector(mode: &str) -> Vec<u8> {
        hex::decode(if mode=="before-subtract"{"0000000000000000000000bc3a5f5d82748557c836c3119e00f91bc43c420bf8dac256f038247f4f294d6b233e1fc187d3b58b3246"}else{"0000000000000000000000ecbccb9da2728557684847192606f9dbe4b6467bd88ac294b038c851c7291d49f3fe2b635753b579f236"}).unwrap()
    }
    #[test]
    fn compression_matches_standard_blake3_root_vectors() {
        for bytes in [b"".as_slice(), b"abc"] {
            let mut block = [0u8; 64];
            block[..bytes.len()].copy_from_slice(bytes);
            let words = std::array::from_fn(|i| {
                u32::from_le_bytes(block[i * 4..i * 4 + 4].try_into().unwrap())
            });
            let result = compress(IV, words, bytes.len() as u32, 11)
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>();
            assert_eq!(result, blake3::hash(bytes).as_bytes());
        }
    }
    #[test]
    fn authenticated_synthetic_vectors_cover_both_character_orders() {
        for mode in ["before-subtract", "after-subtract"] {
            let raw = vector(mode);
            let f = format(mode);
            assert_eq!(
                decode(&raw, 11, 8, 37, 61, &f, [1, 2, 3, 4, 5, 6, 7, 8]).unwrap(),
                "\0\x05hello\0"
            );
            for index in [11, 27, 29, 37, raw.len() - 1] {
                let mut damaged = raw.clone();
                damaged[index] ^= 1;
                assert!(decode(&damaged, 11, 8, 37, 61, &f, [1, 2, 3, 4, 5, 6, 7, 8]).is_err());
            }
            assert!(decode(
                &raw[..raw.len() - 1],
                11,
                8,
                37,
                61,
                &f,
                [1, 2, 3, 4, 5, 6, 7, 8]
            )
            .is_err());
            assert!(decode(&raw, 11, 8, 37, 61, &f, [8, 7, 6, 5, 4, 3, 2, 1]).is_err());
            assert!(decode(&raw, 11, 8, 38, 61, &f, [1, 2, 3, 4, 5, 6, 7, 8]).is_err());
        }
    }
    #[test]
    fn scanner_accepts_one_result_for_converged_instruction_states() {
        let f = format("before-subtract");
        let mut raw = vector("before-subtract");
        let enc = |value: u8, key: u8| (value ^ key).wrapping_add(17);
        raw[5] = enc(3, 0);
        raw[6] = enc(17, 37);
        raw[7] = enc(0, 37);
        for (i, b) in [136, 128, 8].into_iter().enumerate() {
            raw[8 + i] = enc(b, 37);
        }
        let p = Program {
            sha256: String::new(),
            byte_length: raw.len(),
            code_length: raw.len(),
            strings: vec![],
        };
        let plan = Plan {
            format: f,
            meta: json!({"build":"c787ce48cf6b","key_material":{"literal_tag":17,"literal_kind_slot":1},"sealed_literals":{"iv":[1,2,3,4,5,6,7,8]}}),
            alphabet: String::new(),
            modulus: String::new(),
            flow: String::new(),
        };
        let result = literals(&raw, &p, &plan, false, 8, false).unwrap();
        let found = result.iter().filter(|l| l.start == 5).collect::<Vec<_>>();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].packed.as_deref(), Some("hello"));
        assert!(literals_bounded(&raw, &p, &plan, false, 8, false, 1).is_err());
        assert!(!literals(&raw, &p, &plan, false, 7, false)
            .unwrap()
            .iter()
            .any(|l| l.start == 5));
    }
}
pub(super) fn decode(
    raw: &[u8],
    position: usize,
    size: usize,
    key: u8,
    mask: u8,
    f: &Format,
    iv: [u32; 8],
) -> Result<String> {
    crate::cpu::check()?;
    require(
        size <= 65535 && position + size + 34 <= raw.len(),
        "Sealed literal bounds",
    )?;
    let read = |index: usize| {
        let seed = f
            .seed(position)
            .wrapping_add(2654435769u32.wrapping_mul(index as u32));
        let byte = f.byte(raw[position + index], key) ^ mask;
        if f.char_mode == "before-subtract" {
            (byte ^ (seed >> 16) as u8).wrapping_sub(seed as u8)
        } else {
            byte.wrapping_sub(seed as u8) ^ (seed >> 16) as u8
        }
    };
    let length = ((read(16) as usize) << 8) | read(17) as usize;
    require(length <= size, "Sealed content bounds")?;
    let mut block = [0u32; 16];
    block[0] = 827605328;
    block[1] = position as u32;
    block[2] = size as u32;
    block[7] = key as u32 | ((mask as u32) << 8);
    for i in 0..16 {
        block[3 + i / 4] |= (read(i) as u32) << ((i % 4) * 8);
    }
    let cv = compress(iv, block, 32, 27);
    let cvbytes = cv
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let cipher = (0..length).map(|i| read(18 + i)).collect::<Vec<_>>();
    let mut message = vec![0; 8];
    for b in cv[0].to_le_bytes().iter().chain(cipher.iter()) {
        message.extend([0, *b]);
    }
    let digest = blake3::keyed_hash(cvbytes.as_slice().try_into().unwrap(), &message);
    require(
        (0..16).all(|i| read(18 + size + i) == digest.as_bytes()[i]),
        "Sealed literal authentication",
    )?;
    let mut plain = Vec::with_capacity(size);
    let mut stream = [0; 32];
    for (i, b) in cipher.into_iter().enumerate() {
        if i % 1024 == 0 {
            crate::cpu::check()?;
        }
        if i % 32 == 0 {
            let mut b = [0u32; 16];
            b[0] = 827151443;
            b[1] = (i / 32) as u32;
            let words = compress(cv, b, 8, 27);
            for (part, word) in stream.chunks_exact_mut(4).zip(words) {
                part.copy_from_slice(&word.to_le_bytes());
            }
        }
        plain.push((b ^ stream[i % 32]) as u16);
    }
    plain.resize(size, 0);
    String::from_utf16(&plain).map_err(|_| Error::new("INVALID_DATA", "Sealed UTF16"))
}
