//! Memory map and register state the 1BL leaves for CB_A (see research/cb/README.md).
use crate::cpu::{Cpu, MSR_HV};
use crate::dev::flash::FlashWindow;
use crate::dev::fuses::FuseLines;
use crate::dev::smc::SmcMailbox;
use crate::dev::soc::SocRegs;
use crate::mem::Bus;

pub const SRAM_BASE: u64 = 0x8000_0200_0000_0000;
/// Covers the CB load area, its relocated copy and the stack (up to 0x1_F300).
pub const SRAM_SIZE: usize = 0x2_0000;
pub const CB_A_LOAD: u64 = 0x8000_0200_0001_0000;
/// Flash MMIO window; CB_A computes `FLASH_BASE + r31` to read the next stage header.
pub const FLASH_BASE: u64 = 0x8000_0200_C800_0000;
pub const FLASH_SIZE: u64 = 0x800_0000;
/// Flash offset of the stage CB_A loads next (CB_B, 0x9EF0 in this image); the 1BL passes it in r31.
pub const CB_NEXT_FLASH_OFFSET: u64 = 0x9EF0;
/// 32-bit real-mode alias of the SRAM: CB_A jumps to `0x0200_0000 + (entry & 0xFFFF)` (code at 0x4A8..0x4B0).
pub const SRAM_ALIAS: u64 = 0x0200_0000;
/// Relocated address of the CB_A digest compare result (r3), forced to "equal" (no glitch).
pub const CB_A_DIGEST_HOOK: u64 = CB_A_LOAD + 0xC000 + 0x7F0;
/// Locked-cache RAM CB_B allocates with `dcbzl` (0x548..0x578), copies itself into at +0x3000 and
/// continues from via `rfid` with HRMOR = 0x017A_0300_0000. 0x4_0000 covers every constant seen (up to 0x0301_02F0).
pub const CACHE_RAM_BASE: u64 = 0x8000_017A_0300_0000;
pub const CACHE_RAM_SIZE: usize = 0x4_0000;
/// Main memory (512 MiB on Corona-era 360s). CB_B's bytecode writes it right after programming the memory
/// controller (first write: +0x4 at 0x7B8..).
pub const DRAM_BASE: u64 = 0x8000_0000_0000_0000;
pub const DRAM_SIZE: usize = 0x2000_0000;
/// Where CB_B's code runs once it has copied itself into the locked cache (0x0300_0000 | image offset + 0x3000).
pub const CB_B_CACHE_CODE: u64 = 0x0300_3000;
/// CB_B's register-programming bytecode interpreter: loop head (image 0x9A8) and its success exit (0xDDC, r5 = 0).
/// HLE: the script only configures memory controller / PCI / SMC hardware, so it is skipped (decisions.md 3).
pub const CB_B_VM_LOOP: u64 = CB_B_CACHE_CODE + 0x9A8;
pub const CB_B_VM_OK_EXIT: u64 = CB_B_CACHE_CODE + 0xDDC;
/// Second locked-cache window, zeroed by CB_B 0x754..0x778 (0x200 lines of 128 bytes) and passed to the next stage in r8.
pub const CACHE_RAM2_BASE: u64 = 0x8000_017C_0400_0000;
pub const CACHE_RAM2_SIZE: usize = 0x2_0000;
/// Effective addresses the locked-cache windows answer to after CB_B programs SPRs 0x3B3..0x3B5 (0x640..0x668 and
/// 0xEF8..0xF20: 0x308/0x305 and 0x408/0x405 with the window base). The CB_B -> CD handoff jumps to 0x0400_0000 + entry.
pub const CACHE_WINDOW1_EA: u64 = 0x0300_0000;
pub const CACHE_WINDOW2_EA: u64 = 0x0400_0000;
/// Flash offset of the stage after CB_B (CD, 0x11A30 in this image); CB_A leaves it in r31 for CB_B.
pub const CD_FLASH_OFFSET: u64 = 0x11A30;
pub const SOC_BASE: u64 = 0x8000_0200_0006_0000;
pub const SOC_SIZE: u64 = 0x1_0000;

/// Register blocks modelled as write-and-read-back files until a trace shows they need behavior.
/// Evidence for each base is in research/cb/README.md.
pub const FUSE_BASE: u64 = 0x8000_0200_0002_0000;
pub const FUSE_SIZE: u64 = 0x2000;

/// Reset values: (absolute address, value, evidence).
pub const REG_PRESETS: &[(u64, u64, &str)] = &[
    // CB_B 0x5000..0x5028: bit 61 must equal "fuse line 0 has data in its low 56 bits" (set for this console)
    (0x8000_0200_0006_1000, 0x2000_0000_0000_0000, "fuse status"),
    // CB_B 0x724..0x73C: lwbrx chip id, top half must be >= 0x2000 (else POST 0xAF); >= 0x4000 also sets header flag 0x80.
    // Corona is a late chip, so LE 0x5000_0000 (raw 0x0000_0050). The exact id is a guess.
    (0x8000_0200_E104_0000, 0x0000_0050, "chip id (guess)"),
];

