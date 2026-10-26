//! TLB de software e janelas de cache travado observadas no CB/CD.

use super::Bus;

impl Bus {
    /// Decode only the page forms observed in CB/CD. Not a full Xenon MMU:
    /// no SLB/VSID matching, protection checks, or replacement-set modelling yet.
    pub fn write_tlb(&mut self, index: u64, rpn: u64, vpn: u64) {
        self.mmu.insert(index, rpn, vpn);
        let (ea, size) = match index & 0xF {
            2 => (((vpn & 0xFF00) << 16), 0x100_0000),
            4 => (((vpn & 0xFF00) << 16) | ((index & 0xFF0) << 12), 0x1_0000),
            // Minimal observed locked-cache span; the actual page-size encoding is unknown.
            8 => ((vpn & 0xFF00) << 16, 0x1_0000),
            _ => return,
        };
        let ra = rpn & 0x0000_FFFF_FFFF_0000;
        // CD's DRAM pages carry a swizzled cache address; route to the backing DRAM.
        let target = if size == 0x1_0000 && ra == ((1 << 40) | (ea << 17) | ea) {
            0x8000_0000_0000_0000 | ea
        } else if ra <= u32::MAX as u64 && ra >= 0xC800_0000 {
            0x8000_0200_0000_0000 | ra
        } else {
            0x8000_0000_0000_0000 | ra
        };
        self.tlb.retain(|(e, s, _)| *e != ea || *s != size);
        self.tlb.push((ea, size, target));
        self.locked_fetch.retain(|(e, s, _)| *e != ea || *s != size);
        if index & 0xF == 4 && rpn & 0xFFF == 0x193
            && ra == ((1 << 40) | (ea << 17) | ea) {
            self.locked_fetch.push((ea, size, target));
        }
    }

    /// Read-only translation snapshot for runner fault diagnostics.
    pub fn tlb_entries(&self) -> &[(u64, u64, u64)] {
        &self.tlb
    }

    pub fn software_tlb_entries(&self) -> &[(u64, u64, u64)] {
        self.mmu.entries()
    }

    /// Last matching software-TLB window. A miss is a real fault: it must not
    /// fall through to HRMOR or a real-mode alias.
    pub(super) fn tlb_lookup(&self, addr: u64, size: usize) -> Option<u64> {
        self.tlb.iter().rev().find(|(b, s, _)| addr >= *b && addr - b < *s
            && size as u64 <= s - (addr - b))
            .map(|(b, _, t)| t + (addr - b))
    }

    /// Locked-cache code is addressed before HRMOR and only for instruction
    /// fetches; data accesses to the same address must still fault.
    pub(super) fn locked_fetch_target(&self, addr: u64) -> Option<u64> {
        self.locked_fetch.iter().rev().find(|(b, s, _)| addr >= *b
            && addr - b < *s && 4 <= s - (addr - b))
            .map(|(b, _, t)| t + (addr - b))
    }
}
