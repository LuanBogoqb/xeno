use crate::alu::{mask64, rotl64};
use crate::mem::{Bus, BusError};
use std::collections::HashMap;

/// Hypervisor state bit of the MSR (bit 3 in IBM numbering).
pub const MSR_HV: u64 = 1 << 60;
const XER_CA: u64 = 1 << 29;
const XER_SO: u64 = 1 << 31;

#[derive(Debug, PartialEq, Eq)]
pub enum Stop {
    Unmapped(BusError),
    UnknownOpcode { pc: u64, word: u32 },
    StepLimit,
    Trap { pc: u64 },
}

impl From<BusError> for Stop {
    fn from(e: BusError) -> Stop {
        Stop::Unmapped(e)
    }
}

pub struct Cpu {
    pub gpr: [u64; 32],
    pub pc: u64,
    pub lr: u64,
    pub ctr: u64,
    pub cr: u32,
    pub xer: u64,
    pub msr: u64,
    /// SPRs without dedicated state; reads of never-written ones give 0.
    pub spr: HashMap<u16, u64>,
}

fn sext16(v: u32) -> u64 {
    v as u16 as i16 as i64 as u64
}

impl Cpu {
    pub fn new(entry: u64) -> Cpu {
        Cpu { gpr: [0; 32], pc: entry, lr: 0, ctr: 0, cr: 0, xer: 0, msr: 0, spr: HashMap::new() }
    }

    pub fn run(&mut self, bus: &mut Bus, max_steps: u64) -> Stop {
        for _ in 0..max_steps {
            if let Err(s) = self.step(bus) {
                return s;
            }
        }
        Stop::StepLimit
    }

    fn set_cr_field(&mut self, n: u32, bits: u32) {
        let sh = (7 - n) * 4;
        self.cr = self.cr & !(0xF << sh) | bits << sh;
    }

    fn so(&self) -> u32 {
        (self.xer & XER_SO != 0) as u32
    }

    fn cmp_signed(&mut self, n: u32, a: i64, b: i64) {
        let f = if a < b { 8 } else if a > b { 4 } else { 2 };
        self.set_cr_field(n, f | self.so());
    }

    fn cmp_unsigned(&mut self, n: u32, a: u64, b: u64) {
        let f = if a < b { 8 } else if a > b { 4 } else { 2 };
        self.set_cr_field(n, f | self.so());
    }

    /// Rc=1 forms update CR0 from the full 64-bit result.
    fn rc(&mut self, w: u32, v: u64) {
        if w & 1 != 0 {
            self.cmp_signed(0, v as i64, 0);
        }
    }

    fn cr_bit(&self, bi: u32) -> bool {
        self.cr >> (31 - bi) & 1 != 0
    }

    /// Evaluates BO/BI of a conditional branch, decrementing CTR when BO asks for it.
    fn branch_taken(&mut self, bo: u32, bi: u32) -> bool {
        if bo & 4 == 0 {
            self.ctr = self.ctr.wrapping_sub(1);
        }
        let ctr_ok = bo & 4 != 0 || (self.ctr != 0) != (bo & 2 != 0);
        let cond_ok = bo & 16 != 0 || self.cr_bit(bi) == (bo & 8 != 0);
        ctr_ok && cond_ok
    }

    fn ea(&self, ra: usize, disp: u64) -> u64 {
        (if ra == 0 { 0 } else { self.gpr[ra] }).wrapping_add(disp)
    }

