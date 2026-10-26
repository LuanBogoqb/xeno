#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BusError {
    Unmapped { addr: u64, size: usize, write: bool },
    Translation { addr: u64, write: bool, segment: bool },
    Protection { addr: u64, write: bool, no_execute: bool },
}

pub trait Device {
    fn read(&mut self, off: u64, size: usize) -> Result<u64, BusError>;
    fn write(&mut self, off: u64, size: usize, val: u64) -> Result<(), BusError>;
}

enum Kind {
    Ram(Vec<u8>),
    Sparse(std::collections::HashMap<u64, Box<[u8; 4096]>>),
    Dev(Box<dyn Device>),
}

struct Region {
    base: u64,
    size: u64,
    kind: Kind,
}

#[derive(Default)]
pub struct Bus {
    pub(crate) mmu: crate::mmu::Mmu,
    regions: Vec<Region>,
    /// (base, size, target): addresses in `base..base+size` are redirected to `target + (addr - base)`.
    aliases: Vec<(u64, u64, u64)>,
    /// Optional plaintext backing for security-engine regions 1 and 3.
    /// Integrity tags, encryption and key-dependent isolation are NOT emulated.
    security_ram: Option<(u64, u64)>,
    hashed_backing: Option<(u64, u64)>,
    /// Reservations belong to hardware threads; writes invalidate every owner.
    reservations: [Option<u64>; 6],
    pub(crate) active_thread: usize,
    pub(crate) multicore: bool,
    pub(crate) ctrl: [u64; 3],
    pub(crate) reset: [bool; 6],
    pub(crate) iic: crate::dev::iic::Iic,
    pub(crate) security: Option<crate::security::SecurityEngine>,
    /// When true, regions 1/3 are decrypted/encrypted on the fly with the security engine.
    pub security_cipher: bool,
    /// Hypervisor real-mode offset (SPR 0x139): addresses with bit 63 clear are ORed with it.
    pub hrmor: u64,
    pub lpcr: u64,
    pub rmor: u64,
    /// Experimental Xenon software-TLB entries: (EA, size, bus address).
    tlb: Vec<(u64, u64, u64)>,
    /// Experimental locked-cache instruction windows, independent of MSR[IR].
    /// CD marks the CF staging page executable (RPN low bits 0x193) before bctrl.
    locked_fetch: Vec<(u64, u64, u64)>,
    pub(crate) translated: bool,
    pub(crate) fetching: bool,
}

impl Bus {
    pub fn ctrl_value(&self, ppe: usize) -> u64 { self.ctrl[ppe] }
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

    pub fn new() -> Bus {
        Bus::default()
    }

    /// Read-only translation snapshot for runner fault diagnostics.
    pub fn tlb_entries(&self) -> &[(u64, u64, u64)] {
        &self.tlb
    }

    pub fn software_tlb_entries(&self) -> &[(u64, u64, u64)] {
        self.mmu.entries()
    }

    fn insert(&mut self, r: Region) {
        let at = self.regions.partition_point(|x| x.base < r.base);
        self.regions.insert(at, r);
    }

    pub fn map_ram(&mut self, base: u64, size: usize) {
        self.insert(Region { base, size: size as u64, kind: Kind::Ram(vec![0; size]) });
    }

    pub fn map_device(&mut self, base: u64, size: u64, dev: Box<dyn Device>) {
        self.insert(Region { base, size, kind: Kind::Dev(dev) });
    }

    /// Real-mode alias, e.g. CB_A jumps to 32-bit 0x0200_xxxx while the SRAM sits at 0x8000_0200_0000_0000.
    pub fn map_alias(&mut self, base: u64, size: u64, target: u64) {
        self.aliases.push((base, size, target));
    }

    /// Opt in only after the validated CD handoff. This is still experimental.
    pub fn enable_software_mmu(&mut self, slb: [(u64, u64); 64]) {
        self.mmu.slb = slb;
        self.mmu.enabled = true;
    }

    pub fn map_security_ram_plaintext(&mut self, target: u64, size: u64) {
        self.security_ram = Some((target, size));
    }

