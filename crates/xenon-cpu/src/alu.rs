/// PPC mask: ones from bit `mb` through `me` (IBM numbering, bit 0 = MSB); wraps when `mb > me`.
pub fn mask64(mb: u32, me: u32) -> u64 {
    let hi = u64::MAX >> mb;
    let lo = u64::MAX << (63 - me);
    if mb <= me { hi & lo } else { hi | lo }
}

pub fn rotl32(v: u32, n: u32) -> u32 {
    v.rotate_left(n & 31)
}

pub fn rotl64(v: u64, n: u32) -> u64 {
    v.rotate_left(n & 63)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask64_normal() {
        assert_eq!(mask64(56, 63), 0xFF);
    }

    #[test]
    fn mask64_wraps() {
        assert_eq!(mask64(60, 3), 0xF000_0000_0000_000F);
    }

    #[test]
    fn rotl32_wraps() {
        assert_eq!(rotl32(0x8000_0001, 1), 3);
    }
}
