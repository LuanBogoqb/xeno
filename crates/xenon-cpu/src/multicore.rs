//! Deterministic six-thread interpreter scheduler (experimental post-CD mode).
//! Registers/SLBs are per thread; software TLB and HRMOR are per PPE. Memory,
//! IIC and reservation invalidation are shared. Not cycle-accurate SMT.
use crate::{cpu::{Cpu, MSR_HV}, hooks::Hooks, mem::Bus, mmu::Mmu,
    runner::{run_until, StageEnd}};

pub struct Machine {
    pub cpus: [Cpu; 6],
    pub steps: [u64; 6],
    pub stopped_thread: Option<usize>,
    mmus: [Mmu; 3],
    hrmor: [u64; 3],
    current_ppe: usize,
    clock: u64,
    dec_clock: [u64; 6],
    timebase: [u64; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine() -> (Machine, Bus) {
        let mut bus = Bus::new();
        bus.enable_software_mmu([(0, 0); 64]);
        (Machine::from_cd(Cpu::new(0x100), &mut bus), bus)
    }

    #[test]
    fn decrementer_waits_for_ee_then_enters_guest_vector() {
        let (mut m, mut b) = machine();
        let base = 0x8000_0000_0000_0000;
        b.map_ram(base, 0x1000);
        b.write(base + 0x100, 4, 0x48000000).unwrap(); // b .
        b.write(base + 0x900, 4, 0x48000000).unwrap();
        m.cpus[0].msr = MSR_HV;
        m.cpus[0].spr.insert(0x16, 1);
        m.cpus[0].dec_armed = true;
        m.run(&mut b, &Hooks::new(), u64::MAX, 3);
        assert_eq!(m.cpus[0].pc, 0x100);
        assert!(m.cpus[0].dec_pending);
        m.cpus[0].msr |= 0x8000;
        m.run(&mut b, &Hooks::new(), u64::MAX, 1);
        assert_eq!(m.cpus[0].pc, 0x900);
        assert_eq!(m.cpus[0].spr[&0x1A], 0x100);
        assert_eq!(m.cpus[0].spr[&0x1B], MSR_HV | 0x8000);
        assert!(!m.cpus[0].dec_pending);
    }

    #[test]
    fn translation_is_per_ppe_but_slb_is_per_thread() {
        let (mut m, mut b) = machine();
        b.hrmor = 0x100;
        b.mmu.insert(8, 0x1234, 5);
        m.cpus[0].slb[0] = (0x8000000, 0x100);
        m.cpus[1].slb[0] = (0x18000001, 0x1100);
        m.select(&mut b, 1);
        assert_eq!(b.mmu.entries().len(), 1);
        assert_eq!(b.mmu.slb[0], m.cpus[1].slb[0]);
        m.select(&mut b, 2);
        assert!(b.mmu.entries().is_empty());
        assert_eq!(b.hrmor, 0);
        b.hrmor = 0x200;
        b.mmu.insert(4, 0x5678, 5);
        m.select(&mut b, 0);
        assert_eq!(b.hrmor, 0x100);
        assert_eq!(b.mmu.entries(), &[(8, 0x1234, 5)]);
        assert_eq!(b.mmu.slb[0], m.cpus[0].slb[0]);
        m.select(&mut b, 3);
        assert_eq!(b.hrmor, 0x200);
        assert_eq!(b.mmu.entries(), &[(4, 0x5678, 5)]);
    }

    #[test]
    fn reservations_survive_switches_but_other_thread_stores_break_them() {
        let (mut m, mut b) = machine();
        let base = 0x8000_0000_0000_0000;
        b.map_ram(base, 0x1000);
        b.map_alias(0x3000, 0x1000, base);
        m.select(&mut b, 0);
        b.load_reserve(base, 4).unwrap();
        m.select(&mut b, 2);
        b.load_reserve(base, 4).unwrap();
        m.select(&mut b, 0);
        assert_eq!(b.store_conditional(base, 4, 1), Ok(true));
        m.select(&mut b, 2);
        assert_eq!(b.store_conditional(0x3000, 4, 2), Ok(false));
        b.load_reserve(base, 4).unwrap();
        m.select(&mut b, 1);
        b.write(0x3004, 4, 3).unwrap();
        m.select(&mut b, 2);
        assert_eq!(b.store_conditional(base, 4, 2), Ok(false));
    }

    #[test]
    fn ctrl_enables_sibling_at_reset_without_disabling_running_sibling() {
        let (mut m, mut b) = machine();
        let base = 0x8000_0000_0000_0000;
        b.map_ram(base, 0x1000);
        let mtspr = 31u32 << 26 | 3 << 21 | (0x98 & 31) << 16 | (0x98 >> 5) << 11 | 467 << 1;
        b.write(base + 0x100, 4, mtspr as u64).unwrap();
        m.cpus[0].gpr[3] = 0xC00000;
        m.cpus[0].step(&mut b).unwrap();
        assert_eq!(b.ctrl[0], 0xC00000);
        assert_eq!(b.reset, [false, true, false, false, false, false]);
        m.select(&mut b, 1);
        m.cpus[1].gpr[3] = 0;
        m.cpus[1].step(&mut b).unwrap();
        assert_eq!(b.ctrl[0], 0x800000);
    }
}

impl Machine {
    /// Only the primary has executed CD. Secondary threads start disabled at
    /// reset, inheriting boot HRMOR per PPE, not CPU0's guest PCR/SLB state.
    pub fn from_cd(primary: Cpu, bus: &mut Bus) -> Self {
        assert!(bus.mmu.enabled, "multicore requires the post-CD software MMU");
        let mut cpus = std::array::from_fn(|_| Cpu::new(0x100));
        cpus[0] = primary;
        for cpu in &mut cpus[1..] { cpu.msr = MSR_HV; }
        bus.multicore = true;
        bus.active_thread = 0;
        bus.ctrl = [1 << 23, 0, 0];
        let tb = cpus[0].tb;
        Self { cpus, steps: [0; 6], stopped_thread: None, clock: 0, dec_clock: [0; 6], timebase: [tb; 3], mmus: std::array::from_fn(|_| {
            let mut m = Mmu::default(); m.enabled = true; m
        }), hrmor: [bus.hrmor; 3], current_ppe: 0 }
    }