    pub fn enable_security_engine(&mut self, master_key: u64) {
        let mut e = crate::security::SecurityEngine::new(master_key);
        e.enable();
        self.security = Some(e);
    }
    pub fn enable_security_engine_with_keys(&mut self, master_key: u64, white_key: [u8; 16], aes_key: [u8; 16], hash_key: [u8; 16]) {
        let mut e = crate::security::SecurityEngine::with_keys(master_key, white_key, aes_key, hash_key);
        e.enable();
        self.security = Some(e);
    }
    pub fn enable_security_cipher(&mut self) { self.security_cipher = true; }
    /// Fork the live DRAM into a sparse plaintext hashed view before the HV
    /// starts. Physical/encrypted writes must not destroy cached HV plaintext.
    /// This models view isolation only, NOT actual encryption or hash checking.
    pub fn isolate_hashed_ram_plaintext(&mut self, backing: u64) {
        assert!(self.hashed_backing.is_none(), "hashed view already isolated");
        let (source, size) = self.security_ram.expect("security RAM backing");
        let region = self.find(source, size as usize, false).expect("DRAM source");
        let Kind::Ram(bytes) = &region.kind else { panic!("DRAM must be dense RAM") };
        let mut pages = std::collections::HashMap::new();
        for (i, chunk) in bytes.chunks(4096).enumerate() {
            if chunk.iter().any(|v| *v != 0) {
                let mut page = Box::new([0; 4096]);
                page[..chunk.len()].copy_from_slice(chunk);
                pages.insert(i as u64, page);
            }
        }
        self.insert(Region { base: backing, size, kind: Kind::Sparse(pages) });
        self.hashed_backing = Some((backing, size));
    }

    fn alias_of(&self, addr: u64) -> Option<u64> {
        self.aliases.iter().find(|(b, s, _)| addr >= *b && addr - *b < *s).map(|(b, _, t)| t + (addr - b))
    }

    /// Aliases are tried on the address as issued (the locked-cache windows are mapped by SPR, ahead of
    /// HRMOR), then HRMOR is applied to bit-63-clear addresses and the aliases are tried again.
    fn xlate(&mut self, addr: u64, size: usize, write: bool) -> Result<u64, BusError> {
        if self.translated {
            if self.mmu.enabled {
                let fetch = self.fetching;
                // LPCR[TL] = 1 selects software-managed TLB reload (a miss is
                // surfaced to the guest handler).  LPCR[TL] = 0 walks the
                // hashed page table in hardware.
                let software_managed = self.lpcr & 0x400 != 0;
                let result = self.mmu.translate_access(addr, size, write, fetch);
                return match result {
                    Ok(real) => self.route_real((1 << 63) | real, size, write),
                    Err(e) => {
                        if let BusError::Translation { segment: false, .. } = e {
                            if !software_managed && self.mmu_hardware_walk(addr, write) {
                                let real = self.mmu.translate_access(addr, size, write, fetch)?;
                                return self.route_real((1 << 63) | real, size, write);
                            }
                        }
                        Err(e)
                    }
                };
            }
            // A miss must not silently fall through to HRMOR/real-mode aliases.
            return self.tlb.iter().rev().find(|(b, s, _)| addr >= *b && addr - b < *s
                && size as u64 <= s - (addr - b))
                .map(|(b, _, t)| t + (addr - b))
                .ok_or(BusError::Unmapped { addr, size, write });
        }
        if let Some(t) = self.alias_of(addr) {
            return Ok(t);
        }
        let addr = if addr >> 63 == 0 { addr | self.hrmor } else { addr };
        self.route_real(addr, size, write)
    }

