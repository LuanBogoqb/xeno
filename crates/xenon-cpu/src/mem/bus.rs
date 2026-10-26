//! Barramento: mapa de regiões, aliases, tradução e acesso a RAM e dispositivos.
//!
//! O restante do módulo `mem` acrescenta as partes que ficavam todas aqui:
//! `tlb` (tradução de software e cache travado), `walk` (passeio pela tabela de
//! páginas), `cipher` (visões cifradas do security engine) e `shared` (reservas
//! entre threads e a janela do IIC).

use crate::mmu::Mmu;

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
    pub(crate) mmu: Mmu,
    regions: Vec<Region>,
    /// (base, size, target): addresses in `base..base+size` are redirected to `target + (addr - base)`.
    aliases: Vec<(u64, u64, u64)>,
    /// Optional plaintext backing for security-engine regions 1 and 3.
    /// Integrity tags, encryption and key-dependent isolation are NOT emulated.
    security_ram: Option<(u64, u64)>,
    hashed_backing: Option<(u64, u64)>,
    /// Reservations belong to hardware threads; writes invalidate every owner.
    pub(super) reservations: [Option<u64>; 6],
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
    pub(super) tlb: Vec<(u64, u64, u64)>,
    /// Experimental locked-cache instruction windows, independent of MSR[IR].
    /// CD marks the CF staging page executable (RPN low bits 0x193) before bctrl.
    pub(super) locked_fetch: Vec<(u64, u64, u64)>,
    pub(crate) translated: bool,
    pub(crate) fetching: bool,
}

impl Bus {
    pub fn new() -> Bus {
        Bus::default()
    }

    pub fn ctrl_value(&self, ppe: usize) -> u64 { self.ctrl[ppe] }

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
            return self.tlb_lookup(addr, size)
                .ok_or(BusError::Unmapped { addr, size, write });
        }
        if let Some(t) = self.alias_of(addr) {
            return Ok(t);
        }
        let addr = if addr >> 63 == 0 { addr | self.hrmor } else { addr };
        self.route_real(addr, size, write)
    }

    pub(super) fn route_real(&self, addr: u64, size: usize, write: bool) -> Result<u64, BusError> {
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

    /// Address translation for the atomic reservations, which need the
    /// physical line before touching memory.
    pub(super) fn translate_atomic(&mut self, addr: u64, size: usize, write: bool) -> Result<u64, BusError> {
        self.xlate(addr, size, write)
    }

    /// Checks that a conditional store lands on mapped memory before it commits.
    pub(super) fn ensure_mapped(&mut self, addr: u64, size: usize, write: bool) -> Result<(), BusError> {
        self.find(addr, size, write).map(|_| ())
    }

    pub fn load(&mut self, addr: u64, bytes: &[u8]) {
        for (i, b) in bytes.iter().enumerate() {
            self.write(addr + i as u64, 1, *b as u64).expect("load outside mapped region");
        }
    }

    pub fn read(&mut self, addr: u64, size: usize) -> Result<u64, BusError> {
        if let Some(result) = self.cipher_read(addr, size) {
            return result;
        }
        let addr = self.xlate(addr, size, false)?;
        if let Some(result) = self.iic_read(addr, size) {
            return result;
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

    pub fn write(&mut self, addr: u64, size: usize, val: u64) -> Result<(), BusError> {
        if let Some(result) = self.cipher_write(addr, size, val) {
            return result;
        }
        let addr = self.xlate(addr, size, true)?;
        self.release_reservations(addr, size);
        if let Some(result) = self.iic_write(addr, size, val) {
            return result;
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

    /// Locked cache code is addressed before HRMOR, like the fixed CB windows.
    /// This is an observed CB/CD cache-window model, not general IR-off TLB fallback.
    pub(crate) fn fetch(&mut self, addr: u64) -> Result<u64, BusError> {
        let physical = if !self.translated {
            self.locked_fetch_target(addr)
        } else {
            None
        };
        self.read(physical.unwrap_or(addr), 4)
    }

    /// Raw physical read used by the page-table walker. `addr` is a real
    /// (32-bit) address; route it through the bus map without translation.
    pub(super) fn read_real(&mut self, addr: u64, size: usize) -> Result<u64, BusError> {
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

    pub(super) fn write_real(&mut self, addr: u64, size: usize, val: u64) -> Result<(), BusError> {
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

    /// Copies a physical block into `out` resolving the region only once.
    /// The security-engine path used to issue one bus read per byte.
    pub(super) fn read_phys_into(&mut self, addr: u64, out: &mut [u8]) -> Result<(), BusError> {
        let size = out.len();
        let target = self.route_real(addr, size, false)?;
        let r = self.find(target, size, false)?;
        let off = (target - r.base) as usize;
        match &mut r.kind {
            Kind::Ram(m) => out.copy_from_slice(&m[off..off + size]),
            Kind::Sparse(pages) => {
                for (i, byte) in out.iter_mut().enumerate() {
                    let o = off + i;
                    *byte = pages.get(&(o as u64 >> 12)).map_or(0, |p| p[o & 4095]);
                }
            }
            Kind::Dev(d) => {
                for (i, byte) in out.iter_mut().enumerate() {
                    *byte = d.read((off + i) as u64, 1)? as u8;
                }
            }
        }
        Ok(())
    }

    /// Writes a physical block with one region lookup, mirroring `read_phys_into`.
    pub(super) fn write_phys_from(&mut self, addr: u64, data: &[u8]) -> Result<(), BusError> {
        let size = data.len();
        let target = self.route_real(addr, size, true)?;
        let r = self.find(target, size, true)?;
        let off = (target - r.base) as usize;
        match &mut r.kind {
            Kind::Ram(m) => m[off..off + size].copy_from_slice(data),
            Kind::Sparse(pages) => {
                for (i, byte) in data.iter().enumerate() {
                    let o = off + i;
                    if *byte != 0 || pages.contains_key(&(o as u64 >> 12)) {
                        pages.entry(o as u64 >> 12).or_insert_with(|| Box::new([0; 4096]))[o & 4095] = *byte;
                    }
                }
            }
            Kind::Dev(d) => {
                for (i, byte) in data.iter().enumerate() {
                    d.write((off + i) as u64, 1, *byte as u64)?;
                }
            }
        }
        Ok(())
    }
}
