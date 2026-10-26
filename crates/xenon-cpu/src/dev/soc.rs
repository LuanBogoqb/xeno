//! SoC register block (0x8000_0200_0006_0000): stores what is written, logs reads of registers
//! nobody wrote yet so the next missing behavior shows up in the trace.
use crate::mem::{BusError, Device};
use std::collections::HashMap;

#[derive(Default)]
pub struct SocRegs {
    name: &'static str,
    base: u64,
    regs: HashMap<u64, u64>,
    /// Bits forced on in reads of a register (status the model always reports), keyed by offset.
    or_masks: HashMap<u64, u64>,
    pub unknown_reads: Vec<u64>,
    trace: bool,
}

impl SocRegs {
    /// Register file named `name` in the logs; `XB_TRACE_MMIO=1` logs every access.
    pub fn named(name: &'static str, base: u64) -> SocRegs {
        SocRegs { name, base, trace: std::env::var_os("XB_TRACE_MMIO").is_some(), ..Default::default() }
    }

    /// Bits of the register at absolute address `addr` that always read as set.
    pub fn or_mask(mut self, addr: u64, mask: u64) -> SocRegs {
        self.or_masks.insert(addr - self.base, mask);
        self
    }

    /// Reset value of the register at absolute address `addr`.
    pub fn preset(mut self, addr: u64, val: u64) -> SocRegs {
        self.regs.insert(addr - self.base, val);
        self
    }
}

impl Device for SocRegs {
    fn read(&mut self, off: u64, size: usize) -> Result<u64, BusError> {
        let forced = self.or_masks.get(&off).copied();
        match self.regs.get(&off).copied().or(forced).map(|v| v | forced.unwrap_or(0)).as_ref() {
            Some(v) => {
                if self.trace {
                    eprintln!("{}: read {:#x} size {size} = {v:#x}", self.name, self.base + off);
                }
                Ok(*v)
            }
            None => {
                if !self.unknown_reads.contains(&off) {
                    eprintln!("{}: read of unwritten reg {:#x} size {size}", self.name, self.base + off);
                    self.unknown_reads.push(off);
                }
                Ok(0)
            }
        }
    }

    fn write(&mut self, off: u64, size: usize, val: u64) -> Result<(), BusError> {
        if self.trace {
            eprintln!("{}: write {:#x} size {size} = {val:#x}", self.name, self.base + off);
        }
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
    fn preset_is_readable_before_any_write() {
        let mut d = SocRegs::named("t", 0x1000).preset(0x1040, 7);
        assert_eq!(d.read(0x40, 8).unwrap(), 7);
        assert!(d.unknown_reads.is_empty());
    }

    #[test]
    fn or_mask_bits_read_set_even_after_a_write() {
        let mut d = SocRegs::named("t", 0x1000).or_mask(0x1040, 0x0004_0000);
        assert_eq!(d.read(0x40, 4).unwrap(), 0x0004_0000);
        d.write(0x40, 4, 0).unwrap();
        d.write(0x40, 4, 0x1).unwrap();
        assert_eq!(d.read(0x40, 4).unwrap(), 0x0004_0001);
        assert!(d.unknown_reads.is_empty());
    }

    #[test]
    fn unwritten_register_reads_zero_and_is_recorded() {
        let mut d = SocRegs::default();
        assert_eq!(d.read(0x20, 4).unwrap(), 0);
        assert_eq!(d.unknown_reads, vec![0x20]);
    }
}
