// Screen coordinates are u16; a larger value saturates instead of wrapping around.
pub fn to_u16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

// Moves `index` by `delta` through `len` items (`len` > 0), wrapping at both ends.
pub fn cycle(index: usize, delta: isize, len: usize) -> usize {
    let shift = delta.unsigned_abs() % len;
    if delta >= 0 { (index + shift) % len } else { (index + len - shift) % len }
}

#[cfg(test)]
mod tests {
    use super::*;

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