    pub fn step(&mut self, bus: &mut Bus) -> Result<(), Stop> {
        let pc = self.pc;
        let w = bus.read(pc, 4)? as u32;
        let mut next = pc.wrapping_add(4);
        let rt = (w >> 21 & 31) as usize;
        let ra = (w >> 16 & 31) as usize;
        let simm = sext16(w);
        let uimm = (w & 0xFFFF) as u64;
        let ds = (w & 0xFFFC) as u16 as i16 as i64 as u64;
        let bad = Stop::UnknownOpcode { pc, word: w };

        match w >> 26 {
            8 => {
                // subfic
                let (t, c1) = (!self.gpr[ra]).overflowing_add(simm);
                let (t, c2) = t.overflowing_add(1);
                self.gpr[rt] = t;
                self.xer = self.xer & !XER_CA | if c1 || c2 { XER_CA } else { 0 };
            }
            10 => {
                let a = if w >> 21 & 1 == 1 { self.gpr[ra] } else { self.gpr[ra] as u32 as u64 };
                self.cmp_unsigned(w >> 23 & 7, a, uimm);
            }
            11 => {
                let a = if w >> 21 & 1 == 1 { self.gpr[ra] as i64 } else { self.gpr[ra] as u32 as i32 as i64 };
                self.cmp_signed(w >> 23 & 7, a, simm as i64);
            }
            14 => self.gpr[rt] = self.ea(ra, simm),
            15 => self.gpr[rt] = self.ea(ra, (simm << 16) as i32 as i64 as u64),
            16 => {
                let taken = self.branch_taken(w >> 21 & 31, w >> 16 & 31);
                if w & 1 != 0 {
                    self.lr = next;
                }
                if taken {
                    next = if w & 2 != 0 { ds } else { pc.wrapping_add(ds) };
                }
            }
            17 => {
                // sc with LEV=2 is the hypervisor call
                if w & 2 != 0 && w >> 5 & 0x7F == 2 {
                    return Err(Stop::Trap { pc });
                }
                return Err(bad);
            }
            18 => {
                let disp = ((w & 0x03FF_FFFC) as i32) << 6 >> 6;
                let disp = disp as i64 as u64;
                if w & 1 != 0 {
                    self.lr = next;
                }
                next = if w & 2 != 0 { disp } else { pc.wrapping_add(disp) };
            }
            19 => {
                let target = match w >> 1 & 0x3FF {
                    16 => self.lr,
                    528 => self.ctr,
                    _ => return Err(bad),
                };
                let taken = self.branch_taken(w >> 21 & 31, w >> 16 & 31);
                if w & 1 != 0 {
                    self.lr = next;
                }
                if taken {
                    next = target & !3;
                }
            }
            20 => {
                // rlwimi
                let (sh, mb, me) = (w >> 11 & 31, w >> 6 & 31, w >> 1 & 31);
                let v = self.gpr[rt] as u32 as u64;
                let m = mask64(mb + 32, me + 32);
                self.gpr[ra] = rotl64(v << 32 | v, sh) & m | self.gpr[ra] & !m;
                self.rc(w, self.gpr[ra]);
            }
            21 => {
                // rlwinm: the 32-bit rotate is replicated into both halves, as the 64-bit ISA defines it
                let (sh, mb, me) = (w >> 11 & 31, w >> 6 & 31, w >> 1 & 31);
                let v = self.gpr[rt] as u32 as u64;
                self.gpr[ra] = rotl64(v << 32 | v, sh) & mask64(mb + 32, me + 32);
                self.rc(w, self.gpr[ra]);
            }
            24 => self.gpr[ra] = self.gpr[rt] | uimm,
            25 => self.gpr[ra] = self.gpr[rt] | uimm << 16,
            28 => {
                self.gpr[ra] = self.gpr[rt] & uimm;
                self.rc(1, self.gpr[ra]);
            }
            30 => {
                let sh = (w >> 11 & 31) | (w >> 1 & 1) << 5;
                let m = (w >> 6 & 31) | (w >> 5 & 1) << 5;
                let r = rotl64(self.gpr[rt], sh);
                self.gpr[ra] = match w >> 2 & 7 {
                    0 => r & mask64(m, 63),
                    1 => r & mask64(0, m),
                    3 => {
                        let mk = mask64(m, 63 - sh);
                        r & mk | self.gpr[ra] & !mk
                    }
                    _ => return Err(bad),
                };
                self.rc(w, self.gpr[ra]);
            }
            31 => self.op31(bus, w, pc)?,
            32 => self.gpr[rt] = bus.read(self.ea(ra, simm), 4)?,
            33 => {
                let ea = self.ea(ra, simm);
                self.gpr[rt] = bus.read(ea, 4)?;
                self.gpr[ra] = ea;
            }
            34 => self.gpr[rt] = bus.read(self.ea(ra, simm), 1)?,
            35 => {
                let ea = self.ea(ra, simm);
                self.gpr[rt] = bus.read(ea, 1)?;
                self.gpr[ra] = ea;
            }
            36 => bus.write(self.ea(ra, simm), 4, self.gpr[rt] & 0xFFFF_FFFF)?,
            37 => {
                let ea = self.ea(ra, simm);
                bus.write(ea, 4, self.gpr[rt] & 0xFFFF_FFFF)?;
                self.gpr[ra] = ea;
            }
            38 => bus.write(self.ea(ra, simm), 1, self.gpr[rt] & 0xFF)?,
            39 => {
                let ea = self.ea(ra, simm);
                bus.write(ea, 1, self.gpr[rt] & 0xFF)?;
                self.gpr[ra] = ea;
            }
            40 => self.gpr[rt] = bus.read(self.ea(ra, simm), 2)?,
            44 => bus.write(self.ea(ra, simm), 2, self.gpr[rt] & 0xFFFF)?,
            58 => {
                let ea = self.ea(ra, ds);
                match w & 3 {
                    0 => self.gpr[rt] = bus.read(ea, 8)?,
                    1 => {
                        self.gpr[rt] = bus.read(ea, 8)?;
                        self.gpr[ra] = ea;
                    }
                    _ => return Err(bad),
                }
            }
            62 => {
                let ea = self.ea(ra, ds);
                match w & 3 {
                    0 => bus.write(ea, 8, self.gpr[rt])?,
                    1 => {
                        bus.write(ea, 8, self.gpr[rt])?;
                        self.gpr[ra] = ea;
                    }
                    _ => return Err(bad),
                }
            }
            _ => return Err(bad),
        }
        self.pc = next;
        Ok(())
    }

