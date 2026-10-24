//! Fuse lines at SoC page 0x2_0000: 12 lines of 64 bits, one per 0x200 bytes (CB_B 0x4F3C..0x4FA0 reads
//! them with stride 0x200 and halts with POST 0x9B when line 0 has no top bits set).
use crate::mem::{BusError, Device};

pub const FUSE_LINES: usize = 12;
pub const FUSE_STRIDE: u64 = 0x200;

pub struct FuseLines {
    lines: [u64; FUSE_LINES],
}

impl FuseLines {
    /// Parses the XeLL `fuseset NN: <16 hex digits>` text; lines missing from the text stay zero
    /// (the dump folds 08..11 into one `08-11:` row).
    pub fn from_text(text: &str) -> FuseLines {
        let mut lines = [0u64; FUSE_LINES];
        for l in text.lines() {
            let Some(rest) = l.strip_prefix("fuseset ") else { continue };
            let Some((n, v)) = rest.split_once(':') else { continue };
            if let (Ok(n), Ok(v)) = (n.trim().parse::<usize>(), u64::from_str_radix(v.trim(), 16)) {
                if n < FUSE_LINES {
                    lines[n] = v;
                }
            }
        }
        FuseLines { lines }
    }
}

impl Device for FuseLines {
    fn read(&mut self, off: u64, size: usize) -> Result<u64, BusError> {
        let (line, within) = ((off / FUSE_STRIDE) as usize, off % FUSE_STRIDE);
        if line >= FUSE_LINES || within + size as u64 > 8 {
            eprintln!("fuses: read +{off:#x} size {size} outside a fuse line, returning 0");
            return Ok(0);
        }
        Ok(self.lines[line] << (8 * within) >> (64 - 8 * size as u64))
    }

    fn write(&mut self, _off: u64, _size: usize, _val: u64) -> Result<(), BusError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "fuseset 00: c0ffffffffffffff\nfuseset 03: 0123456789abcdef\nfuseset 08-11: 0000000000000000\n";

    #[test]
    fn lines_read_at_stride_0x200() {
        let mut f = FuseLines::from_text(TEXT);
        assert_eq!(f.read(0, 8).unwrap(), 0xc0ff_ffff_ffff_ffff);
        assert_eq!(f.read(3 * 0x200, 8).unwrap(), 0x0123_4567_89ab_cdef);
        assert_eq!(f.read(1 * 0x200, 8).unwrap(), 0);
    }

    #[test]
    fn narrow_reads_are_big_endian_slices() {
        let mut f = FuseLines::from_text(TEXT);
        assert_eq!(f.read(3 * 0x200, 4).unwrap(), 0x0123_4567);
        assert_eq!(f.read(3 * 0x200 + 4, 4).unwrap(), 0x89ab_cdef);
        assert_eq!(f.read(3 * 0x200 + 7, 1).unwrap(), 0xef);
    }

    #[test]
    fn past_the_twelfth_line_reads_zero() {
        let mut f = FuseLines::from_text(TEXT);
        assert_eq!(f.read(12 * 0x200, 8).unwrap(), 0);
    }
}
