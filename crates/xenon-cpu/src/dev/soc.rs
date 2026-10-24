//! SoC register block (0x8000_0200_0006_0000): stores what is written, logs reads of registers
//! nobody wrote yet so the next missing behavior shows up in the trace.
use crate::mem::{BusError, Device};
use std::collections::HashMap;

#[derive(Default)]
pub struct SocRegs {
    regs: HashMap<u64, u64>,
    pub unknown_reads: Vec<u64>,
}

impl Device for SocRegs {
    fn read(&mut self, off: u64, size: usize) -> Result<u64, BusError> {
        match self.regs.get(&off) {
            Some(v) => Ok(*v),
            None => {
                eprintln!("soc: read of unwritten reg +{off:#x} size {size}");
                self.unknown_reads.push(off);
                Ok(0)
            }
        }
    }

    fn write(&mut self, off: u64, _size: usize, val: u64) -> Result<(), BusError> {
        self.regs.insert(off, val);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::Device;

    #[test]
    fn written_value_reads_back() {
        let mut d = SocRegs::default();
        d.write(0x1010, 8, 0xD000_0000_0000_0000).unwrap();
        assert_eq!(d.read(0x1010, 8).unwrap(), 0xD000_0000_0000_0000);
    }

    #[test]
    fn unwritten_register_reads_zero_and_is_recorded() {
        let mut d = SocRegs::default();
        assert_eq!(d.read(0x20, 4).unwrap(), 0);
        assert_eq!(d.unknown_reads, vec![0x20]);
    }
}
