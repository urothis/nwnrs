/// Returns the exact NWScript runtime hash for a cooked byte string.
///
/// Upstream computes this as `XXH32(bytes, 0) ^ XXH32("", 0, 0)`.
pub fn nwscript_string_hash_bytes(bytes: &[u8]) -> i32 {
    let null_hash = xxh32(&[], 0);
    let hash = xxh32(bytes, 0) ^ null_hash;
    i32::from_ne_bytes(hash.to_ne_bytes())
}

/// Returns the exact NWScript runtime hash for a UTF-8 string slice.
pub fn nwscript_string_hash(input: &str) -> i32 {
    nwscript_string_hash_bytes(input.as_bytes())
}

fn xxh32(input: &[u8], seed: u32) -> u32 {
    const PRIME32_1: u32 = 2_654_435_761;
    const PRIME32_2: u32 = 2_246_822_519;
    const PRIME32_3: u32 = 3_266_489_917;
    const PRIME32_4: u32 = 668_265_263;
    const PRIME32_5: u32 = 374_761_393;

    let len = input.len();
    let mut index = 0usize;
    let mut h32 = if len >= 16 {
        let mut v1 = seed.wrapping_add(PRIME32_1).wrapping_add(PRIME32_2);
        let mut v2 = seed.wrapping_add(PRIME32_2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(PRIME32_1);

        while index <= len - 16 {
            v1 = xxh32_round(v1, read_le_u32(input, index));
            index += 4;
            v2 = xxh32_round(v2, read_le_u32(input, index));
            index += 4;
            v3 = xxh32_round(v3, read_le_u32(input, index));
            index += 4;
            v4 = xxh32_round(v4, read_le_u32(input, index));
            index += 4;
        }

        rotate_left(v1, 1)
            .wrapping_add(rotate_left(v2, 7))
            .wrapping_add(rotate_left(v3, 12))
            .wrapping_add(rotate_left(v4, 18))
    } else {
        seed.wrapping_add(PRIME32_5)
    };

    h32 = h32.wrapping_add(u32::try_from(len).ok().unwrap_or(u32::MAX));

    while index + 4 <= len {
        h32 = h32.wrapping_add(read_le_u32(input, index).wrapping_mul(PRIME32_3));
        h32 = rotate_left(h32, 17).wrapping_mul(PRIME32_4);
        index += 4;
    }

    while index < len {
        let byte = input.get(index).copied().unwrap_or(0);
        h32 = h32.wrapping_add(u32::from(byte).wrapping_mul(PRIME32_5));
        h32 = rotate_left(h32, 11).wrapping_mul(PRIME32_1);
        index += 1;
    }

    xxh32_avalanche(h32)
}

fn xxh32_round(seed: u32, input: u32) -> u32 {
    const PRIME32_1: u32 = 2_654_435_761;
    const PRIME32_2: u32 = 2_246_822_519;

    let seed = seed.wrapping_add(input.wrapping_mul(PRIME32_2));
    rotate_left(seed, 13).wrapping_mul(PRIME32_1)
}

fn xxh32_avalanche(mut hash: u32) -> u32 {
    const PRIME32_2: u32 = 2_246_822_519;
    const PRIME32_3: u32 = 3_266_489_917;

    hash ^= hash >> 15;
    hash = hash.wrapping_mul(PRIME32_2);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(PRIME32_3);
    hash ^= hash >> 16;
    hash
}

fn read_le_u32(bytes: &[u8], offset: usize) -> u32 {
    let b0 = bytes.get(offset).copied().unwrap_or(0);
    let b1 = bytes.get(offset + 1).copied().unwrap_or(0);
    let b2 = bytes.get(offset + 2).copied().unwrap_or(0);
    let b3 = bytes.get(offset + 3).copied().unwrap_or(0);
    u32::from_le_bytes([b0, b1, b2, b3])
}

fn rotate_left(value: u32, amount: u32) -> u32 {
    value.rotate_left(amount)
}

#[cfg(test)]
mod tests {
    use super::{nwscript_string_hash, nwscript_string_hash_bytes};

    #[test]
    fn matches_upstream_known_hash_values() {
        assert_eq!(nwscript_string_hash(""), 0);
        assert_eq!(nwscript_string_hash("hello"), -104060164);
    }

    #[test]
    fn hashes_raw_byte_sequences_not_utf8_codepoints() {
        let hash = nwscript_string_hash_bytes(&[b'"', b'\n', b'\\', 0xff, 0x80]);

        assert_ne!(hash, 0);
    }
}
