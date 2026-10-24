//! Runs a CB stage until it jumps to the next stage's entry.
use crate::cpu::{Cpu, Stop};
use crate::hooks::{Action, Hooks, Override};
use crate::mem::Bus;

#[derive(Debug, PartialEq, Eq)]
pub enum StageEnd {
    Handoff { pc: u64 },
    Stop(Stop),
    StepLimit { pc: u64 },
}

/// Returns why it ended and how many instructions ran.
pub fn run_until(cpu: &mut Cpu, bus: &mut Bus, hooks: &Hooks, handoff_pc: u64, max_steps: u64) -> (StageEnd, u64) {
    let mut steps = 0;
    loop {
        if cpu.pc == handoff_pc {
            return (StageEnd::Handoff { pc: cpu.pc }, steps);
        }
        if steps == max_steps {
            return (StageEnd::StepLimit { pc: cpu.pc }, steps);
        }
        for a in hooks.at(cpu.pc) {
            match a {
                Action::SetGpr { reg, val } => {
                    if cpu.gpr[*reg] != *val {
                        hooks.record(Override { pc: cpu.pc, reg: *reg, old: cpu.gpr[*reg] });
                    }
                    cpu.gpr[*reg] = *val;
                }
                Action::Log(m) => eprintln!("hook {:#x}: {m}", cpu.pc),
            }
        }
        if let Err(s) = cpu.step(bus) {
            return (StageEnd::Stop(s), steps);
        }
        steps += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDI_R3_1: u32 = 0x3860_0001; // addi r3,0,1
    const B_SELF: u32 = 0x4800_0000; // b .

    fn bus_with(words: &[u32]) -> Bus {
        let mut b = Bus::new();
        b.map_ram(0x1000, 0x100);
        for (i, w) in words.iter().enumerate() {
            b.write(0x1000 + 4 * i as u64, 4, *w as u64).unwrap();
        }
        b
    }

    #[test]
    fn stops_at_handoff_pc() {
        let mut bus = bus_with(&[ADDI_R3_1; 3]);
        let mut cpu = Cpu::new(0x1000);
        let (end, n) = run_until(&mut cpu, &mut bus, &Hooks::new(), 0x100C, 100);
        assert_eq!((end, n), (StageEnd::Handoff { pc: 0x100C }, 3));
    }

    #[test]
    fn hook_sets_gpr_before_fetch() {
        let mut bus = bus_with(&[0x6000_0000, 0x6000_0000]); // nop, nop
        let mut cpu = Cpu::new(0x1000);
        cpu.gpr[3] = 5;
        let mut h = Hooks::new();
        h.add(0x1000, Action::SetGpr { reg: 3, val: 0 });
        run_until(&mut cpu, &mut bus, &h, 0x1008, 100);
        assert_eq!(cpu.gpr[3], 0);
        assert_eq!(h.overrides(), vec![Override { pc: 0x1000, reg: 3, old: 5 }]);
    }

    #[test]
    fn step_limit_reports_pc() {
        let mut bus = bus_with(&[B_SELF]);
        let mut cpu = Cpu::new(0x1000);
        let (end, n) = run_until(&mut cpu, &mut bus, &Hooks::new(), 0xFFFF, 10);
        assert_eq!((end, n), (StageEnd::StepLimit { pc: 0x1000 }, 10));
    }
}
