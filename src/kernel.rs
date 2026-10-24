//! Finite wire-codec kernels. No JavaScript, DOM, or challenge VM.

use sha2::{Digest, Sha256};
use std::fmt::Write;
use std::time::{Duration, Instant};

const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_BITS: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_BITS: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

struct Bits {
    output: Vec<u8>,
    pending: u64,
    count: u32,
}
impl Bits {
    fn write(&mut self, value: u32, count: u32) {
        self.pending |= (value as u64) << self.count;
        self.count += count;
        while self.count >= 8 {
            self.output.push(self.pending as u8);
            self.pending >>= 8;
            self.count -= 8;
        }
    }
    fn literal(&mut self, symbol: u32) {
        let (value, count) = if symbol < 144 {
            (48 + symbol, 8)
        } else if symbol < 256 {
            (400 + symbol - 144, 9)
        } else if symbol < 280 {
            (symbol - 256, 7)
        } else {
            (192 + symbol - 280, 8)
        };
        self.write(value.reverse_bits() >> (32 - count), count);
    }
}

fn insert(data: &[u8], position: usize, heads: &mut [usize], previous: &mut [usize]) -> usize {
    let key = (((data[position] as usize) << 5)
        ^ ((data[position + 1] as usize) << 2)
        ^ data[position + 2] as usize)
        & 8191;
    let candidate = heads[key];
    previous[position & 32767] = candidate;
    heads[key] = position + 1;
    candidate
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut bits = Bits {
        output: Vec::with_capacity(data.len() + 32),
        pending: 0,
        count: 0,
    };
    let mut heads = vec![0; 8192];
    let mut previous = vec![0; 32768];
    bits.write(1, 1);
    bits.write(1, 2);
    let mut position = 0;
    while position < data.len() {
        let (mut best, mut distance) = (0, 0);
        if position + 3 <= data.len() {
            let mut entry = insert(data, position, &mut heads, &mut previous);
            for _ in 0..2 {
                if entry == 0 {
                    break;
                }
                let candidate = entry - 1;
                if candidate >= position || position - candidate > 32768 {
                    break;
                }
                let maximum = (data.len() - position).min(258);
                let mut matched = 0;
                while matched < maximum && data[position + matched] == data[candidate + matched] {
                    matched += 1;
                }
                if matched > best && matched > 2 {
                    best = matched;
                    distance = position - candidate;
                    if matched == maximum {
                        break;
                    }
                }
                entry = previous[candidate & 32767];
            }
        }
        if best > 2 {
            for (index, (&base, &extra)) in LENGTH_BASE.iter().zip(LENGTH_BITS.iter()).enumerate() {
                if best <= base + (1usize << extra) - 1 {
                    bits.literal(257 + index as u32);
                    bits.write((best - base) as u32, extra);
                    break;
                }
            }
            for (index, (&base, &extra)) in
                DISTANCE_BASE.iter().zip(DISTANCE_BITS.iter()).enumerate()
            {
                if distance <= base + (1usize << extra) - 1 {
                    bits.write((index as u32).reverse_bits() >> 27, 5);
                    bits.write((distance - base) as u32, extra);
                    break;
                }
            }
            for offset in 1..best {
                if position + offset + 3 <= data.len() {
                    insert(data, position + offset, &mut heads, &mut previous);
                }
            }
            position += best;
        } else {
            bits.literal(data[position] as u32);
            position += 1;
        }
    }
    bits.literal(256);
    if bits.count != 0 {
        bits.output.push(bits.pending as u8);
    }
    bits.output
}

/// Encode at most 8 MiB; caller owns both buffers for the duration of this call.
#[no_mangle]
pub unsafe extern "C" fn cf_deflate_fixed(
    input: *const u8,
    length: usize,
    output: *mut u8,
    capacity: usize,
) -> isize {
    if length > 8 * 1024 * 1024 || input.is_null() || output.is_null() {
        return -1;
    }
    let data = std::slice::from_raw_parts(input, length);
    let result = deflate(data);
    if result.len() > capacity {
        return -2;
    }
    std::ptr::copy_nonoverlapping(result.as_ptr(), output, result.len());
    result.len() as isize
}

pub(crate) fn xtea_schedule(words: [u32; 4]) -> [u32; 64] {
    let mut result = [0; 64];
    let mut total = 0u32;
    for pair in result.chunks_exact_mut(2) {
        pair[0] = total.wrapping_add(words[(total & 3) as usize]);
        total = total.wrapping_add(0x9e3779b9);
        pair[1] = total.wrapping_add(words[((total >> 11) & 3) as usize]);
    }
    result
}

pub(crate) fn xtea_pair(mut left: u32, mut right: u32, schedule: &[u32; 64]) -> [u32; 2] {
    for pair in schedule.chunks_exact(2) {
        left = left.wrapping_add(((right << 4) ^ (right >> 5)).wrapping_add(right) ^ pair[0]);
        right = right.wrapping_add(((left << 4) ^ (left >> 5)).wrapping_add(left) ^ pair[1]);
    }
    [left, right]
}