    /// Hardware hashed-page-table walk. Mirrors the Xenon/CBE TLB reload:
    /// hash the VSID and page number, search the primary and secondary PTEG
    /// (8 PTEs each) described by SDR1, then install the matching translation
    /// in the software-TLB model and update the R/C bits. Returns whether a
    /// matching valid PTE was found.
    fn mmu_hardware_walk(&mut self, ea: u64, write: bool) -> bool {
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

    /// Raw physical read used by the page-table walker. `addr` is a real
    /// (32-bit) address; route it through the bus map without translation.
    fn read_real(&mut self, addr: u64, size: usize) -> Result<u64, BusError> {
        let target = self.route_real(addr, size, false)?;
        let r = self.find(target, size, false)?;
        let off = target - r.base;
        match &mut r.kind {
            Kind::Ram(m) => Ok(m[off as usize..off as usize + size]
                .iter().fold(0u64, |a, b| (a << 8) | *b as u64)),
            Kind::Sparse(pages) => Ok((0..size as u64).fold(0, |value, i| {
                let off = off + i;
                (value << 8) | pages.get(&(off >> 12)).map_or(0, |p| p[(off & 4095) as usize] as u64)
            })),
            Kind::Dev(d) => d.read(off, size),
        }
    }

    fn write_real(&mut self, addr: u64, size: usize, val: u64) -> Result<(), BusError> {
        let target = self.route_real(addr, size, true)?;
        let r = self.find(target, size, true)?;
        let off = target - r.base;
        match &mut r.kind {
            Kind::Ram(m) => {
                for i in 0..size {
                    m[off as usize + i] = (val >> (8 * (size - 1 - i))) as u8;
                }
                Ok(())
            }
            Kind::Sparse(pages) => {
                for i in 0..size {
                    let offset = off + i as u64;
                    let value = (val >> (8 * (size - 1 - i))) as u8;
                    if value != 0 || pages.contains_key(&(offset >> 12)) {
                        pages.entry(offset >> 12).or_insert_with(|| Box::new([0; 4096]))[(offset & 4095) as usize] = value;
                    }
                }
                Ok(())
            }
            Kind::Dev(d) => d.write(off, size, val),
        }
    }

    fn route_real(&self, addr: u64, size: usize, write: bool) -> Result<u64, BusError> {
        if let Some(target) = self.alias_of(addr) {
            return Ok(target);
        }
        let real = addr & !(1 << 63);
        if matches!(real >> 40, 1 | 3) {
            let backing = if real >> 40 == 1 { self.hashed_backing.or(self.security_ram) } else { self.security_ram };
            if let Some((base, len)) = backing {
                let offset = real & 0x3FFF_FFFF;
                return if offset < len && size as u64 <= len - offset {
                    Ok(base + offset)
                } else {
                    Err(BusError::Unmapped { addr, size, write })
                };
            }
        }
        // General MMU real addresses have no bit-63 marker. The existing bus
        // map uses that marker to distinguish physical DRAM from effective EA.
        if self.mmu.enabled && (addr & !(1 << 63)) >> 40 == 0 {
            return Ok((1 << 63) | (addr & 0xFFFF_FFFF));
        }
        Ok(addr)
    }

    fn find(&mut self, addr: u64, size: usize, write: bool) -> Result<&mut Region, BusError> {
        let err = BusError::Unmapped { addr, size, write };
        let i = self.regions.partition_point(|r| r.base <= addr);
        let r = self.regions.get_mut(i.checked_sub(1).ok_or(err)?).ok_or(err)?;
        match addr.checked_add(size as u64) {
            Some(end) if end <= r.base + r.size => Ok(r),
            _ => Err(err),
        }
    }

    pub fn load(&mut self, addr: u64, bytes: &[u8]) {
        for (i, b) in bytes.iter().enumerate() {
            self.write(addr + i as u64, 1, *b as u64).expect("load outside mapped region");
        }
    }

    pub fn read(&mut self, addr: u64, size: usize) -> Result<u64, BusError> {
        // Security-engine cipher path: regions 1/3 are decrypted views of the
        // physical DRAM (region 0). When enabled, reads transform on the fly.
        let region = addr >> 40;
        if (region == 1 || region == 3) && self.security_cipher && self.security.is_some() {
            let off = (addr & 0x3FFF_FFFF) as usize;
            let mut out = vec![0u8; size];
            let mut i = 0;
            while i < size {
                let cur = off + i;
                let block_off = cur & !0xF;
                let in_block = cur & 0xF;
                let take = (16 - in_block).min(size - i);
                // read 16-byte block ciphertext
                let mut ct = [0u8; 16];
                for j in 0..16 {
                    let phys = 0x8000_0000_0000_0000u64 + (block_off + j) as u64;
                    ct[j] = self.read(phys, 1).unwrap_or(0) as u8;
                }
                let eng = self.security.as_ref().unwrap();
                let pt = eng.xex_decrypt_block(&ct, block_off as u64);
                for j in 0..take {
                    out[i + j] = pt[in_block + j];
                }
                i += take;
            }
            let mut result = 0u64;
            for b in &out { result = (result << 8) | *b as u64; }
            return Ok(result);
        }
        // A true TLB miss (no entry) must fault, even when translated.
        let addr = match self.xlate(addr, size, false) {
            Ok(a) => a,
            Err(e) => return Err(e),
        };
        if self.multicore && (0x8000_0200_0005_0000..0x8000_0200_0005_8000).contains(&addr) {
            if !matches!(size, 1 | 2 | 4 | 8) || (addr & 7) as usize + size > 8 {
                return Err(BusError::Unmapped { addr, size, write: false });
            }
            return Ok(self.iic.read(addr - 0x8000_0200_0005_0000, size));
        }
        let r = self.find(addr, size, false)?;
        let off = addr - r.base;
        match &mut r.kind {
            Kind::Ram(m) => Ok(m[off as usize..off as usize + size]
                .iter()
                .fold(0u64, |a, b| (a << 8) | *b as u64)),
            Kind::Sparse(pages) => Ok((0..size as u64).fold(0, |value, i| {
                let off = off + i;
                (value << 8) | pages.get(&(off >> 12)).map_or(0, |p| p[(off & 4095) as usize] as u64)
            })),
            Kind::Dev(d) => d.read(off, size),
        }
    }

    pub(crate) fn load_reserve(&mut self, addr: u64, size: usize) -> Result<u64, BusError> {
        self.reservations[self.active_thread] = None;
        let physical = self.xlate(addr, size, false)?;
        let value = self.read(addr, size)?;
        self.reservations[self.active_thread] = Some(physical & !127);
        Ok(value)
    }

    pub(crate) fn store_conditional(&mut self, addr: u64, size: usize, val: u64) -> Result<bool, BusError> {
        let reservation = self.reservations[self.active_thread].take();
        let physical = self.xlate(addr, size, true)?;
        self.find(physical, size, true)?;
        if reservation != Some(physical & !127) {
            return Ok(false);
        }
        self.write(addr, size, val)?;
        Ok(true)
    }

    /// Locked cache code is addressed before HRMOR, like the fixed CB windows.
    /// This is an observed CB/CD cache-window model, not general IR-off TLB fallback.
    pub(crate) fn fetch(&mut self, addr: u64) -> Result<u64, BusError> {
        let physical = if !self.translated {
            self.locked_fetch.iter().rev().find(|(b, s, _)| addr >= *b
                && addr - b < *s && 4 <= s - (addr - b))
                .map(|(b, _, t)| t + (addr - b))
        } else { None };
        self.read(physical.unwrap_or(addr), 4)
    }

    pub fn write(&mut self, addr: u64, size: usize, val: u64) -> Result<(), BusError> {
        let region = addr >> 40;
        if (region == 1 || region == 3) && self.security_cipher && self.security.is_some() {
            let off = (addr & 0x3FFF_FFFF) as usize;
            // Convert val to bytes big-endian.
            let mut val_bytes = vec![0u8; size];
            for i in 0..size {
                val_bytes[i] = (val >> (8 * (size - 1 - i))) as u8;
            }
            // Group by 16-byte blocks.
            let mut i = 0;
            while i < size {
                let cur = off + i;
                let block_off = cur & !0xF;
                let in_block = cur & 0xF;
                let take = (16 - in_block).min(size - i);
                // read existing ciphertext block
                let mut ct = [0u8; 16];
                for j in 0..16 {
                    let phys = 0x8000_0000_0000_0000u64 + (block_off + j) as u64;
                    ct[j] = self.read(phys, 1).unwrap_or(0) as u8;
                }
                // decrypt to plaintext, apply, encrypt back (scoped so self can be borrowed mutably afterwards)
                let new_ct = {
                    let eng = self.security.as_ref().unwrap();
                    let mut pt = eng.xex_decrypt_block(&ct, block_off as u64);
                    for j in 0..take {
                        pt[in_block + j] = val_bytes[i + j];
                    }
                    eng.xex_encrypt_block(&pt, block_off as u64)
                };
                // write ciphertext
                for j in 0..16 {
                    let phys = 0x8000_0000_0000_0000u64 + (block_off + j) as u64;
                    self.write(phys, 1, new_ct[j] as u64)?;
                }
                i += take;
            }
            return Ok(());
        }
        let addr = match self.xlate(addr, size, true) {
            Ok(a) => a,
            Err(e) => return Err(e),
        };
        for reservation in &mut self.reservations {
            if let Some(line) = *reservation {
                if addr < line.saturating_add(128) && addr.saturating_add(size as u64) > line {
                    *reservation = None;
                }
            }
        }
        if self.multicore && (0x8000_0200_0005_0000..0x8000_0200_0005_8000).contains(&addr) {
            if !matches!(size, 1 | 2 | 4 | 8) || (addr & 7) as usize + size > 8 {
                return Err(BusError::Unmapped { addr, size, write: true });
            }
            self.iic.write(addr - 0x8000_0200_0005_0000, size, val);
            return Ok(());
        }
        let r = self.find(addr, size, true)?;
        let off = addr - r.base;
        match &mut r.kind {
            Kind::Ram(m) => {
                for i in 0..size {
                    m[off as usize + i] = (val >> (8 * (size - 1 - i))) as u8;
                }
                Ok(())
            }
            Kind::Sparse(pages) => {
                for i in 0..size {
                    let offset = off + i as u64;
                    let value = (val >> (8 * (size - 1 - i))) as u8;
                    if value != 0 || pages.contains_key(&(offset >> 12)) {
                        pages.entry(offset >> 12).or_insert_with(|| Box::new([0; 4096]))[(offset & 4095) as usize] = value;
                    }
                }
                Ok(())
            }
            Kind::Dev(d) => d.write(off, size, val),
        }
    }
}

#[cfg(test)]
mod tests;
