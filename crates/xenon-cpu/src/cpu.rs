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
    /// SF-dependent record semantics enabled with the experimental post-CD MMU.
    word_mode: bool,
    /// Time base: advances by one per executed instruction.
    pub tb: u64,
    pub(crate) dec_pending: bool,
    pub(crate) dec_armed: bool,
    /// HRMOR written by mtspr 0x139; takes effect at the next context-synchronizing instruction (isync, rfid),
    /// or else after the instruction that follows the write (CB_B changes it right before a `blr`).
    pending_hrmor: Option<u64>,
    hrmor_armed: bool,
    /// Per-thread SLB: (RB, RS) as written by slbmte, indexed by RB[58..63].
    pub slb: [(u64, u64); 64],
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
        Cpu { gpr: [0; 32], pc: entry, lr: 0, ctr: 0, cr: 0, xer: 0, msr: 0, word_mode: false, tb: 0, dec_pending: false, dec_armed: false, pending_hrmor: None, hrmor_armed: false, slb: [(0, 0); 64], spr: HashMap::new() }
    }

    pub fn run(&mut self, bus: &mut Bus, max_steps: u64) -> Stop {
        for _ in 0..max_steps {
            if let Err(s) = self.step(bus) {
                return s;
            }
        }
        Stop::StepLimit
    }

    fn sync_context(&mut self, bus: &mut Bus) {
        if let Some(h) = self.pending_hrmor.take() {
            bus.hrmor = h;
        }
    }

    /// Run after every instruction: a pending HRMOR is applied once the instruction after the mtspr has executed.
    fn age_hrmor(&mut self, bus: &mut Bus) {
        if self.pending_hrmor.is_some() {
            if self.hrmor_armed {
                self.sync_context(bus);
            } else {
                self.hrmor_armed = true;
            }
        }
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

    /// Rc=1 compares the signed word in SF=0, doubleword in SF=1. The legacy
    /// boot interpreter retains its pre-M2 behavior until the new MMU is on.
    fn rc(&mut self, w: u32, v: u64) {
        if w & 1 != 0 {
            let value = if self.word_mode { v as u32 as i32 as i64 } else { v as i64 };
            self.cmp_signed(0, value, 0);
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
        let result = self.step_inner(bus);
        let fetch = bus.fetching;
        bus.fetching = false;
        bus.translated = false;
        if let Err(Stop::Unmapped(BusError::Protection { addr, write, no_execute })) = &result {
            let pc = if self.msr >> 63 == 0 { self.pc as u32 as u64 } else { self.pc };
            self.spr.insert(0x1A, pc);
            let reason = if *no_execute { 0x1000_0000 } else { 0x0800_0000 };
            self.spr.insert(0x1B, self.msr | if fetch { reason } else { 0 });
            if !fetch {
                self.spr.insert(0x13, if self.msr >> 63 == 0 { *addr as u32 as u64 } else { *addr });
                self.spr.insert(0x12, reason | if *write { 0x0200_0000 } else { 0 });
            }
            self.msr = MSR_HV | (1 << 63) | (self.msr & 0x1000);
            self.pc = if fetch { 0x400 } else { 0x300 };
            return Ok(());
        }
        if let Err(Stop::Unmapped(BusError::Translation { addr, write, segment })) = &result {
            let vector = match (fetch, segment) {
                (true, true) => 0x480,
                (true, false) => 0x400,
                (false, true) => 0x380,
                (false, false) => 0x300,
            };
            let saved_pc = if self.msr >> 63 == 0 { self.pc as u32 as u64 } else { self.pc };
            self.spr.insert(0x1A, saved_pc);
            self.spr.insert(0x1B, self.msr | if fetch && !segment { 0x4000_0000 } else { 0 });
            if !fetch {
                self.spr.insert(0x13, if self.msr >> 63 == 0 { *addr as u32 as u64 } else { *addr }); // DAR
                self.spr.insert(0x12, 0x4000_0000 | if *write { 0x0200_0000 } else { 0 });
            }
            // Replacement way hint consumed by the HV's software reload path.
            // Single-CPU deterministic replacement; class is computed by HV.
            self.spr.insert(0x3B2, 8);
            self.msr = MSR_HV | (1 << 63) | (self.msr & 0x1000);
            self.pc = vector;
            return Ok(());
        }
        result
    }

    /// Advance a programmed decrementer in deterministic emulated time.
    pub(crate) fn tick_decrementer(&mut self, ticks: u64) {
        if !self.dec_armed { return; }
        if let Some(dec) = self.spr.get_mut(&0x16) {
            let old = *dec as u32;
            // Request on the unsigned 0 -> FFFFFFFF transition, independently
            // of MSR[EE]. Delivery waits until interrupts are enabled.
            if ticks > old as u64 { self.dec_pending = true; }
            *dec = old.wrapping_sub(ticks as u32) as u64;
        }
    }

    /// Architectural hardware interrupt entry.
    pub(crate) fn interrupt(&mut self, vector: u64) {
        self.spr.insert(0x1A, self.pc);
        self.spr.insert(0x1B, self.msr);
        self.msr = MSR_HV | (1 << 63) | (self.msr & 0x1000);
        self.pc = vector;
    }

    fn step_inner(&mut self, bus: &mut Bus) -> Result<(), Stop> {
        let pc = self.pc;
        self.word_mode = bus.mmu.enabled && self.msr >> 63 == 0;
        bus.mmu.sf = self.msr >> 63 != 0;
        bus.mmu.problem = self.msr & 0x4000 != 0;
        bus.fetching = true;
        bus.translated = self.msr & 0x20 != 0;
        let w = bus.fetch(pc)? as u32;
        bus.fetching = false;
        bus.translated = self.msr & 0x10 != 0;
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
                let c = if self.word_mode { (self.gpr[ra] as u32).overflowing_add(simm as u32).1 } else { c };
                self.gpr[rt] = t;
                self.xer = self.xer & !XER_CA | if c { XER_CA } else { 0 };
                self.rc(w >> 26 & 1, t);
            }
            8 => {
                // subfic
                let (t, c1) = (!self.gpr[ra]).overflowing_add(simm);
                let (t, c2) = t.overflowing_add(1);
                let carry = if self.word_mode { (!self.gpr[ra] as u32 as u64) + (simm as u32 as u64) + 1 > u32::MAX as u64 } else { c1 || c2 };
                self.gpr[rt] = t;
                self.xer = self.xer & !XER_CA | if carry { XER_CA } else { 0 };
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
                if bus.mmu.enabled && w & 2 != 0 && matches!(w >> 5 & 0x7F, 0 | 2) {
                    self.spr.insert(0x1A, if self.msr >> 63 == 0 { next as u32 as u64 } else { next });
                    self.spr.insert(0x1B, self.msr);
                    self.msr = MSR_HV | (1 << 63) | (self.msr & 0x1000);
                    next = 0xC00;
                } else {
                // sc with LEV=2 is the hypervisor call
                if w & 2 != 0 && w >> 5 & 0x7F == 2 {
                    return Err(Stop::Trap { pc });
                }
                return Err(bad);
                }
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
                    xo @ (33 | 129 | 193 | 225 | 257 | 289 | 417 | 449) => {
                        let a = self.cr_bit(w >> 16 & 31);
                        let b = self.cr_bit(w >> 11 & 31);
                        let value = match xo {
                            33 => !(a | b),   // crnor
                            129 => a & !b,    // crandc
                            193 => a ^ b,     // crxor
                            225 => !(a & b),  // crnand
                            257 => a & b,     // crand
                            289 => !(a ^ b),  // creqv
                            417 => a | !b,    // crorc
                            449 => a | b,     // cror
                            _ => unreachable!(),
                        };
                        let mask = 1 << (31 - rt);
                        self.cr = (self.cr & !mask) | if value { mask } else { 0 };
                        self.pc = next;
                        self.tb += 1;
                        self.age_hrmor(bus);
                        return Ok(());
                    }
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
                        if bus.mmu.enabled { self.msr &= !0x7800_0000; } // SRR1 storage-fault flags, not MSR bits
                        if bus.mmu.enabled && self.msr >> 63 == 0 {
                            next = next as u32 as u64;
                        }
                        self.sync_context(bus);
                        self.pc = next;
                        return Ok(());
                    }
                    150 => {
                        // isync
                        self.sync_context(bus);
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
            28 | 29 => { // andi. / andis.
                self.gpr[ra] = self.gpr[rt] & (uimm << if w >> 26 == 29 { 16 } else { 0 });
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
        self.pc = if bus.mmu.enabled && self.msr >> 63 == 0 { next as u32 as u64 } else { next };
        self.tb += 1;
        self.age_hrmor(bus);
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
            xo @ (53 | 55 | 119 | 311 | 375) if ra != 0 && ra != rt => {
                let size = match xo { 53 => 8, 55 => 4, 119 => 1, _ => 2 };
                let value = bus.read(ea_x, size)?;
                self.gpr[rt] = if xo == 375 { value as u16 as i16 as i64 as u64 } else { value };
                self.gpr[ra] = ea_x;
            }
            xo @ (181 | 183 | 247 | 439) if ra != 0 => {
                let size = match xo { 181 => 8, 183 => 4, 247 => 1, _ => 2 };
                bus.write(ea_x, size, s)?;
                self.gpr[ra] = ea_x;
            }
            20 | 84 => { // lwarx / ldarx
                let size = if xo_is(w, 20) { 4 } else { 8 };
                self.gpr[rt] = bus.load_reserve(ea_x, size)?;
            }
            150 | 214 if w & 1 != 0 => { // stwcx. / stdcx.
                let size = if xo_is(w, 150) { 4 } else { 8 };
                let stored = bus.store_conditional(ea_x, size, s)?;
                self.set_cr_field(0, if stored { 2 } else { 0 } | self.so());
            }
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
            954 => {
                self.gpr[ra] = s as u8 as i8 as i64 as u64;
                self.rc(w, self.gpr[ra]);
            }
            922 => {
                self.gpr[ra] = s as u16 as i16 as i64 as u64;
                self.rc(w, self.gpr[ra]);
            }
            21 => self.gpr[rt] = bus.read(ea_x, 8)?,
            279 => self.gpr[rt] = bus.read(ea_x, 2)?,
            343 => self.gpr[rt] = bus.read(ea_x, 2)? as u16 as i16 as i64 as u64,
            407 => bus.write(ea_x, 2, s & 0xFFFF)?,
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
            58 => { // cntlzd
                self.gpr[ra] = s.leading_zeros() as u64;
                self.rc(w, self.gpr[ra]);
            }
            24 | 536 => {
                let n = b & 0x3F;
                self.gpr[ra] = if n & 0x20 != 0 { 0 } else if xo_is(w, 24) { ((s as u32) << n) as u64 } else { ((s as u32) >> n) as u64 };
                self.rc(w, self.gpr[ra]);
            }
            824 => {
                let n = rb as u32;
                let value = s as u32 as i32;
                self.gpr[ra] = (value >> n) as i64 as u64;
                let carry = value < 0 && (s as u32 & ((1u32 << n) - 1)) != 0;
                self.xer = (self.xer & !XER_CA) | if carry { XER_CA } else { 0 };
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
            9 => {
                self.gpr[rt] = ((a as u128 * b as u128) >> 64) as u64;
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
                let carry = if self.word_mode { (!a as u32 as u64) + (b as u32 as u64) + ca > u32::MAX as u64 } else { c1 || c2 };
                self.gpr[rt] = t;
                self.xer = self.xer & !XER_CA | if carry { XER_CA } else { 0 };
                self.rc(w, t);
            }
            xo @ (10 | 138 | 202 | 200) => { // addc, adde, addze, subfze
                let carry = (self.xer & XER_CA != 0) as u64;
                let (left, right, cin) = match xo {
                    10 => (a, b, 0),
                    138 => (a, b, carry),
                    202 => (a, 0, carry),
                    200 => (!a, 0, carry),
                    _ => unreachable!(),
                };
                let (value, c1) = left.overflowing_add(right);
                let (value, c2) = value.overflowing_add(cin);
                let carry = if self.word_mode { (left as u32 as u64) + (right as u32 as u64) + cin > u32::MAX as u64 } else { c1 || c2 };
                self.gpr[rt] = value;
                self.xer = (self.xer & !XER_CA) | if carry { XER_CA } else { 0 };
                self.rc(w, value);
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
            144 => {
                // mtcrf / mtocrf: FXM selects CR fields, not individual bits.
                let fxm = w >> 12 & 0xFF;
                let mask = (0..8).filter(|i| fxm >> (7 - i) & 1 == 1)
                    .fold(0u32, |m, i| m | 0xF << (28 - 4 * i));
                self.cr = (self.cr & !mask) | (s as u32 & mask);
            }
            8 => {
                let (value, borrow) = b.overflowing_sub(a);
                let borrow = if self.word_mode { (b as u32).overflowing_sub(a as u32).1 } else { borrow };
                self.gpr[rt] = value;
                self.xer = (self.xer & !XER_CA) | if !borrow { XER_CA } else { 0 };
                self.rc(w, value);
            }
            83 => self.gpr[rt] = self.msr,
            104 => {
                self.gpr[rt] = a.wrapping_neg();
                self.rc(w, self.gpr[rt]);
            }
            146 => self.msr = s,
            178 => {
                // mtmsrd: L=1 only touches EE (bit 48) and RI (bit 62)
                let mask = if w >> 16 & 1 == 1 { 0x8002 } else { u64::MAX };
                self.msr = self.msr & !mask | s & mask;
            }
            402 => {
                self.slb[(b & 63) as usize] = (b, s);
                bus.mmu.slb = self.slb;
            }
            434 | 498 => {
                if bus.mmu.enabled {
                    for (rb, _) in &mut self.slb {
                        if xo_is(w, 498) || *rb >> 28 == b >> 28 {
                            *rb &= !0x0800_0000;
                        }
                    }
                    bus.mmu.slb = self.slb;
                }
            }
            851 => self.gpr[rt] = self.slb[(b & 63) as usize].1, // slbmfev
            915 => self.gpr[rt] = self.slb[(b & 63) as usize].0, // slbmfee
            371 => {
                let tbr = (w >> 16 & 31) | (w >> 11 & 31) << 5;
                self.gpr[rt] = if tbr == 269 { self.tb >> 32 } else { self.tb };
            }
            // tlbie/tlbiel and slbie invalidate the experimental software TLB/SLB.
            274 | 306 => { bus.mmu.invalidate_addr(b); }
            // barriers, cache management and remaining invalidates: no cache modelled
            598 | 854 | 54 | 86 | 982 | 470 | 278 | 246 => {}
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
                    match spr {
                        0x13E => self.gpr[rt] = bus.lpcr,
                        0x13F => self.gpr[rt] = bus.rmor,
                        0x3F9 => self.gpr[rt] = bus.mmu.hid6, // HID6 (0x3F6 is HID5)
                        _ => {}
                    }
                    if bus.multicore {
                        self.gpr[rt] = match spr {
                            0x88 => bus.ctrl[bus.active_thread / 2],
                            0x139 => bus.hrmor,
                            0x10C => self.tb & 0xFFFF_FFFF,
                            0x10D => self.tb >> 32,
                            0x3FF => bus.active_thread as u64,
                            _ => self.gpr[rt],
                        };
                    }
                } else {
                    *reg = s;
                    if bus.multicore && spr == 0x16 {
                        self.dec_armed = true;
                        self.dec_pending = (s as u32 as i32) < 0;
                    }
                    if bus.multicore && spr == 0x11C { self.tb = (self.tb & !0xFFFF_FFFF) | (s & 0xFFFF_FFFF); }
                    if bus.multicore && spr == 0x11D { self.tb = (s << 32) | (self.tb & 0xFFFF_FFFF); }
                    if bus.multicore && spr == 0x98 {
                        let ppe = bus.active_thread / 2;
                        let sibling = bus.active_thread ^ 1;
                        let old = bus.ctrl[ppe];
                        let value = s | (old & (1 << (23 - sibling % 2)));
                        for thread in ppe * 2..ppe * 2 + 2 {
                            let bit = 1 << (23 - thread % 2);
                            if old & bit == 0 && value & bit != 0 { bus.reset[thread] = true; }
                        }
                        bus.ctrl[ppe] = value;
                    }
                    if (0x3B0..0x3B8).contains(&spr) && std::env::var_os("XB_TRACE_SPR").is_some() {
                        eprintln!("mtspr {spr:#x} = {s:#x} at pc {pc:#x}");
                    }
                    if spr == 0x139 {
                        self.pending_hrmor = Some(s);
                        self.hrmor_armed = false;
                    }
                    if spr == 0x19 { bus.mmu.sdr1 = s; }
                    if spr == 0x13E { bus.lpcr = s; }
                    if spr == 0x13F { bus.rmor = s; }
                    if spr == 0x3F9 { bus.mmu.hid6 = s; } // HID6 (0x3F6 is HID5)
                    if spr == 0x3B4 {
                        bus.write_tlb(self.spr.get(&0x3B3).copied().unwrap_or(0), self.spr.get(&0x3B5).copied().unwrap_or(0), s);
                    }
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
    fn protected_store_and_nx_fetch_enter_guest_storage_vectors() {
        for nx in [false, true] {
            let mut b = Bus::new();
            let raw = 0x8000_0000_0000_0000;
            b.map_ram(raw, 0x2000);
            let word = d(36, 3, 4, 0) as u64;
            b.write(raw + 0x1000, 4, word).unwrap();
            let mut c = Cpu::new(0x80001000);
            c.slb[0] = (0x88000000, 0x8000);
            c.msr = 0x3030;
            c.gpr[4] = c.pc;
            c.gpr[3] = 0xDEADBEEF;
            b.enable_software_mmu(c.slb);
            b.mmu.insert(0x18, if nx { 0x1196 } else { 0x1193 }, 0x8001);
            c.step(&mut b).unwrap();
            assert_eq!(c.pc, if nx { 0x400 } else { 0x300 });
            assert_eq!(c.spr[&0x1A], 0x80001000);
            if nx { assert_eq!(c.spr[&0x1B], 0x10003030); }
            else {
                assert_eq!(c.spr[&0x13], 0x80001000);
                assert_eq!(c.spr[&0x12], 0x0A000000);
            }
            assert_eq!(b.read(raw + 0x1000, 4), Ok(word));
        }
    }

    #[test]
    fn decrementer_latches_crossing_and_write_rearms() {
        let (mut c, mut b) = setup(&[x(31, 3, 0x16, 0, 467, 0)]);
        b.multicore = true;
        c.spr.insert(0x16, 0); // observing the default register must not arm it
        c.tick_decrementer(100);
        assert!(!c.dec_pending);
        c.spr.insert(0x16, 3);
        c.dec_armed = true;
        c.tick_decrementer(3);
        assert_eq!(c.spr[&0x16], 0);
        assert!(!c.dec_pending);
        c.tick_decrementer(1);
        assert_eq!(c.spr[&0x16], u32::MAX as u64);
        assert!(c.dec_pending);
        c.gpr[3] = 100;
        c.step(&mut b).unwrap();
        assert!(!c.dec_pending);
        c.tick_decrementer(101);
        assert!(c.dec_pending);
        c.pc = BASE;
        c.gpr[3] = 0xFFFFFFFF;
        c.step(&mut b).unwrap();
        assert!(c.dec_pending);
    }

    #[test]
    fn indexed_update_memory_ops_update_base_only_after_success() {
        for (load, store, size) in [(53, 181, 8), (55, 183, 4), (119, 247, 1), (311, 439, 2), (375, 439, 2)] {
            let (mut c, mut b) = setup(&[x(31, 3, 4, 5, store, 0), x(31, 6, 4, 0, load, 0)]);
            c.gpr[3] = u64::MAX;
            c.gpr[4] = 0x2000;
            c.gpr[5] = 8;
            steps(&mut c, &mut b, 2);
            assert_eq!(c.gpr[4], 0x2008);
            assert_eq!(c.gpr[6], if load == 375 { u64::MAX } else { u64::MAX >> (8 * (8 - size)) });
            c.pc = BASE;
            c.gpr[4] = 0xF00000;
            assert!(c.step(&mut b).is_err());
            assert_eq!(c.gpr[4], 0xF00000);
        }
    }

    #[test]
    fn andis_masks_shifted_immediate_and_always_records() {
        let (mut c, mut b) = setup(&[d(29, 3, 4, 0xA000), d(29, 3, 5, 0x1000)]);
        c.gpr[3] = 0xFFFF_FFFF_8000_1234;
        c.xer = XER_SO;
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[4], 0x8000_0000);
        assert_eq!(c.cr >> 28, 5); // legacy test setup is doubleword record mode
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[5], 0);
        assert_eq!(c.cr >> 28, 3);
    }

    #[test]
    fn kernel_word_mode_records_zero_and_carry_at_the_32_bit_boundary() {
        for sf in [false, true] {
            let mut b = Bus::new();
            let base = 0x8000_0000_0000_0000;
            b.map_ram(base, 0x2000);
            b.write(base + BASE, 4, d(13, 3, 3, 1) as u64).unwrap(); // addic. r3,r3,1
            b.write(base + BASE + 4, 4, x(31, 5, 0, 4, 138, 0) as u64).unwrap(); // adde r5,r0,r4
            b.enable_software_mmu([(0, 0); 64]);
            let mut c = Cpu::new(BASE);
            c.msr = if sf { 1 << 63 } else { 0 };
            c.gpr[3] = u32::MAX as u64;
            c.step(&mut b).unwrap();
            assert_eq!(c.cr >> 28, if sf { 4 } else { 2 });
            assert_eq!(c.xer & XER_CA != 0, !sf);
            c.step(&mut b).unwrap();
            assert_eq!(c.gpr[5], if sf { 0 } else { 1 });
        }
    }

    #[test]
    fn word_mode_subfc_uses_word_borrow_not_upper_register_bits() {
        let mut b = Bus::new();
        b.map_ram(0x8000_0000_0000_0000, 0x2000);
        b.write(0x8000_0000_0000_0000 + BASE, 4, x(31, 3, 4, 5, 8, 1) as u64).unwrap();
        b.enable_software_mmu([(0, 0); 64]);
        let mut c = Cpu::new(BASE);
        c.gpr[4] = 1 << 32;
        c.step(&mut b).unwrap();
        assert_ne!(c.xer & XER_CA, 0);
        assert_eq!(c.cr >> 28, 2);
    }

    #[test]
    fn software_mmu_system_call_enters_guest_hv_and_returns_after_sc() {
        for word in [0x4400_0002, 0x4400_0042] {
            let mut b = Bus::new();
            b.map_ram(0x8000_0000_0000_0000, 0x10000);
            b.write(0x8000_0000_0000_1000, 4, word).unwrap();
            b.write(0x8000_0000_0000_0C00, 4, 0x4C00_0024).unwrap();
            b.enable_software_mmu([(0, 0); 64]);
            let mut c = Cpu::new(0x1000);
            c.msr = 0x1000;
            c.step(&mut b).unwrap();
            assert_eq!(c.pc, 0xC00);
            assert_eq!(c.spr[&0x1A], 0x1004);
            assert_eq!(c.spr[&0x1B], 0x1000);
            assert_ne!(c.msr & MSR_HV, 0);
            c.step(&mut b).unwrap();
            assert_eq!(c.pc, 0x1004);
            assert_eq!(c.msr, 0x1000);
        }
    }

    #[test]
    fn software_mmu_misses_enter_hv_vectors_and_preserve_restart_state() {
        for segment in [true, false] {
            let mut c = Cpu::new(0x8007_8DB0);
            let mut b = Bus::new();
            if !segment { c.slb[8] = (0x8800_0008, 0x8100); }
            b.enable_software_mmu(c.slb);
            c.msr = 0x0200_3030;
            c.step(&mut b).unwrap();
            assert_eq!(c.pc, if segment { 0x480 } else { 0x400 });
            assert_eq!(c.spr[&0x1A], 0x8007_8DB0);
            assert_eq!(c.spr[&0x1B], 0x0200_3030 | if segment { 0 } else { 0x4000_0000 });
            assert_eq!(c.msr, MSR_HV | (1 << 63) | 0x1000);
            assert_eq!(c.spr[&0x3B2], 8);
            assert!(!b.translated);
        }
    }

    #[test]
    fn software_mmu_data_fault_is_not_mistaken_for_fetch_and_dar_is_32_bit() {
        let mut b = Bus::new();
        b.map_ram(0x8000_0000_0000_0000, 0x10000);
        b.write(0x8000_0000_0000_1000, 4, d(32, 3, 4, 0) as u64).unwrap();
        b.enable_software_mmu([(0, 0); 64]);
        let mut c = Cpu::new(0x1000);
        c.msr = MSR_HV | 0x10;
        c.gpr[4] = c.pc; // data EA equals PC, but this is a data segment miss
        c.step(&mut b).unwrap();
        assert_eq!(c.pc, 0x380);
        assert_eq!(c.spr[&0x13], 0x1000);
        c.pc = 0x1000;
        c.msr = MSR_HV | 0x10;
        c.gpr[4] = 0xFFFF_FFFF_8E04_8608;
        c.step(&mut b).unwrap();
        assert_eq!(c.spr[&0x13], 0x8E04_8608);
    }

    #[test]
    fn software_mmu_rfid_truncates_32_bit_kernel_entry_pc() {
        let mut b = Bus::new();
        b.map_ram(0x8000_0000_0000_0000, 0x10000);
        b.write(0x8000_0000_0000_1000, 4, 0x4C00_0024).unwrap();
        b.enable_software_mmu([(0, 0); 64]);
        let mut c = Cpu::new(0x1000);
        c.spr.insert(0x1A, 0xFFFF_FFFF_8007_8DB0);
        c.spr.insert(0x1B, 0x0200_3030);
        c.step(&mut b).unwrap();
        assert_eq!(c.pc, 0x8007_8DB0);
    }

    #[test]
    fn carry_arithmetic_handles_both_overflow_stages() {
        let cases = [
            (10, u64::MAX, 1, false, 0, true),
            (10, 1, 2, true, 3, false),
            (138, u64::MAX, 0, true, 0, true),
            (138, u64::MAX, u64::MAX, true, u64::MAX, true),
            (138, 1, 2, false, 3, false),
            (202, u64::MAX, 99, true, 0, true),
            (202, 5, 99, false, 5, false),
            (200, 0, 99, true, 0, true),
            (200, 1, 99, true, u64::MAX, false),
            (200, 0, 99, false, u64::MAX, false),
        ];
        for (xo, a, b, carry, expected, carry_out) in cases {
            let (mut c, mut bus) = setup(&[x(31, 3, 4, 5, xo, 1)]);
            c.gpr[4] = a;
            c.gpr[5] = b;
            c.xer = XER_SO | if carry { XER_CA } else { 0 };
            steps(&mut c, &mut bus, 1);
            assert_eq!(c.gpr[3], expected, "XO {xo}");
            assert_eq!(c.xer & XER_CA != 0, carry_out, "XO {xo}");
            assert_ne!(c.xer & XER_SO, 0);
            assert_eq!(c.cr >> 28, if expected == 0 { 3 } else if expected >> 63 != 0 { 9 } else { 5 });
        }
    }

    #[test]
    fn cr_logical_truth_tables_preserve_other_bits() {
        // Truth table bit index = 2*A+B.
        for (xo, truth) in [(33, 0b0001), (129, 0b0100), (193, 0b0110), (225, 0b0111),
            (257, 0b1000), (289, 0b1001), (417, 0b1101), (449, 0b1110)] {
            for inputs in 0..4 {
                let (mut c, mut b) = setup(&[x(19, 31, 4, 24, xo, 0)]);
                c.cr = 0xA555_5555 & !((1 << 27) | (1 << 7));
                c.cr |= ((inputs >> 1) << 27) | ((inputs & 1) << 7);
                let before = c.cr;
                steps(&mut c, &mut b, 1);
                assert_eq!(c.cr, (before & !1) | (truth >> inputs & 1));
                assert_eq!(c.pc, BASE + 4);
                assert_eq!(c.tb, 1);
            }
        }
    }

    #[test]
    fn cntlzd_counts_all_64_bits_and_records() {
        for (value, expected) in [(0, 64), (1, 63), (0x10001, 47), (1 << 63, 0)] {
            let (mut c, mut b) = setup(&[x(31, 3, 4, 0, 58, 1)]);
            c.gpr[3] = value;
            c.xer = XER_SO;
            steps(&mut c, &mut b, 1);
            assert_eq!(c.gpr[4], expected);
            assert_eq!(c.cr >> 28, if expected == 0 { 3 } else { 5 });
        }
    }

    #[test]
    fn reserve_loads_and_conditional_stores_update_only_cr0() {
        for (load_xo, store_xo, size, value) in [(20, 150, 4, 0x1234_5678), (84, 214, 8, 0x1234_5678_9ABC_DEF0)] {
            let (mut c, mut b) = setup(&[
                x(31, 3, 0, 4, load_xo, 0),
                x(31, 5, 0, 4, store_xo, 1),
                x(31, 5, 0, 4, store_xo, 1),
            ]);
            c.gpr[4] = 0x2000;
            c.gpr[5] = value;
            c.xer = XER_SO;
            c.cr = 0x8ABC_DEF0;
            b.write(0x2000, size, 0xDEAD).unwrap();
            steps(&mut c, &mut b, 1);
            assert_eq!(c.gpr[3], 0xDEAD);
            steps(&mut c, &mut b, 1);
            assert_eq!(b.read(0x2000, size), Ok(value));
            assert_eq!(c.cr, 0x3ABC_DEF0); // EQ | SO
            steps(&mut c, &mut b, 1);
            assert_eq!(c.cr, 0x1ABC_DEF0); // reservation consumed
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
    fn instruction_and_data_translation_are_independent_and_errors_reset_bus_mode() {
        let (mut c, mut b) = setup(&[0x8064_0000]); // lwz r3,0(r4)
        b.map_ram(0x8000_0200_0000_0000, 0x10000);
        b.write(0x8000_0200_0000_00FC, 4, 0x1234).unwrap();
        b.write_tlb(0x102, 0x0200_0000_11BE, 0x1005);
        c.gpr[4] = 0x1000_00FC;
        c.msr = 0x10; // DR on, IR off
        c.step(&mut b).unwrap();
        assert_eq!(c.gpr[3], 0x1234);
        c.pc = BASE;
        c.gpr[4] = 0x2000_0000;
        assert!(c.step(&mut b).is_err());
        assert_eq!(b.read(BASE, 4), Ok(0x8064_0000));

        b.write(0x8000_0200_0000_0000, 4, 0x3860_0007).unwrap();
        c.pc = 0x1000_0000;
        c.msr = 0x20; // IR on, DR off
        c.step(&mut b).unwrap();
        assert_eq!(c.gpr[3], 7);
    }

    #[test]
    fn extsw_sign_extends() {
        let (mut c, mut b) = setup(&[x(31, 3, 4, 0, 986, 0)]);
        c.gpr[3] = 0x0000_0000_8000_0000;
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[4], 0xFFFF_FFFF_8000_0000);
    }

    #[test]
    fn cd_integer_instructions() {
        for (xo, value, expected) in [(954, 0x80u64, 0xFFFFFFFFFFFFFF80u64), (922, 0x8000u64, 0xFFFFFFFFFFFF8000u64)] {
            let (mut c, mut b) = setup(&[x(31, 3, 4, 0, xo, 1)]);
            c.gpr[3] = value;
            steps(&mut c, &mut b, 1);
            assert_eq!(c.gpr[4], expected);
            assert_eq!(c.cr >> 28, 8);
        }
        let (mut c, mut b) = setup(&[x(31, 3, 4, 0, 104, 0), x(31, 5, 4, 3, 8, 0)]);
        c.gpr[4] = 1;
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[3], u64::MAX);
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[5], u64::MAX - 1);
        assert_ne!(c.xer & XER_CA, 0);
    }

    #[test]
    fn srawi_sign_extends_and_updates_carry() {
        for (value, shift, expected, carry) in [(0x8000_0001u64, 3u64, 0xFFFFFFFFF0000000u64, true), (0x8000_0000u64, 0u64, 0xFFFFFFFF80000000u64, false), (8u64, 3u64, 1u64, false)] {
            let (mut c, mut b) = setup(&[x(31, 3, 4, shift as u32, 824, 0)]);
            c.gpr[3] = value;
            c.xer = XER_CA;
            steps(&mut c, &mut b, 1);
            assert_eq!(c.gpr[4], expected);
            assert_eq!(c.xer & XER_CA != 0, carry);
        }
    }

    #[test]
    fn indexed_halfword_loads_and_store() {
        let (mut c, mut b) = setup(&[x(31, 3, 4, 5, 407, 0), x(31, 6, 4, 5, 279, 0), x(31, 7, 4, 5, 343, 0)]);
        c.gpr[3] = 0x1234_8001;
        c.gpr[4] = 0x2000;
        c.gpr[5] = 2;
        steps(&mut c, &mut b, 3);
        assert_eq!(b.read(0x2002, 2), Ok(0x8001));
        assert_eq!(c.gpr[6], 0x8001);
        assert_eq!(c.gpr[7], 0xFFFFFFFFFFFF8001);
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
    fn mtcrf_and_mtocrf_preserve_unselected_fields() {
        for (fxm, single, expected) in [(0x02, true, 0x1234_56D8), (0x81, false, 0xA234_567F), (0, false, 0x1234_5678)] {
            let w = x(31, 5, 0, 0, 144, 0) | fxm << 12 | (single as u32) << 20;
            let (c, _) = run1(w, |c, _| {
                c.cr = 0x1234_5678;
                c.gpr[5] = 0xDEAD_BEEF_ABCD_EFDF;
            });
            assert_eq!(c.cr, expected);
        }
    }

    #[test]
    fn mulhdu_returns_unsigned_high_half_and_honors_record_bit() {
        for (a, b, expected) in [
            (0, u64::MAX, 0),
            (u64::MAX, u64::MAX, u64::MAX - 1),
            (1 << 63, 2, 1),
            (0x1234_5678_9ABC_DEF0, 0xFEDC_BA98_7654_3210, 0x121F_A00A_D77D_7422),
        ] {
            let (c, _) = run1(x(31, 3, 4, 5, 9, 1), |c, _| {
                c.gpr[4] = a;
                c.gpr[5] = b;
                c.xer = XER_SO | XER_CA;
            });
            assert_eq!(c.gpr[3], expected);
            assert_eq!(c.xer, XER_SO | XER_CA);
            assert_eq!(c.cr >> 28, if expected == 0 { 3 } else if expected >> 63 != 0 { 9 } else { 5 });
        }
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

    #[test]
    fn hid6_lives_at_spr_3f9_and_is_stored_for_the_mmu() {
        // mtspr 0x3F9,r3 ; mfspr r4,0x3F9 -- HID6 (0x3F6 is HID5).
        let (mut c, mut b) = setup(&[
            x(31, 3, 0x3F9 & 31, 0x3F9 >> 5, 467, 0),
            x(31, 4, 0x3F9 & 31, 0x3F9 >> 5, 339, 0),
        ]);
        c.gpr[3] = 0x0001_0038_0000_0000;
        steps(&mut c, &mut b, 2);
        assert_eq!(c.gpr[4], 0x0001_0038_0000_0000);
        assert_eq!(b.mmu.hid6, 0x0001_0038_0000_0000);
        assert!(b.mmu.large_pages_enabled());
    }

    #[test]
    fn hid5_spr_3f6_stays_independent_of_hid6() {
        let (mut c, mut b) = setup(&[
            x(31, 3, 0x3F6 & 31, 0x3F6 >> 5, 467, 0),
            x(31, 4, 0x3F6 & 31, 0x3F6 >> 5, 339, 0),
        ]);
        c.gpr[3] = 0x55;
        steps(&mut c, &mut b, 2);
        assert_eq!(c.gpr[4], 0x55);
        assert_eq!(b.mmu.hid6, 0);
    }

    #[test]
    fn hrmor_takes_effect_at_the_next_context_synchronization() {
        // mtspr 0x139,r3 ; isync
        let (mut c, mut b) = setup(&[x(31, 3, 0x139 & 31, 0x139 >> 5, 467, 0), xl(150, 0, 0, 0)]);
        c.gpr[3] = 0x017A_0300_0000;
        steps(&mut c, &mut b, 1);
        assert_eq!(b.hrmor, 0);
        assert_eq!(c.spr[&0x139], 0x017A_0300_0000);
        steps(&mut c, &mut b, 1);
        assert_eq!(b.hrmor, 0x017A_0300_0000);
    }

    #[test]
    fn hrmor_takes_effect_at_rfid_after_fetching_it_with_the_old_value() {
        // mtspr 0x139,r3 ; rfid -- the rfid is fetched under the old HRMOR
        let (mut c, mut b) = setup(&[x(31, 3, 0x139 & 31, 0x139 >> 5, 467, 0), xl(18, 0, 0, 0)]);
        c.gpr[3] = 0x4000;
        c.spr.insert(0x1A, 0x20);
        steps(&mut c, &mut b, 2);
        assert_eq!(b.hrmor, 0x4000);
        assert_eq!(c.pc, 0x20);
    }

    #[test]
    fn hrmor_takes_effect_for_the_fetch_after_a_blr_that_follows_the_mtspr() {
        // mtspr 0x139,r3 ; blr  -- the target is fetched with the new HRMOR
        let (mut c, mut b) = setup(&[x(31, 3, 0x139 & 31, 0x139 >> 5, 467, 0), 19 << 26 | 20 << 21 | 16 << 1]);
        c.gpr[3] = 0x10000;
        c.lr = 0x40;
        steps(&mut c, &mut b, 1);
        assert_eq!(b.hrmor, 0);
        steps(&mut c, &mut b, 1);
        assert_eq!((c.pc, b.hrmor), (0x40, 0x10000));
    }

    #[test]
    fn op_mtmsr_mtmsrd() {
        let (c, _) = run1(x(31, 3, 0, 0, 146, 0), |c, _| c.gpr[3] = MSR_HV | 0x30);
        assert_eq!(c.msr, MSR_HV | 0x30);
        let (c, _) = run1(x(31, 3, 0, 0, 178, 0), |c, _| {
            c.gpr[3] = u64::MAX;
            c.msr = MSR_HV;
        });
        assert_eq!(c.msr, u64::MAX);
        // L=1 (bit 16 of the word) only takes EE and RI
        let (c, _) = run1(x(31, 3, 1, 0, 178, 0), |c, _| {
            c.gpr[3] = u64::MAX;
            c.msr = MSR_HV;
        });
        assert_eq!(c.msr, MSR_HV | 0x8002);
    }

    #[test]
    fn op_slbmte_and_back() {
        // slbmte r3(RS), r4(RB) ; slbmfee r5, r4 ; slbmfev r6, r4
        let (mut c, mut b) = setup(&[x(31, 3, 0, 4, 402, 0), x(31, 5, 0, 4, 915, 0), x(31, 6, 0, 4, 851, 0)]);
        c.gpr[3] = 0x1310;
        c.gpr[4] = 0x1800_0001;
        steps(&mut c, &mut b, 3);
        assert_eq!((c.gpr[5], c.gpr[6]), (0x1800_0001, 0x1310));
    }
}
