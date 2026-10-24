//! Memory map and register state the 1BL leaves for CB_A (see research/cb/README.md).
use crate::cpu::{Cpu, MSR_HV};

pub const SRAM_BASE: u64 = 0x8000_0200_0000_0000;
/// Covers the CB load area, its relocated copy and the stack (up to 0x1_F300).
pub const SRAM_SIZE: usize = 0x2_0000;
pub const CB_A_LOAD: u64 = 0x8000_0200_0001_0000;
/// Flash MMIO window; CB_A computes `FLASH_BASE + r31` to read the next stage header.
pub const FLASH_BASE: u64 = 0x8000_0200_C800_0000;
pub const FLASH_SIZE: u64 = 0x800_0000;
/// Flash offset of the stage CB_A loads next (CB_B, 0x9EF0 in this image); the 1BL passes it in r31.
pub const CB_NEXT_FLASH_OFFSET: u64 = 0x9EF0;
pub const SOC_BASE: u64 = 0x8000_0200_0006_0000;
pub const SOC_SIZE: u64 = 0x1_0000;

/// 1BL hand-off: r0 is 0 and r31 is the next stage's flash offset (the 1BL argument; code at 0x840 requires it 16-aligned and in 0x80..0x7FFFF80), everything else is set by CB_A itself.
pub fn initial_regs(cpu: &mut Cpu) {
    cpu.gpr[0] = 0;
    cpu.gpr[31] = CB_NEXT_FLASH_OFFSET;
    cpu.msr |= MSR_HV;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::{Cpu, MSR_HV};

    #[test]
    fn initial_regs_matches_readme() {
        let mut c = Cpu::new(CB_A_LOAD + 0x3D0);
        c.gpr[0] = 7;
        c.gpr[31] = 7;
        initial_regs(&mut c);
        assert_eq!(c.gpr[0], 0);
        assert_eq!(c.gpr[31], CB_NEXT_FLASH_OFFSET);
        assert_ne!(c.msr & MSR_HV, 0);
        assert_eq!(CB_A_LOAD, 0x8000_0200_0001_0000);
        assert!(CB_A_LOAD >= SRAM_BASE && CB_A_LOAD + 0x1AF0 <= SRAM_BASE + SRAM_SIZE as u64);
        assert!(0x8000_0200_0001_F300 < SRAM_BASE + SRAM_SIZE as u64);
    }
}