    /// Primary opcode 31 (none of these redirect flow).
    fn op31(&mut self, bus: &mut Bus, w: u32, pc: u64) -> Result<(), Stop> {
        let rt = (w >> 21 & 31) as usize;
        let ra = (w >> 16 & 31) as usize;
        let rb = (w >> 11 & 31) as usize;
        let bad = Stop::UnknownOpcode { pc, word: w };
        let (s, a, b) = (self.gpr[rt], self.gpr[ra], self.gpr[rb]);
        let ea_x = self.ea(ra, b);
        match w >> 1 & 0x3FF {
            0 => {
                let (x, y) = if w >> 21 & 1 == 1 { (a as i64, b as i64) } else { (a as u32 as i32 as i64, b as u32 as i32 as i64) };
                self.cmp_signed(w >> 23 & 7, x, y);
            }
            32 => {
                let (x, y) = if w >> 21 & 1 == 1 { (a, b) } else { (a as u32 as u64, b as u32 as u64) };
                self.cmp_unsigned(w >> 23 & 7, x, y);
            }
            23 => self.gpr[rt] = bus.read(ea_x, 4)?,
            87 => self.gpr[rt] = bus.read(ea_x, 1)?,
            151 => bus.write(ea_x, 4, s & 0xFFFF_FFFF)?,
            215 => bus.write(ea_x, 1, s & 0xFF)?,
            28 => {
                self.gpr[ra] = s & b;
                self.rc(w, self.gpr[ra]);
            }
            40 => {
                self.gpr[rt] = b.wrapping_sub(a);
                self.rc(w, self.gpr[rt]);
            }
            124 => {
                self.gpr[ra] = !(s | b);
                self.rc(w, self.gpr[ra]);
            }
            266 => {
                self.gpr[rt] = a.wrapping_add(b);
                self.rc(w, self.gpr[rt]);
            }
            316 => {
                self.gpr[ra] = s ^ b;
                self.rc(w, self.gpr[ra]);
            }
            444 => {
                self.gpr[ra] = s | b;
                self.rc(w, self.gpr[ra]);
            }
            986 => {
                self.gpr[ra] = s as u32 as i32 as i64 as u64;
                self.rc(w, self.gpr[ra]);
            }
            xo @ (339 | 467) => {
                let spr = (w >> 16 & 31) | (w >> 11 & 31) << 5;
                let reg = match spr {
                    1 => &mut self.xer,
                    8 => &mut self.lr,
                    9 => &mut self.ctr,
                    n => self.spr.entry(n as u16).or_insert(0),
                };
                if xo == 339 {
                    self.gpr[rt] = *reg;
                } else {
                    *reg = s;
                }
            }
            _ => return Err(bad),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::{Bus, BusError};

    const BASE: u64 = 0x1000;

    fn d(op: u32, rt: u32, ra: u32, imm: u32) -> u32 {
        op << 26 | rt << 21 | ra << 16 | (imm & 0xFFFF)
    }
    fn x(op: u32, rt: u32, ra: u32, rb: u32, xo: u32, rc: u32) -> u32 {
        op << 26 | rt << 21 | ra << 16 | rb << 11 | xo << 1 | rc
    }

    fn setup(words: &[u32]) -> (Cpu, Bus) {
        let mut bus = Bus::new();
        bus.map_ram(0, 0x10000);
        for (i, w) in words.iter().enumerate() {
            bus.write(BASE + 4 * i as u64, 4, *w as u64).unwrap();
        }
        (Cpu::new(BASE), bus)
    }

    fn steps(cpu: &mut Cpu, bus: &mut Bus, n: usize) {
        for _ in 0..n {
            cpu.step(bus).unwrap();
        }
    }

    #[test]
    fn lis_ori_build_constant() {
        let (mut c, mut b) = setup(&[d(15, 3, 0, 0x8000), d(24, 3, 3, 0x1234)]);
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[3], 0xFFFF_FFFF_8000_0000);
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[3], 0xFFFF_FFFF_8000_1234);
    }

