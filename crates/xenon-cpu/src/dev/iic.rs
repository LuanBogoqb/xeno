//! Experimental Xenon IIC: six 0x1000-byte processor blocks, big-endian
//! qword registers. Numeric register offsets/packet fields cross-checked with
//! xenon-emu's XenonIIC and the guest HV's startup sequence (0xC44).
//! No timer or device interrupt sources are synthesized here.
#[derive(Default)]
pub struct Iic {
    regs: [[u64; 32]; 6],
    pending: [u128; 6],
    service: [Vec<u8>; 6],
    global: std::collections::HashMap<u64, u64>,
}

impl Iic {
    pub fn pending(&self, thread: usize) -> bool {
        self.eligible(thread).is_some()
    }

    fn eligible(&self, thread: usize) -> Option<u8> {
        let priority = ((self.regs[thread][1] & 127) as u8)
            .max(self.service[thread].last().copied().unwrap_or(0));
        (priority.saturating_add(1)..124).rev()
            .find(|v| self.pending[thread] & (1u128 << v) != 0)
    }

    pub fn read(&mut self, off: u64, size: usize) -> u64 {
        let aligned = off & !7;
        let value = if aligned < 0x6000 {
            let t = (aligned >> 12) as usize;
            let reg = (aligned & 0xFFF) as usize / 8;
            match reg {
                10 | 11 => {
                    let v = self.eligible(t).unwrap_or((self.regs[t][14] & 127) as u8);
                    if self.eligible(t).is_some() {
                        self.pending[t] &= !(1u128 << v);
                        self.service[t].push(v);
                        if reg == 11 { self.regs[t][1] = v as u64; }
                    }
                    v as u64
                }
                _ => self.regs[t].get(reg).copied().unwrap_or(0),
            }
        } else { self.global.get(&aligned).copied().unwrap_or(0) };
        let shift = 8 * (8 - (off & 7) as usize - size);
        (value >> shift) & (u64::MAX >> (8 * (8 - size)))
    }

    pub fn write(&mut self, off: u64, size: usize, value: u64) {
        let aligned = off & !7;
        let shift = 8 * (8 - (off & 7) as usize - size);
        let mask = (u64::MAX >> (8 * (8 - size))) << shift;
        if aligned < 0x6000 {
            let t = (aligned >> 12) as usize;
            let reg = (aligned & 0xFFF) as usize / 8;
            if reg >= 32 { return; }
            let value = (self.regs[t][reg] & !mask) | (value << shift & mask);
            self.regs[t][reg] = value;
            match reg {
                2 => {
                    let vector = (value & 127) as u8;
                    let targets = value >> 16 & 63;
                    for dst in 0..6 {
                        if self.regs[dst][0] & targets != 0 && vector < 124 {
                            self.pending[dst] |= 1u128 << vector;
                        }
                    }
                }
                12 | 13 => {
                    self.service[t].pop();
                    if reg == 13 { self.regs[t][1] = value & 127; }
                }
                _ => {}
            }
        } else {
            let old = self.global.get(&aligned).copied().unwrap_or(0);
            self.global.insert(aligned, (old & !mask) | (value << shift & mask));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ipi_targets_logical_ids_and_acknowledges_once() {
        let mut i = Iic::default();
        i.write(0x2000, 8, 4);
        i.write(0x4000, 8, 16);
        i.write(0x2070, 8, 124);
        i.write(0x10, 8, 0x140078);
        assert!(i.pending(2) && i.pending(4));
        assert!(!i.pending(0) && !i.pending(1));
        assert_eq!(i.read(0x2050, 8), 120);
        assert!(!i.pending(2));
        assert_eq!(i.read(0x2050, 8), 124);
        assert!(i.pending(4));
        i.write(0x2068, 8, 100);
        i.write(0x10, 8, 0x40070);
        assert_eq!(i.read(0x2058, 8), 112);
        assert_eq!(i.read(0x2008, 8), 112);
    }
    #[test]
    fn big_endian_slices_and_priority_masking() {
        let mut i = Iic::default();
        i.write(0, 8, 1);
        i.write(8, 8, 120);
        i.write(0x10, 8, 0x10078);
        assert!(!i.pending(0));
        i.write(12, 4, 0);
        assert!(i.pending(0));
        assert_eq!(i.read(0x57, 1), 120);
        i.write(0x6070, 8, 0x1122334455667788);
        assert_eq!(i.read(0x6070, 4), 0x11223344);
        assert_eq!(i.read(0x6074, 4), 0x55667788);
    }
}
