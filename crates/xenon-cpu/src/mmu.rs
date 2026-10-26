//! Experimental software-managed Xenon SLB/TLB. No hashed-page-table walker
//! or complete protection yet (base PP/key and NX checks only). Kept opt-in to isolate the validated CD
//! cache-window model. Register layout reference: IBM CBE handbook and
//! xenon-emu's PPC_MMU.cpp / PPC_System.cpp (numeric field definitions only).
use crate::mem::BusError;

/// HID6 (SPR 0x3F9 on the Cell/Xenon PPE; SPR 0x3F6 is HID5). Field
/// definitions follow the Linux kernel's `arch/powerpc/include/asm/reg.h`
/// (`SPRN_HID6`, `HID6_LB`, `HID6_DLP`), which encodes the CBE register.
/// The boot chain programs 0x1003800000000 (DLP and LB clear), so large
/// pages stay enabled; the remaining set bits (35..37, 48) are not
/// page-size controls and are kept in `hid6` as stored.
pub(crate) struct Mmu {
    pub enabled: bool,
    pub sf: bool,
    pub problem: bool,
    pub slb: [(u64, u64); 64],
    /// SPR 0x3F9 (HID6) as written by the guest. Only the fields that change
    /// translation are interpreted: DLP disables large pages, LB selects the
    /// concurrent large-page modes (not modelled, see `page_shift`).
    pub hid6: u64,
    /// SPR 0x19 (SDR1): HTABORG in bits 22..43 and HTABSIZE in bits 59..63.
    /// Used by the hardware page-table walker.
    pub sdr1: u64,
    entries: Vec<(u64, u64, u64)>, // index, RPN, VPN
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(m: &mut Mmu, ea: u64, rs: u64, shift: u32, physical: u64, way: u64) {
        m.slb[0] = ((ea & !0x0FFF_FFFF) | 0x0800_0000, rs);
        let va = (rs >> 12 << 28) | (ea & 0x0FFF_FFFF);
        let ti = match shift {
            12 => (((va >> 24 ^ va >> 16) & 15) << 4) | (va >> 12 & 15),
            16 => (((va >> 24 ^ va >> 20) & 15) << 4) | (va >> 16 & 15),
            _ => va >> 24 & 255,
        };
        let index = ((va & 0x70_0000) << 25) | ti << 4 | way;
        let vpn = (va >> 16 & 0x0001_FFFF_FFFF_FF80) | if shift == 12 { 1 } else { 5 };
        m.insert(index, physical | 0x192, vpn);
    }

    #[test]
    fn pp_table_checks_supervisor_and_problem_keys() {
        for problem in [false, true] {
            for key in [false, true] {
                for pp in 0..4 {
                    let mut m = Mmu::default();
                    m.problem = problem;
                    let rs = 0x8000 | if key { if problem { 0x400 } else { 0x800 } } else { 0 };
                    entry(&mut m, 0x80078000, rs, 12, 0x30000078000, 8);
                    m.entries[0].1 = (m.entries[0].1 & !3) | pp;
                    assert_eq!(m.translate(0x80078000, 4, false).is_ok(), !key || pp != 0);
                    assert_eq!(m.translate(0x80078000, 4, true).is_ok(), if key { pp == 2 } else { pp != 3 });
                }
            }
        }
    }

    #[test]
    fn invalidate_addr_removes_only_the_matching_page() {
        let mut m = Mmu::default();
        entry(&mut m, 0x80078000, 0x8000, 12, 0x30000078000, 8);
        assert_eq!(m.translate(0x80078800, 4, false), Ok(0x30000078800));
        // Same 4 KiB page, different offset: the entry must be dropped.
        m.invalidate_addr(0x80078800);
        assert!(m.entries().is_empty());
        assert!(m.translate(0x80078800, 4, false).is_err());
        // Unknown segment falls back to a full clear.
        entry(&mut m, 0x80078000, 0x8000, 12, 0x30000078000, 8);
        m.invalidate_addr(0x90000000);
        assert!(m.entries().is_empty());
    }

    #[test]
    fn segment_and_page_nx_disallow_only_instruction_fetch() {
        for segment in [false, true] {
            let mut m = Mmu::default();
            entry(&mut m, 0x80078000, 0x8000, 12, 0x30000078000, 8);
            if segment { m.slb[0].1 |= 0x200; } else { m.entries[0].1 |= 4; }
            assert!(m.translate(0x80078000, 4, false).is_ok());
            assert!(matches!(m.translate_access(0x80078000, 4, false, true),
                Err(BusError::Protection { no_execute: true, .. })));
        }
    }