/// Derive the 256 request-local schedules and encrypt whole eight-byte blocks.
#[no_mangle]
pub unsafe extern "C" fn cf_managed_blocks(
    input: *const u8,
    length: usize,
    key: *const u8,
    output: *mut u8,
    capacity: usize,
) -> isize {
    if input.is_null()
        || key.is_null()
        || output.is_null()
        || length > 8 * 1024 * 1024
        || length % 8 != 0
        || capacity < length
    {
        return -1;
    }
    let raw = std::slice::from_raw_parts(input, length);
    let key = std::slice::from_raw_parts(key, 16);
    let words =
        std::array::from_fn(|i| u32::from_be_bytes(key[i * 4..i * 4 + 4].try_into().unwrap()));
    let initial = xtea_schedule(words);
    let schedules: Vec<_> = (0..256)
        .map(|i| {
            let first = xtea_pair(0, i, &initial);
            let second = xtea_pair(0, (i + 1) & 255, &initial);
            xtea_schedule([first[0], first[1], second[0], second[1]])
        })
        .collect();
    let result = std::slice::from_raw_parts_mut(output, length);
    for (i, (block, target)) in raw
        .chunks_exact(8)
        .zip(result.chunks_exact_mut(8))
        .enumerate()
    {
        let words = xtea_pair(
            u32::from_be_bytes(block[..4].try_into().unwrap()),
            u32::from_be_bytes(block[4..].try_into().unwrap()),
            &schedules[i & 255],
        );
        target[..4].copy_from_slice(&words[0].to_be_bytes());
        target[4..].copy_from_slice(&words[1].to_be_bytes());
    }
    length as isize
}

/// Return the best counter, attempts, leading zero bits and actual microseconds.
#[no_mangle]
pub unsafe extern "C" fn cf_sha256_work(
    prefix: *const u8,
    prefix_len: usize,
    suffix: *const u8,
    suffix_len: usize,
    budget_us: u64,
    first: u64,
    maximum: u64,
    output: *mut u64,
) -> i32 {
    cf_sha256_work_strided(
        prefix, prefix_len, suffix, suffix_len, budget_us, first, maximum, 1, output,
    )
}

/// The caller can partition a native search into disjoint counter residues.
#[no_mangle]
pub unsafe extern "C" fn cf_sha256_work_strided(
    prefix: *const u8,
    prefix_len: usize,
    suffix: *const u8,
    suffix_len: usize,
    budget_us: u64,
    first: u64,
    maximum: u64,
    step: u64,
    output: *mut u64,
) -> i32 {
    if prefix.is_null()
        || suffix.is_null()
        || output.is_null()
        || prefix_len > 128
        || suffix_len > 4096
        || budget_us == 0
        || budget_us > 1_000_000
        || maximum == 0
        || maximum > 2_000_000
        || step == 0
        || step > 64
        || first.checked_add((maximum - 1) * step).is_none()
    {
        return -1;
    }
    let Ok(prefix) = std::str::from_utf8(std::slice::from_raw_parts(prefix, prefix_len)) else {
        return -2;
    };
    let Ok(suffix) = std::str::from_utf8(std::slice::from_raw_parts(suffix, suffix_len)) else {
        return -2;
    };
    let started = Instant::now();
    let budget = Duration::from_micros(budget_us);
    let mut input = String::with_capacity(prefix.len() + 20 + suffix.len());
    input.push_str(prefix);
    let mut best_bits = None;
    let mut best_counter = first;
    let mut attempts = 0;
    loop {
        if attempts % 256 == 0 && crate::cpu::stopped() {
            return -3;
        }
        let counter = first + attempts * step;
        input.truncate(prefix.len());
        let _ = write!(input, "{}", counter);
        input.push_str(suffix);
        let digest = Sha256::digest(input.as_bytes());
        let mut bits = 0;
        for byte in digest {
            bits += byte.leading_zeros();
            if byte != 0 {
                break;
            }
        }
        attempts += 1;
        if best_bits.map_or(true, |previous| bits > previous) {
            best_bits = Some(bits);
            best_counter = counter;
        }
        if started.elapsed() >= budget || attempts >= maximum {
            break;
        }
    }
    let result = [
        best_counter,
        attempts,
        best_bits.unwrap_or(0) as u64,
        started.elapsed().as_micros() as u64,
    ];
    std::ptr::copy_nonoverlapping(result.as_ptr(), output, result.len());
    0
}

