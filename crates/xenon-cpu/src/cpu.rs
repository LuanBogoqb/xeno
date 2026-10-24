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
    /// Time base: advances by one per executed instruction.
    pub tb: u64,
    /// SPRs without dedicated state; reads of never-written ones give 0.
    pub spr: HashMap<u16, u64>,
}

/// True when the XO field of an op-31 word equals `xo` (disambiguates arms that share a body).
fn xo_is(w: u32, xo: u32) -> bool {
    w >> 1 & 0x3FF == xo
}

fn sext16(v: u32) -> u64 {
    v as u16 as i16 as i64 as u64
}

impl Cpu {
    pub fn new(entry: u64) -> Cpu {
        Cpu { gpr: [0; 32], pc: entry, lr: 0, ctr: 0, cr: 0, xer: 0, msr: 0, tb: 0, spr: HashMap::new() }
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
            7 => self.gpr[rt] = (self.gpr[ra] as i64).wrapping_mul(simm as i64) as u64,
            12 | 13 => {
                let (t, c) = self.gpr[ra].overflowing_add(simm);
                self.gpr[rt] = t;
                self.xer = self.xer & !XER_CA | if c { XER_CA } else { 0 };
                self.rc(w >> 26 & 1, t);
            }
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
                match w >> 1 & 0x3FF {
                    0 => {
                        // mcrf
                        let (bf, bfa) = (w >> 23 & 7, w >> 18 & 7);
                        self.set_cr_field(bf, self.cr >> ((7 - bfa) * 4) & 0xF);
                        self.pc = next;
                        return Ok(());
                    }
                    18 => {
                        // rfid
                        next = self.spr.get(&0x1A).copied().unwrap_or(0) & !3;
                        self.msr = self.spr.get(&0x1B).copied().unwrap_or(0);
                        self.pc = next;
                        return Ok(());
                    }
                    150 => {
                        // isync
                        self.pc = next;
                        return Ok(());
                    }
                    _ => {}
                }
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
            23 => {
                // rlwnm
                let (mb, me) = (w >> 6 & 31, w >> 1 & 31);
                let v = self.gpr[rt] as u32 as u64;
                self.gpr[ra] = rotl64(v << 32 | v, (self.gpr[w as usize >> 11 & 31] & 31) as u32) & mask64(mb + 32, me + 32);
                self.rc(w, self.gpr[ra]);
            }
            24 => self.gpr[ra] = self.gpr[rt] | uimm,
            25 => self.gpr[ra] = self.gpr[rt] | uimm << 16,
            26 => self.gpr[ra] = self.gpr[rt] ^ uimm,
            27 => self.gpr[ra] = self.gpr[rt] ^ uimm << 16,
            28 => {
                self.gpr[ra] = self.gpr[rt] & uimm;
                self.rc(1, self.gpr[ra]);
            }
            30 => {
                let sh = if w >> 2 & 7 == 4 { (self.gpr[w as usize >> 11 & 31] & 63) as u32 } else { (w >> 11 & 31) | (w >> 1 & 1) << 5 };
                let m = (w >> 6 & 31) | (w >> 5 & 1) << 5;
                let r = rotl64(self.gpr[rt], sh);
                self.gpr[ra] = match w >> 2 & 7 {
                    0 => r & mask64(m, 63),
                    1 => r & mask64(0, m),
                    4 if w >> 1 & 1 == 0 => r & mask64(m, 63), // rldcl
                    4 => r & mask64(0, m),                     // rldcr
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
            42 => self.gpr[rt] = bus.read(self.ea(ra, simm), 2)? as u16 as i16 as i64 as u64,
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
        self.tb += 1;
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
            21 => self.gpr[rt] = bus.read(ea_x, 8)?,
            149 => bus.write(ea_x, 8, s)?,
            534 => self.gpr[rt] = (bus.read(ea_x, 4)? as u32).swap_bytes() as u64,
            662 => bus.write(ea_x, 4, (s as u32).swap_bytes() as u64)?,
            790 => self.gpr[rt] = (bus.read(ea_x, 2)? as u16).swap_bytes() as u64,
            918 => bus.write(ea_x, 2, (s as u16).swap_bytes() as u64)?,
            60 => {
                self.gpr[ra] = s & !b;
                self.rc(w, self.gpr[ra]);
            }
            26 => {
                self.gpr[ra] = (s as u32).leading_zeros() as u64;
                self.rc(w, self.gpr[ra]);
            }
            24 | 536 => {
                let n = b & 0x3F;
                self.gpr[ra] = if n & 0x20 != 0 { 0 } else if xo_is(w, 24) { ((s as u32) << n) as u64 } else { ((s as u32) >> n) as u64 };
                self.rc(w, self.gpr[ra]);
            }
            27 | 539 => {
                let n = b & 0x7F;
                self.gpr[ra] = if n & 0x40 != 0 { 0 } else if xo_is(w, 27) { s << n } else { s >> n };
                self.rc(w, self.gpr[ra]);
            }
            235 => {
                self.gpr[rt] = (a as i32 as i64).wrapping_mul(b as i32 as i64) as u64;
                self.rc(w, self.gpr[rt]);
            }
            233 => {
                self.gpr[rt] = a.wrapping_mul(b);
                self.rc(w, self.gpr[rt]);
            }
            459 => {
                self.gpr[rt] = (a as u32).checked_div(b as u32).unwrap_or(0) as u64;
                self.rc(w, self.gpr[rt]);
            }
            457 => {
                self.gpr[rt] = a.checked_div(b).unwrap_or(0);
                self.rc(w, self.gpr[rt]);
            }
            136 => {
                let ca = (self.xer & XER_CA != 0) as u64;
                let (t, c1) = (!a).overflowing_add(b);
                let (t, c2) = t.overflowing_add(ca);
                self.gpr[rt] = t;
                self.xer = self.xer & !XER_CA | if c1 || c2 { XER_CA } else { 0 };
                self.rc(w, t);
            }
            19 => {
                // mfcr / mfocrf
                self.gpr[rt] = if w >> 20 & 1 == 1 {
                    let fxm = w >> 12 & 0xFF;
                    let mask = (0..8).filter(|i| fxm >> (7 - i) & 1 == 1).fold(0u32, |m, i| m | 0xF << (28 - 4 * i));
                    (self.cr & mask) as u64
                } else {
                    self.cr as u64
                };
            }
            83 => self.gpr[rt] = self.msr,
            371 => {
                let tbr = (w >> 16 & 31) | (w >> 11 & 31) << 5;
                self.gpr[rt] = if tbr == 269 { self.tb >> 32 } else { self.tb };
            }
            // barriers, cache management and TLB/SLB invalidates: no cache or MMU is modelled
            598 | 854 | 54 | 86 | 982 | 470 | 278 | 246 | 274 | 306 | 498 => {}
            1014 => {
                // dcbz zeroes a 32-byte line, dcbzl (RT bit 0) a 128-byte one
                let line = if rt & 1 == 1 { 128 } else { 32 };
                let base = ea_x & !(line - 1);
                for i in 0..line {
                    bus.write(base + i, 1, 0)?;
                }
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

    fn xl(xo: u32, bt: u32, ba: u32, bb: u32) -> u32 {
        19 << 26 | bt << 21 | ba << 16 | bb << 11 | xo << 1
    }

    fn run1(w: u32, init: impl FnOnce(&mut Cpu, &mut Bus)) -> (Cpu, Bus) {
        let (mut c, mut b) = setup(&[w]);
        init(&mut c, &mut b);
        steps(&mut c, &mut b, 1);
        (c, b)
    }

    #[test]
    fn op_mulli() {
        let (c, _) = run1(d(7, 3, 4, (-3i16) as u32), |c, _| c.gpr[4] = 7);
        assert_eq!(c.gpr[3] as i64, -21);
    }

    #[test]
    fn op_addic_sets_carry() {
        let (c, _) = run1(d(12, 3, 4, 1), |c, _| c.gpr[4] = u64::MAX);
        assert_eq!(c.gpr[3], 0);
        assert_ne!(c.xer & XER_CA, 0);
        let (c, _) = run1(d(13, 3, 4, 0), |c, _| c.gpr[4] = 5);
        assert_eq!(c.xer & XER_CA, 0);
        assert_eq!(c.cr >> 28, 0b0100);
    }

    #[test]
    fn op_xori_xoris() {
        let (c, _) = run1(d(26, 3, 4, 0xFF00), |c, _| c.gpr[3] = 0x0FF0);
        assert_eq!(c.gpr[4], 0xF0F0);
        let (c, _) = run1(d(27, 3, 4, 0x0001), |c, _| c.gpr[3] = 0x1_0000);
        assert_eq!(c.gpr[4], 0);
    }

    #[test]
    fn op_rlwnm_and_rotlw() {
        // rotlw r4,r3,r5 = rlwnm r4,r3,r5,0,31; the amount is the low 5 bits of rb
        let w = 23 << 26 | 3 << 21 | 4 << 16 | 5 << 11 | 31 << 1;
        let (c, _) = run1(w, |c, _| {
            c.gpr[3] = 0xAAAA_AAAA_8000_0001;
            c.gpr[5] = 0x21;
        });
        assert_eq!(c.gpr[4], 0x0000_0003);
    }

    #[test]
    fn op_rldcr() {
        // rldcr r4,r3,r5,me=7: rotl by rb&63, keep bits 0..=7 (IBM numbering, MSB first)
        let w = 30 << 26 | 3 << 21 | 4 << 16 | 5 << 11 | 7 << 6 | 9 << 1;
        let (c, _) = run1(w, |c, _| {
            c.gpr[3] = 0x0123_4567_89AB_CDEF;
            c.gpr[5] = 8;
        });
        assert_eq!(c.gpr[4], 0x2300_0000_0000_0000);
    }

    #[test]
    fn op_shifts() {
        let (c, _) = run1(x(31, 3, 4, 5, 24, 0), |c, _| {
            c.gpr[3] = 0xFFFF_FFFF_0000_0001;
            c.gpr[5] = 4;
        });
        assert_eq!(c.gpr[4], 0x10);
        let (c, _) = run1(x(31, 3, 4, 5, 24, 0), |c, _| {
            c.gpr[3] = 1;
            c.gpr[5] = 32;
        });
        assert_eq!(c.gpr[4], 0);
        let (c, _) = run1(x(31, 3, 4, 5, 536, 0), |c, _| {
            c.gpr[3] = 0xFFFF_FFFF_8000_0000;
            c.gpr[5] = 31;
        });
        assert_eq!(c.gpr[4], 1);
        let (c, _) = run1(x(31, 3, 4, 5, 27, 0), |c, _| {
            c.gpr[3] = 1;
            c.gpr[5] = 40;
        });
        assert_eq!(c.gpr[4], 1 << 40);
        let (c, _) = run1(x(31, 3, 4, 5, 539, 0), |c, _| {
            c.gpr[3] = 1 << 40;
            c.gpr[5] = 64;
        });
        assert_eq!(c.gpr[4], 0);
    }

    #[test]
    fn op_andc_cntlzw() {
        let (c, _) = run1(x(31, 3, 4, 5, 60, 0), |c, _| {
            c.gpr[3] = 0xFF;
            c.gpr[5] = 0x0F;
        });
        assert_eq!(c.gpr[4], 0xF0);
        let (c, _) = run1(x(31, 3, 4, 0, 26, 0), |c, _| c.gpr[3] = 0xFFFF_FFFF_0001_0000);
        assert_eq!(c.gpr[4], 15);
    }

    #[test]
    fn op_mul_div() {
        let (c, _) = run1(x(31, 3, 4, 5, 235, 0), |c, _| {
            c.gpr[4] = 0xFFFF_FFFF_FFFF_FFFF; // -1 as 32-bit
            c.gpr[5] = 3;
        });
        assert_eq!(c.gpr[3] as i64, -3);
        let (c, _) = run1(x(31, 3, 4, 5, 233, 0), |c, _| {
            c.gpr[4] = 1 << 33;
            c.gpr[5] = 3;
        });
        assert_eq!(c.gpr[3], 3 << 33);
        let (c, _) = run1(x(31, 3, 4, 5, 459, 0), |c, _| {
            c.gpr[4] = 0xDEAD_BEEF_0000_0064;
            c.gpr[5] = 7;
        });
        assert_eq!(c.gpr[3], 14);
        let (c, _) = run1(x(31, 3, 4, 5, 457, 0), |c, _| {
            c.gpr[4] = 1 << 40;
            c.gpr[5] = 1 << 8;
        });
        assert_eq!(c.gpr[3], 1 << 32);
    }

    #[test]
    fn op_subfe_uses_carry() {
        let (c, _) = run1(x(31, 3, 4, 5, 136, 0), |c, _| {
            c.gpr[4] = 1;
            c.gpr[5] = 10;
            c.xer |= XER_CA;
        });
        assert_eq!(c.gpr[3], 9); // ~1 + 10 + 1
        assert_ne!(c.xer & XER_CA, 0);
        let (c, _) = run1(x(31, 3, 4, 5, 136, 0), |c, _| {
            c.gpr[4] = 1;
            c.gpr[5] = 10;
        });
        assert_eq!(c.gpr[3], 8);
    }

    #[test]
    fn op_lha_sign_extends() {
        let (c, _) = run1(d(42, 3, 4, 0), |c, b| {
            c.gpr[4] = 0x2000;
            b.write(0x2000, 2, 0x8001).unwrap();
        });
        assert_eq!(c.gpr[3], 0xFFFF_FFFF_FFFF_8001);
    }

    #[test]
    fn op_indexed_dword_and_byte_reverse() {
        let (c, _) = run1(x(31, 3, 4, 5, 21, 0), |c, b| {
            c.gpr[4] = 0x2000;
            c.gpr[5] = 8;
            b.write(0x2008, 8, 0x1122_3344_5566_7788).unwrap();
        });
        assert_eq!(c.gpr[3], 0x1122_3344_5566_7788);
        let (_, mut b) = run1(x(31, 3, 4, 5, 149, 0), |c, _| {
            c.gpr[3] = 0xAABB_CCDD_EEFF_0011;
            c.gpr[4] = 0x2000;
            c.gpr[5] = 8;
        });
        assert_eq!(b.read(0x2008, 8).unwrap(), 0xAABB_CCDD_EEFF_0011);
        let (c, _) = run1(x(31, 3, 4, 5, 534, 0), |c, b| {
            c.gpr[4] = 0x2000;
            b.write(0x2000, 4, 0x1122_3344).unwrap();
        });
        assert_eq!(c.gpr[3], 0x4433_2211);
        let (_, mut b) = run1(x(31, 3, 4, 5, 662, 0), |c, _| {
            c.gpr[3] = 0x1122_3344;
            c.gpr[4] = 0x2000;
        });
        assert_eq!(b.read(0x2000, 4).unwrap(), 0x4433_2211);
        let (c, _) = run1(x(31, 3, 4, 5, 790, 0), |c, b| {
            c.gpr[4] = 0x2000;
            b.write(0x2000, 2, 0x1122).unwrap();
        });
        assert_eq!(c.gpr[3], 0x2211);
        let (_, mut b) = run1(x(31, 3, 4, 5, 918, 0), |c, _| {
            c.gpr[3] = 0xAA1122;
            c.gpr[4] = 0x2000;
        });
        assert_eq!(b.read(0x2000, 2).unwrap(), 0x2211);
    }

    #[test]
    fn op_mcrf_mfocrf_mftb_mfmsr() {
        // mcrf cr1,cr0
        let (c, _) = run1(xl(0, 1 << 2, 0 << 2, 0), |c, _| c.cr = 0xA000_0000);
        assert_eq!(c.cr, 0xAA00_0000);
        // mfocrf r3, fxm=0x80 (cr0)
        let (c, _) = run1(x(31, 3, 0, 0, 19, 0) | 1 << 20 | 0x80 << 12, |c, _| c.cr = 0x1234_5678);
        assert_eq!(c.gpr[3], 0x1000_0000);
        // mftb r3 / mftb r4 (TBL = 268): the time base advances every step
        let tbr = (268 & 31) << 16 | (268 >> 5) << 11;
        let (mut c, mut b) = setup(&[31 << 26 | 3 << 21 | tbr | 371 << 1, 31 << 26 | 4 << 21 | tbr | 371 << 1]);
        steps(&mut c, &mut b, 2);
        assert!(c.gpr[4] > c.gpr[3]);
        let (c, _) = run1(x(31, 3, 0, 0, 83, 0), |c, _| c.msr = MSR_HV | 2);
        assert_eq!(c.gpr[3], MSR_HV | 2);
    }

    #[test]
    fn op_barriers_are_noops_and_dcbz_zeroes() {
        for xo in [598u32, 854, 54, 86, 982, 470, 278, 246, 274, 306, 498] {
            let (c, _) = run1(x(31, 0, 4, 5, xo, 0), |c, _| c.gpr[4] = 0x2000);
            assert_eq!(c.pc, BASE + 4, "xo {xo}");
        }
        let (c, _) = run1(xl(150, 0, 0, 0), |_, _| {});
        assert_eq!(c.pc, BASE + 4);
        // dcbz zeroes the aligned 32 bytes, dcbzl the aligned 128
        let fill = |c: &mut Cpu, b: &mut Bus| {
            c.gpr[5] = 0x2013;
            for i in 0..0x100u64 {
                b.write(0x2000 + i, 1, 0xFF).unwrap();
            }
        };
        let (_, mut b) = run1(x(31, 0, 0, 5, 1014, 0), fill);
        assert_eq!(b.read(0x2000, 8).unwrap(), 0);
        assert_eq!(b.read(0x2018, 8).unwrap(), 0);
        assert_eq!(b.read(0x2020, 1).unwrap(), 0xFF);
        let (_, mut b) = run1(x(31, 1, 0, 5, 1014, 0), fill);
        assert_eq!(b.read(0x2000, 1).unwrap(), 0);
        assert_eq!(b.read(0x207F, 1).unwrap(), 0);
        assert_eq!(b.read(0x2080, 1).unwrap(), 0xFF);
    }

    #[test]
    fn op_rfid_loads_pc_and_msr_from_srr() {
        let (c, _) = run1(xl(18, 0, 0, 0), |c, _| {
            c.spr.insert(0x1A, 0x4003);
            c.spr.insert(0x1B, MSR_HV | 0x30);
        });
        assert_eq!(c.pc, 0x4000);
        assert_eq!(c.msr, MSR_HV | 0x30);
    }
}
