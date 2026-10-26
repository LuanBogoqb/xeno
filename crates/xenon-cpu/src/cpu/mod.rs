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
mod tests;
