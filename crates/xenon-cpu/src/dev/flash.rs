//! Flash window the CB stages read through MMIO: raw NAND image, big-endian, 0xFF past the end.
use crate::mem::{BusError, Device};

pub struct FlashWindow {
    data: Vec<u8>,
}

impl FlashWindow {
    pub fn new(data: Vec<u8>) -> FlashWindow {
        FlashWindow { data }
    }
}

impl Device for FlashWindow {
    fn read(&mut self, off: u64, size: usize) -> Result<u64, BusError> {
        let mut v = 0u64;
        for i in 0..size as u64 {
            let b = self.data.get((off + i) as usize).copied().unwrap_or(0xFF);
            v = v << 8 | b as u64;
        }
        Ok(v)
    }

    fn write(&mut self, _off: u64, _size: usize, _val: u64) -> Result<(), BusError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::Device;

    #[test]
    fn reads_big_endian_from_image() {
        let mut d = FlashWindow::new(vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
        assert_eq!(d.read(0, 4).unwrap(), 0x1122_3344);
        assert_eq!(d.read(0, 8).unwrap(), 0x1122_3344_5566_7788);
        assert_eq!(d.read(7, 1).unwrap(), 0x88);
    }

    #[test]
    fn past_the_image_reads_ff() {
        let mut d = FlashWindow::new(vec![0x11]);
        assert_eq!(d.read(0, 2).unwrap(), 0x11FF);
    }
}