    fn select(&mut self, bus: &mut Bus, thread: usize) {
        let ppe = thread / 2;
        if self.current_ppe != ppe {
            self.hrmor[self.current_ppe] = bus.hrmor;
            std::mem::swap(&mut self.mmus[self.current_ppe], &mut bus.mmu);
            std::mem::swap(&mut self.mmus[ppe], &mut bus.mmu);
            bus.hrmor = self.hrmor[ppe];
            self.current_ppe = ppe;
        }
        bus.active_thread = thread;
        bus.mmu.slb = self.cpus[thread].slb;
    }

    /// Stop-PC applies to CPU0; budget counts instructions across all CPUs.
    pub fn run(&mut self, bus: &mut Bus, hooks: &Hooks, stop_pc: u64, budget: u64) -> (StageEnd, u64) {
        let mut total = 0;
        self.stopped_thread = None;
        loop {
            if self.cpus[0].pc == stop_pc {
                self.select(bus, 0);
                return (StageEnd::Handoff { pc: stop_pc }, total);
            }
            let mut progressed = false;
            for thread in 0..6 {
                let bit = 1 << (23 - thread % 2);
                // Startup IPI wakes a disabled PPE at reset (guest HV sc1).
                if thread % 2 == 0 && bus.ctrl[thread / 2] & bit == 0 && bus.iic.pending(thread) {
                    bus.ctrl[thread / 2] |= bit;
                    bus.reset[thread] = true;
                }
                if bus.ctrl[thread / 2] & bit == 0 { continue; }
                self.select(bus, thread);
                self.cpus[thread].tick_decrementer(self.clock - self.dec_clock[thread]);
                self.dec_clock[thread] = self.clock;
                if std::mem::take(&mut bus.reset[thread]) { self.cpus[thread].interrupt(0x100); }
                for _ in 0..256 {
                    if total == budget {
                        self.select(bus, 0);
                        return (StageEnd::StepLimit { pc: self.cpus[0].pc }, total);
                    }
                    if thread == 0 && self.cpus[0].pc == stop_pc { break; }
                    if bus.ctrl[thread / 2] & bit == 0 { break; }
                    if self.cpus[thread].msr & 0x8000 != 0 && bus.iic.pending(thread) {
                        self.cpus[thread].interrupt(0x500);
                    } else if self.cpus[thread].msr & 0x8000 != 0 && self.cpus[thread].dec_pending {
                        self.cpus[thread].dec_pending = false;
                        self.cpus[thread].interrupt(0x900);
                    }
                    self.cpus[thread].tb = self.timebase[thread / 2];
                    let (end, n) = run_until(&mut self.cpus[thread], bus, hooks, u64::MAX, 1);
                    self.timebase[thread / 2] = self.cpus[thread].tb;
                    for ppe in 0..3 {
                        if ppe != thread / 2 { self.timebase[ppe] = self.timebase[ppe].wrapping_add(n); }
                    }
                    self.clock += n;
                    self.cpus[thread].tick_decrementer(n);
                    self.dec_clock[thread] = self.clock;
                    total += n;
                    self.steps[thread] += n;
                    progressed = true;
                    if matches!(end, StageEnd::Stop(_)) {
                        self.stopped_thread = Some(thread);
                        eprintln!("CPU{thread} stopped at {:#x}", self.cpus[thread].pc);
                        return (end, total);
                    }
                }
            }
            if !progressed {
                self.select(bus, 0);
                return (StageEnd::StepLimit { pc: self.cpus[0].pc }, total);
            }
        }
    }
}