    #[test]
    fn rlwinm_ignores_high_bits() {
        // rlwinm r4,r3,0,24,31
        let w = 21 << 26 | 3 << 21 | 4 << 16 | 24 << 6 | 31 << 1;
        let (mut c, mut b) = setup(&[w]);
        c.gpr[3] = 0xDEAD_BEEF_0000_00F0;
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[4], 0xF0);
    }

    #[test]
    fn extsw_sign_extends() {
        let (mut c, mut b) = setup(&[x(31, 3, 4, 0, 986, 0)]);
        c.gpr[3] = 0x0000_0000_8000_0000;
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[4], 0xFFFF_FFFF_8000_0000);
    }

    #[test]
    fn cmpw_uses_low_32_bits() {
        // cmpw cr0,r3,r4
        let (mut c, mut b) = setup(&[x(31, 0, 3, 4, 0, 0)]);
        c.gpr[3] = 0x1111_1111_0000_0005;
        c.gpr[4] = 0x2222_2222_0000_0005;
        steps(&mut c, &mut b, 1);
        assert_eq!(c.cr >> 28, 0b0010);
    }

    #[test]
    fn bl_sets_lr_and_blr_returns() {
        // bl +8 ; (skipped) ; blr
        let (mut c, mut b) = setup(&[18 << 26 | 8 | 1, 0, 19 << 26 | 20 << 21 | 16 << 1]);
        steps(&mut c, &mut b, 1);
        assert_eq!(c.pc, BASE + 8);
        assert_eq!(c.lr, BASE + 4);
        steps(&mut c, &mut b, 1);
        assert_eq!(c.pc, BASE + 4);
    }

    #[test]
    fn bdnz_loop_ends_with_ctr_zero() {
        // bdnz . (self-branch while ctr != 0)
        let (mut c, mut b) = setup(&[16 << 26 | 16 << 21 | 0, 0]);
        c.ctr = 5;
        steps(&mut c, &mut b, 5);
        assert_eq!(c.ctr, 0);
        assert_eq!(c.pc, BASE + 4);
    }

    #[test]
    fn lwz_stw_roundtrip_big_endian() {
        let (mut c, mut b) = setup(&[d(36, 3, 5, 0), d(32, 4, 5, 0)]);
        c.gpr[3] = 0xAABB_CCDD;
        c.gpr[5] = 0x2000;
        steps(&mut c, &mut b, 2);
        assert_eq!(b.read(0x2000, 1).unwrap(), 0xAA);
        assert_eq!(c.gpr[4], 0xAABB_CCDD);
    }

    #[test]
    fn ld_std_roundtrip() {
        let (mut c, mut b) = setup(&[d(62, 3, 5, 8), d(58, 4, 5, 8)]);
        c.gpr[3] = 0x0102_0304_0506_0708;
        c.gpr[5] = 0x2000;
        steps(&mut c, &mut b, 2);
        assert_eq!(b.read(0x2008, 1).unwrap(), 0x01);
        assert_eq!(c.gpr[4], 0x0102_0304_0506_0708);
    }

    #[test]
    fn unknown_opcode_stops() {
        let (mut c, mut b) = setup(&[0]);
        assert_eq!(c.run(&mut b, 10), Stop::UnknownOpcode { pc: 0x1000, word: 0 });
    }

    #[test]
    fn step_limit_stops() {
        let (mut c, mut b) = setup(&[18 << 26]); // b .
        assert_eq!(c.run(&mut b, 100), Stop::StepLimit);
    }

    #[test]
    fn unmapped_load_stops() {
        let (mut c, mut b) = setup(&[d(32, 4, 5, 0)]);
        c.gpr[5] = 0x9000_0000;
        assert_eq!(
            c.run(&mut b, 10),
            Stop::Unmapped(BusError::Unmapped { addr: 0x9000_0000, size: 4, write: false })
        );
    }

    #[test]
    fn mtspr_mfspr_roundtrip_unknown_spr() {
        // mtspr 0x98,r3 ; mfspr r4,0x98  (spr field is split: low5 | high5<<5)
        let f = |xo: u32, rt: u32| 31 << 26 | rt << 21 | (0x98 & 31) << 16 | (0x98 >> 5) << 11 | xo << 1;
        let (mut c, mut b) = setup(&[f(467, 3), f(339, 4)]);
        c.gpr[3] = 0xCAFE;
        steps(&mut c, &mut b, 2);
        assert_eq!(c.gpr[4], 0xCAFE);
    }

    #[test]
    fn mflr_mtlr() {
        let (mut c, mut b) = setup(&[31 << 26 | 3 << 21 | 8 << 16 | 467 << 1, 31 << 26 | 4 << 21 | 8 << 16 | 339 << 1]);
        c.gpr[3] = 0x1234;
        steps(&mut c, &mut b, 2);
        assert_eq!((c.lr, c.gpr[4]), (0x1234, 0x1234));
    }

    #[test]
    fn msr_hv_bit_is_bit60() {
        assert_eq!(MSR_HV, 0x1000_0000_0000_0000);
    }

    #[test]
    fn sc_lev2_traps() {
        let (mut c, mut b) = setup(&[0x4400_0042]);
        assert_eq!(c.run(&mut b, 10), Stop::Trap { pc: BASE });
    }

    #[test]
    fn stbu_updates_base() {
        let (mut c, mut b) = setup(&[d(39, 7, 3, 1)]);
        c.gpr[3] = 0x2000;
        c.gpr[7] = 0x1AB;
        steps(&mut c, &mut b, 1);
        assert_eq!((c.gpr[3], b.read(0x2001, 1).unwrap()), (0x2001, 0xAB));
    }
}