/// Find checked instruction envelopes, never evaluate their contents.
/// The key table and checksum salt are recovered from the current script.
#[no_mangle]
pub unsafe extern "C" fn cf_checked_instruction_spans(
    input: *const u8,
    length: usize,
    keys: *const u8,
    byte_offset: u8,
    opcode: u8,
    salt: u32,
    output: *mut u32,
    capacity: usize,
) -> isize {
    if input.is_null()
        || keys.is_null()
        || output.is_null()
        || length > 1024 * 1024
        || capacity > 65536
    {
        return -1;
    }
    let raw = std::slice::from_raw_parts(input, length);
    let keys = std::slice::from_raw_parts(keys, 256);
    let mut position = 0usize;
    let mut count = 0usize;
    let mut work_left = 64 * 1024 * 1024;
    while position + 8 <= length {
        let key = keys[raw[position] as usize];
        let read = |index: usize| raw[index].wrapping_sub(byte_offset) ^ key;
        let size = ((read(position + 1) as usize) << 16)
            | ((read(position + 2) as usize) << 8)
            | read(position + 3) as usize;
        let end = position + 8 + size;
        if !(4..=65536).contains(&size) || end > length {
            position += 1;
            continue;
        }
        if size > work_left {
            break;
        }
        work_left -= size;
        let expected = u32::from_be_bytes([
            read(position + 4),
            read(position + 5),
            read(position + 6),
            read(position + 7),
        ]);
        let mut checksum = position as u32 ^ salt;
        for operand in [opcode as u32, key as u32, size as u32] {
            checksum = checksum.wrapping_mul(16777619) ^ operand;
        }
        checksum = checksum.wrapping_mul(16777619);
        for byte in &raw[position + 8..end] {
            checksum = (checksum ^ byte.wrapping_sub(byte_offset) as u32).wrapping_mul(16777619);
        }
        if checksum != expected {
            position += 1;
            continue;
        }
        if count == capacity {
            return -2;
        }
        *output.add(count * 2) = position as u32;
        *output.add(count * 2 + 1) = end as u32;
        count += 1;
        position = end;
    }
    count as isize
}

/// Decode only the supplied character bytes; no instruction or host API runs.
#[no_mangle]
pub unsafe extern "C" fn cf_character_bytes(
    input: *const u8,
    length: usize,
    key: u8,
    byte_offset: u8,
    mask: u8,
    seed: u32,
    after_subtract: u8,
    wide: u8,
    output: *mut u8,
) -> i32 {
    if input.is_null()
        || output.is_null()
        || length > 8 * 1024 * 1024
        || after_subtract > 1
        || wide > 1
        || (wide == 1 && length % 2 != 0)
    {
        return -1;
    }
    let raw = std::slice::from_raw_parts(input, length);
    let mut seed = seed;
    for (index, byte) in raw.iter().enumerate() {
        let value = byte.wrapping_sub(byte_offset) ^ key ^ mask;
        let high = (seed >> 16) as u8;
        let decoded = if after_subtract == 1 {
            value.wrapping_sub(seed as u8) ^ high
        } else {
            (value ^ high).wrapping_sub(seed as u8)
        };
        *output.add(if wide == 1 { index ^ 1 } else { index }) = decoded;
        seed = seed.wrapping_add(2654435769);
    }
    0
}

/// Locate the finite short literal by current program input and transform.
/// Only candidate positions are returned; adjacent key-mask validation remains
/// in the caller. There is no instruction or JavaScript execution.
#[no_mangle]
pub unsafe extern "C" fn cf_short_literal_positions(
    input: *const u8,
    length: usize,
    code_length: usize,
    name: *const u8,
    name_length: usize,
    byte_offset: u8,
    mask: u8,
    salt: u32,
    after_subtract: u8,
    output: *mut u32,
    capacity: usize,
) -> isize {
    if input.is_null()
        || name.is_null()
        || output.is_null()
        || length > 1024 * 1024
        || code_length > length
        || code_length > 512 * 1024
        || !(1..=64).contains(&name_length)
        || after_subtract > 1
        || capacity > 65536
    {
        return -1;
    }
    let raw = std::slice::from_raw_parts(input, length);
    let name = std::slice::from_raw_parts(name, name_length);
    let end = code_length.saturating_sub(name_length + 20);
    let mut count = 0;
    for position in 3..end {
        let key = raw[position].wrapping_sub(byte_offset) ^ name_length as u8;
        let mut seed = (position + 1) as u32 ^ salt;
        seed ^= seed >> 16;
        seed = seed.wrapping_mul(40503);
        seed ^= seed >> 13;
        seed = seed.wrapping_mul(13849);
        seed ^= seed >> 16;
        let mut equal = true;
        for (index, expected) in name.iter().enumerate() {
            let value = raw[position + 1 + index].wrapping_sub(byte_offset) ^ key ^ mask;
            let high = (seed >> 16) as u8;
            let decoded = if after_subtract == 1 {
                value.wrapping_sub(seed as u8) ^ high
            } else {
                (value ^ high).wrapping_sub(seed as u8)
            };
            if decoded != *expected {
                equal = false;
                break;
            }
            seed = seed.wrapping_add(2654435769);
        }
        if equal {
            if count == capacity {
                return -2;
            }
            *output.add(count) = position as u32;
            count += 1;
        }
    }
    count as isize
}

#[cfg(test)]
mod tests {
    use super::deflate;
    #[test]
    fn fixed_empty_stream() {
        assert_eq!(deflate(b""), vec![3, 0]);
    }
}
