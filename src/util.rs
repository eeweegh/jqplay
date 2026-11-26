// Screen coordinates are u16; a larger value saturates instead of wrapping around.
pub fn to_u16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

// Moves `index` by `delta` through `len` items (`len` > 0), wrapping at both ends.
pub fn cycle(index: usize, delta: isize, len: usize) -> usize {
    let shift = delta.unsigned_abs() % len;
    if delta >= 0 { (index + shift) % len } else { (index + len - shift) % len }
}

// Standard base64 with padding (RFC 4648), for the OSC 52 clipboard sequence.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (plain, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), want, "{plain:?}");
        }
        assert_eq!(base64("\u{e9}".as_bytes()), "w6k=");
    }

    #[test]
    fn cycle_wraps_in_both_directions() {
        assert_eq!(cycle(0, -1, 3), 2);
        assert_eq!(cycle(2, 1, 3), 0);
        assert_eq!(cycle(1, 7, 3), 2);
        assert_eq!(cycle(1, -7, 3), 0);
        assert_eq!(cycle(0, 5, 1), 0);
    }

    #[test]
    fn to_u16_saturates() {
        assert_eq!(to_u16(70_000), u16::MAX);
        assert_eq!(to_u16(12), 12);
    }
}
