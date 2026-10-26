//! Passeio pela tabela de páginas hasheada (SDR1), no modelo do Xenon/CBE.

use super::Bus;

impl Bus {
    /// Hardware hashed-page-table walk. Mirrors the Xenon/CBE TLB reload:
    /// hash the VSID and page number, search the primary and secondary PTEG
    /// (8 PTEs each) described by SDR1, then install the matching translation
    /// in the software-TLB model and update the R/C bits. Returns whether a
    /// matching valid PTE was found.
    pub(super) fn mmu_hardware_walk(&mut self, ea: u64, write: bool) -> bool {
        if self.mmu.sdr1 == 0 || self.lpcr & 0x400 != 0 {
            return false;
        }
        let ea = if self.mmu.sf { ea } else { ea as u32 as u64 };
        let Some(&(_, rs)) = self.mmu.slb.iter().find(|(rb, _)| {
            rb & 0x0800_0000 != 0 && rb >> 28 == ea >> 28
        }) else {
            return false;
        };
        let large = rs & 0x100 != 0 && self.mmu.large_pages_enabled();
        let lp = rs >> 4 & 7;
        let p = self.mmu.page_shift(large, lp);
        let vsid = (rs >> 12) as u128;
        let va = (vsid << 28) | (ea & 0x0FFF_FFFF) as u128;
        let va64 = va as u64;
        let vpn = (va64 >> p) << p;
        let compare_mask = match p {
            12 => 0xFFFF_FFFF_FFFF_F000u64,
            16 => 0xFFFF_FFFF_FF00_0000u64,
            24 => 0xFFFF_FFFF_0000_0000u64,
            _ => 0xFFFF_FFFF_FFFF_F000u64,
        };
        // Hash0 = (VSID >> 28) ^ (PAGE >> p), masked by SDR1[HTABSIZE].
        let hash0_raw = (vsid as u64) ^ ((ea & 0x0FFF_FFFF) >> p);
        let htab_org = self.mmu.sdr1 & 0x0FFF_FFFF_FFFC_0000;
        let htab_size = self.mmu.sdr1 & 0x1F;
        let htab_mask = (1u64 << (11 + htab_size)) - 1;
        let hash0 = hash0_raw & htab_mask;
        let hash1 = (!hash0) & htab_mask;
        for (hash, want_h) in [(hash0, 0u64), (hash1, 1u64)] {
            let pteg = htab_org | (hash << 7);
            for i in 0..8u64 {
                let pte_addr = pteg + i * 16;
                let pte0 = match self.read_real(pte_addr, 8) { Ok(v) => v, Err(_) => continue };
                let pte1 = match self.read_real(pte_addr + 8, 8) { Ok(v) => v, Err(_) => continue };
                if pte0 & 1 == 0 || (pte0 >> 1) & 1 != want_h {
                    continue;
                }
                let pte_large = pte0 & 4 != 0;
                let pte_lp = (pte1 >> 12) & 1;
                if pte_large != large || (large && pte_lp != lp) {
                    continue;
                }
                // AVPN[0:51] must match the virtual page.
                if ((pte0 & 0x0001_FFFF_FFFF_F000) << 16) != (va64 & 0xFFFF_FFFF_F000_0000) {
                    continue;
                }
                if (vpn & compare_mask) != (va64 & compare_mask) {
                    continue;
                }
                let rpn = if large { pte1 & 0x0000_03FF_FFFF_E000 } else { pte1 & 0x0000_03FF_FFFF_F000 };
                // Reconstruct the software-TLB index/VPN in the same encoding
                // used by the interpreter's mtspr TLB-write path.
                let ti = match p {
                    12 => (((va64 >> 24 ^ va64 >> 16) & 15) << 4) | (va64 >> 12 & 15),
                    16 => (((va64 >> 24 ^ va64 >> 20) & 15) << 4) | (va64 >> 16 & 15),
                    _ => (va64 >> 24) & 0xFF,
                };
                let index = ((va64 & 0x70_0000) << 25) | (ti << 4) | 8;
                let vpn_entry = ((va64 >> 16) & 0x0001_FFFF_FFFF_FF80) | if p == 12 { 1 } else { 5 };
                self.mmu.insert(index, rpn | 0x192, vpn_entry);
                // Update the referenced and changed bits, best effort.
                let mut new_pte1 = pte1 | 0x100;
                if write { new_pte1 |= 0x80; }
                let _ = self.write_real(pte_addr + 8, 8, new_pte1);
                return true;
            }
        }
        false
    }
}