    #[test]
    fn hid6_dlp_forces_4k_translation() {
        // A large-page segment (L=1, LP=0) with only a 4 KiB entry present:
        // DLP clear -> 64 KiB lookup, no match; DLP set -> 4 KiB lookup, match.
        let mut m = Mmu::default();
        m.sf = true;
        entry(&mut m, 0x803C_0000, 0x8100, 12, 0x100_003C_0000, 8);
        assert!(m.translate(0x803C_0000, 4, false).is_err());
        m.hid6 = Mmu::HID6_DLP;
        assert!(!m.large_pages_enabled());
        assert_eq!(m.translate(0x803C_0000, 4, false), Ok(0x100_003C_0000));
        // LP is ignored as well: a 16 MiB segment falls back to 4 KiB.
        m.entries.clear();
        entry(&mut m, 0x8A00_0000, 0x8110, 12, 0x100_0A00_0000, 8);
        assert_eq!(m.translate(0x8A00_0000, 4, false), Ok(0x100_0A00_0000));
    }

    #[test]
    fn software_tlb_translates_4k_64k_and_16m_pages() {
        for (ea, rs, shift, physical, offset) in [
            (0x8007_8000, 0x8000, 12, 0x300_0007_8000, 0xDB0),
            (0x803C_0000, 0x8100, 16, 0x100_003C_0000, 0x1234),
            (0x8A00_0000, 0x8110, 24, 0x100_0A00_0000, 0x123456),
        ] {
            let mut m = Mmu::default();
            m.sf = true;
            entry(&mut m, ea, rs, shift, physical | if shift == 24 { 0x1000 } else { 0 }, 8);
            assert_eq!(m.translate(ea + offset, 4, false), Ok(physical + offset), "shift {shift}");
            assert!(m.translate(ea + (1 << shift), 4, false).is_err());
            assert!(m.translate(ea + (1 << shift) - 1, 2, false).is_err());
        }
    }

    #[test]
    fn vsid_and_validity_are_required_and_32_bit_eas_are_truncated() {
        let mut m = Mmu::default();
        entry(&mut m, 0x8007_8000, 0x8000, 12, 0x300_0007_8000, 8);
        assert_eq!(m.translate(0xFFFF_FFFF_8007_8DB0, 4, false), Ok(0x300_0007_8DB0));
        m.sf = true;
        assert!(matches!(m.translate(0xFFFF_FFFF_8007_8DB0, 4, false), Err(BusError::Translation { segment: true, .. })));
        m.slb[0].1 = 0x9000;
        assert!(matches!(m.translate(0x8007_8DB0, 4, true), Err(BusError::Translation { segment: false, write: true, .. })));
        m.slb[0].0 &= !0x0800_0000;
        assert!(matches!(m.translate(0x8007_8DB0, 4, false), Err(BusError::Translation { segment: true, .. })));
    }
}

impl Default for Mmu {
    fn default() -> Self {
        Self { enabled: false, sf: false, problem: false, slb: [(0, 0); 64], hid6: 0, sdr1: 0, entries: Vec::new() }
    }
}

impl Mmu {
    pub fn entries(&self) -> &[(u64, u64, u64)] { &self.entries }
    #[allow(dead_code)]
    pub fn invalidate_all(&mut self) { self.entries.clear(); }
    /// Invalidate only the software-TLB entries that map the virtual page
    /// containing `addr`.  This mirrors tlbie/tlbiel semantics closely enough
    /// for the kernel's page-table updates while leaving unrelated entries
    /// resident.  Falls back to clearing everything if the segment is unknown.
    pub fn invalidate_addr(&mut self, addr: u64) {
        let addr = if self.sf { addr } else { addr as u32 as u64 };
        let Some((_, rs)) = self.slb.iter().find(|(rb, _)| {
            rb & 0x0800_0000 != 0 && rb >> 28 == addr >> 28
        }) else {
            self.entries.clear();
            return;
        };
        let rs = *rs;
        let va = ((rs as u128 >> 12) << 28) | (addr & 0x0FFF_FFFF) as u128;
        let large = rs & 0x100 != 0 && self.large_pages_enabled();
        let lp = rs >> 4 & 7;
        let shift = self.page_shift(large, lp);
        let offset_mask = (1u64 << shift) - 1;
        let va_page = va & !(offset_mask as u128);
        self.entries.retain(|(index, rpn, vpn)| {
            if vpn & 1 == 0 || (vpn & 4 != 0) != large
                || (large && (rpn >> 12 & 1) != lp) {
                return true;
            }
            let ti = index >> 4 & 0xFF;
            let mut page = ((vpn & 0x0001_FFFF_FFFF_FF80) as u128) << 16
                | ((index & 0xE000_0000_0000) >> 25) as u128;
            if shift == 12 {
                page |= ((((ti >> 4) ^ ((page >> 24) as u64 & 15)) & 15) as u128) << 16;
                page |= ((ti & 15) as u128) << 12;
            } else if shift == 16 {
                page |= ((ti & 15) as u128) << 16;
            }
            (page & !(offset_mask as u128)) != va_page
        });
    }
    #[allow(dead_code)]
    pub fn invalidate_slb(&mut self, index: usize) { if index < self.slb.len() { self.slb[index] = (0, 0); } }
    pub fn insert(&mut self, index: u64, rpn: u64, vpn: u64) {
        self.entries.retain(|(i, _, _)| i & 0xFFF != index & 0xFFF);
        self.entries.push((index, rpn, vpn));
    }