/// SMC mailbox (PCI BAR the stages program at config 0xD015_0010 to 0xEA00_1000).
pub const SMC_BASE: u64 = 0x8000_0000_EA00_1000;
pub const SMC_SIZE: u64 = 0x1000;

/// Bits that always read set: (absolute address, mask as seen on the bus, evidence).
pub const REG_OR_MASKS: &[(u64, u64, &str)] = &[
    // CB_B bytecode 0x1154..0x115C: ((reg >> 8) & 0xF) must equal 4 after the register is programmed; the registers
    // are little-endian (lwbrx), so LE 0x400 is raw 0x0004_0000. Meaning (link state) is a guess.
    (0x8000_0000_E400_15EC, 0x0004_0000, "link state field = 4"),
];

pub const REG_BLOCKS: &[(&str, u64, u64)] = &[
    // SoC page 0x6_0000 (CB_A/CB_B stage markers), 0x3_0000 (CB_B 0x45C), 0x2_4000..0x4_0000 (CB_B 0x4D00..: address windows)
    ("periph", FUSE_BASE + FUSE_SIZE, 0xE_0000 - FUSE_SIZE),
    // CB_B's register-programming bytecode (0x9A8..0xDD4 interpreter) stwbrx/lwbrx here, e.g. +0x8010
    // CB_B bytecode reads 0x2000_1800, just past the 512 MiB of DRAM (memory-size probe); read/write-back until its expectation is known
    ("dram-hole", DRAM_BASE + DRAM_SIZE as u64, 0xC000_0000 - DRAM_SIZE as u64),
    ("pcicfg", 0x8000_0000_C000_0000, 0x2A00_1000),
    ("pcicfg2", SMC_BASE + SMC_SIZE, 0x15FF_E000),
    // CB_B 0x720: lwbrx of the chip id, top half compared with 0x2000
    // CB_B 0x8640..: writes 0x8000_0200_D000_0010 (a second window onto the same kind of config space, behind the flash window)
    ("pcicfg-b", 0x8000_0200_D000_0000, 0x3000_0000),
    // CB_B 0x86B0..: writes 0x8000_0200_EA00_0000; the rest of the 0xE000_0000 peripheral window, around the chip id page
    ("hw-e0", 0x8000_0200_E000_0000, 0x0104_0000),
    ("hw-e1", 0x8000_0200_E105_0000, 0x2000_0000 - 0x0105_0000),
    ("chipid", 0x8000_0200_E104_0000, 0x1_0000),
];

/// The platform as the CB stages see it: SRAM (with its 32-bit alias), flash window and register blocks.
pub fn build_bus(flash: Vec<u8>, fuses: FuseLines) -> Bus {
    let mut bus = Bus::new();
    bus.map_ram(SRAM_BASE, SRAM_SIZE);
    bus.map_alias(SRAM_ALIAS, 0x1_0000, CB_A_LOAD);
    bus.map_ram(DRAM_BASE, DRAM_SIZE);
    bus.map_ram(CACHE_RAM_BASE, CACHE_RAM_SIZE);
    bus.map_alias(CACHE_RAM_BASE & !(1 << 63), CACHE_RAM_SIZE as u64, CACHE_RAM_BASE);
    bus.map_alias(CACHE_WINDOW1_EA, CACHE_RAM_SIZE as u64, CACHE_RAM_BASE);
    bus.map_ram(CACHE_RAM2_BASE, CACHE_RAM2_SIZE);
    bus.map_alias(CACHE_RAM2_BASE & !(1 << 63), CACHE_RAM2_SIZE as u64, CACHE_RAM2_BASE);
    bus.map_alias(CACHE_WINDOW2_EA, CACHE_RAM2_SIZE as u64, CACHE_RAM2_BASE);
    bus.map_device(FLASH_BASE, FLASH_SIZE, Box::new(FlashWindow::new(flash)));
    bus.map_device(SMC_BASE, SMC_SIZE, Box::new(SmcMailbox::new(SMC_BASE)));
    bus.map_device(FUSE_BASE, FUSE_SIZE, Box::new(fuses));
    for (name, base, size) in REG_BLOCKS {
        let mut regs = SocRegs::named(name, *base);
        for (addr, val, _) in REG_PRESETS.iter().filter(|(a, _, _)| a >= base && a < &(base + size)) {
            regs = regs.preset(*addr, *val);
        }
        for (addr, mask, _) in REG_OR_MASKS.iter().filter(|(a, _, _)| a >= base && a < &(base + size)) {
            regs = regs.or_mask(*addr, *mask);
        }
        bus.map_device(*base, *size, Box::new(regs));
    }
    bus
}

/// 1BL hand-off: r0 is 0 and r31 is the next stage's flash offset (the 1BL argument; code at 0x840 requires it 16-aligned and in 0x80..0x7FFFF80), everything else is set by CB_A itself.
pub fn initial_regs(cpu: &mut Cpu) {
    cpu.gpr[0] = 0;
    cpu.gpr[31] = CB_NEXT_FLASH_OFFSET;
    cpu.msr |= MSR_HV;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_visible_through_the_bus() {
        let mut bus = build_bus(vec![], FuseLines::from_text(""));
        assert_eq!(bus.read(0x8000_0200_0006_1000, 8).unwrap(), 0x2000_0000_0000_0000);
    }
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