    /// HID6[DLP] (bit 20): disable all large page modes, leaving 4 KiB only.
    pub(crate) const HID6_DLP: u64 = 1 << 20;

    /// HID6[DLP] clear means the SLB L/LP fields select a large page size.
    /// HID6[LB] (bits 12..15) selects which large sizes are available
    /// concurrently; the boot chain leaves it clear, so the LP field alone
    /// picks the size and LB is documented but not modelled.
    pub(crate) fn large_pages_enabled(&self) -> bool {
        self.hid6 & Self::HID6_DLP == 0
    }

    pub(crate) fn page_shift(&self, large: bool, lp: u64) -> u32 {
        if !large || !self.large_pages_enabled() { return 12; }
        // Observed boot configuration: LP=0 selects 64 KiB, LP=1 16 MiB.
        // HID6[LB] (which sizes coexist) is not modelled; with LB clear the
        // LP field alone selects the size, matching the validated boot chain.
        if lp == 0 { 16 } else { 24 }
    }

    #[cfg(test)]
    pub fn translate(&self, addr: u64, size: usize, write: bool) -> Result<u64, BusError> {
        self.translate_access(addr, size, write, false)
    }

    pub fn translate_access(&self, addr: u64, size: usize, write: bool, fetch: bool) -> Result<u64, BusError> {
        let fault = |segment| BusError::Translation { addr, write, segment };
        let addr = if self.sf { addr } else { addr as u32 as u64 };
        let (_, rs) = self.slb.iter().find(|(rb, _)| rb & 0x0800_0000 != 0
            && rb >> 28 == addr >> 28).ok_or_else(|| fault(true))?;
        if fetch && rs & 0x200 != 0 {
            return Err(BusError::Protection { addr, write, no_execute: true });
        }
        let va = ((*rs as u128 >> 12) << 28) | (addr & 0x0FFF_FFFF) as u128;
        let large = rs & 0x100 != 0 && self.large_pages_enabled();
        let lp = rs >> 4 & 7;
        let shift = self.page_shift(large, lp);
        let offset_mask = (1u64 << shift) - 1;
        let va_page = va & !(offset_mask as u128);
        for (index, rpn, vpn) in self.entries.iter().rev() {
            if vpn & 1 == 0 || (vpn & 4 != 0) != large
                || (large && (rpn >> 12 & 1) != lp) { continue; }
            let ti = index >> 4 & 0xFF;
            let mut page = ((vpn & 0x0001_FFFF_FFFF_FF80) as u128) << 16
                | ((index & 0xE000_0000_0000) >> 25) as u128;
            if shift == 12 {
                page |= ((((ti >> 4) ^ ((page >> 24) as u64 & 15)) & 15) as u128) << 16;
                page |= ((ti & 15) as u128) << 12;
            } else if shift == 16 {
                page |= ((ti & 15) as u128) << 16;
            }
            if page & !(offset_mask as u128) != va_page { continue; }
            if fetch && rpn & 4 != 0 {
                return Err(BusError::Protection { addr, write, no_execute: true });
            }
            let key = rs & (if self.problem { 0x400 } else { 0x800 }) != 0;
            // PowerPC HPTE protection table. PP0 is bit 3 of the stored RPN
            // word (extended encodings); PP is bits 0..1. NX (no-execute) is
            // bit 2 and handled separately above.
            let pp0 = (rpn >> 3) & 1;
            let pp = rpn & 3;
            let (readable, writable) = if pp0 == 0 {
                // Base PP0=0 table (matches prior behaviour).
                let readable = !key || pp != 0;
                let writable = if key { pp == 2 } else { pp != 3 };
                (readable, writable)
            } else {
                // PP0=1 extended table.
                if !key {
                    // Supervisor: 00=R/W, 01=Read, 10=R/X, 11=Read.
                    (true, pp == 0)
                } else {
                    // Problem: 00=R/X, 01=Read, 10=Read, 11=Read.
                    (true, false)
                }
            };
            if !readable || (write && !writable) || rpn >> 63 != 0 {
                return Err(BusError::Protection { addr, write, no_execute: false });
            }
            let offset = addr & offset_mask;
            if size as u64 > offset_mask + 1 - offset { return Err(fault(false)); }
            let physical = (rpn & 0x0000_03FF_FFFF_F000 & !offset_mask) | offset;
            return Ok(physical);
        }
        Err(fault(false))
    }
}
