#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BusError {
    Unmapped { addr: u64, size: usize, write: bool },
}

pub trait Device {
    fn read(&mut self, off: u64, size: usize) -> Result<u64, BusError>;
    fn write(&mut self, off: u64, size: usize, val: u64) -> Result<(), BusError>;
}

enum Kind {
    Ram(Vec<u8>),
    Dev(Box<dyn Device>),
}

struct Region {
    base: u64,
    size: u64,
    kind: Kind,
}

#[derive(Default)]
pub struct Bus {
    regions: Vec<Region>,
    /// (base, size, target): addresses in `base..base+size` are redirected to `target + (addr - base)`.
    aliases: Vec<(u64, u64, u64)>,
}

impl Bus {
    pub fn new() -> Bus {
        Bus::default()
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

    fn xlate(&self, addr: u64) -> u64 {
        match self.aliases.iter().find(|(b, s, _)| addr >= *b && addr - *b < *s) {
            Some((b, _, t)) => t + (addr - b),
            None => addr,
        }
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
        let addr = self.xlate(addr);
        let r = self.find(addr, size, false)?;
        let off = addr - r.base;
        match &mut r.kind {
            Kind::Ram(m) => Ok(m[off as usize..off as usize + size]
                .iter()
                .fold(0u64, |a, b| (a << 8) | *b as u64)),
            Kind::Dev(d) => d.read(off, size),
        }
    }

    pub fn write(&mut self, addr: u64, size: usize, val: u64) -> Result<(), BusError> {
        let addr = self.xlate(addr);
        let r = self.find(addr, size, true)?;
        let off = addr - r.base;
        match &mut r.kind {
            Kind::Ram(m) => {
                for i in 0..size {
                    m[off as usize + i] = (val >> (8 * (size - 1 - i))) as u8;
                }
                Ok(())
            }
            Kind::Dev(d) => d.write(off, size, val),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LastOff(u64);
    impl Device for LastOff {
        fn read(&mut self, off: u64, _size: usize) -> Result<u64, BusError> {
            self.0 = off;
            Ok(self.0)
        }
        fn write(&mut self, off: u64, _size: usize, _val: u64) -> Result<(), BusError> {
            self.0 = off;
            Ok(())
        }
    }

    #[test]
    fn ram_is_big_endian() {
        let mut bus = Bus::new();
        bus.map_ram(0x1000, 0x100);
        bus.write(0x1000, 8, 0x1122334455667788).unwrap();
        assert_eq!(bus.read(0x1000, 4).unwrap(), 0x11223344);
    }

    #[test]
    fn unmapped_read_errors() {
        let mut bus = Bus::new();
        assert_eq!(
            bus.read(0x9000, 4),
            Err(BusError::Unmapped { addr: 0x9000, size: 4, write: false })
        );
    }

    #[test]
    fn access_crossing_region_end_errors() {
        let mut bus = Bus::new();
        bus.map_ram(0x1000, 16);
        assert!(bus.read(0x100C, 8).is_err());
    }

    #[test]
    fn device_gets_offset() {
        let mut bus = Bus::new();
        bus.map_device(0x2000, 0x100, Box::new(LastOff(0)));
        assert_eq!(bus.read(0x2010, 4).unwrap(), 0x10);
    }

    #[test]
    fn address_overflow_errors_instead_of_panicking() {
        let mut b = Bus::new();
        b.map_ram(0, 0x1000);
        assert!(b.read(u64::MAX - 1, 4).is_err());
        assert!(b.write(u64::MAX - 1, 4, 0).is_err());
    }

    #[test]
    fn alias_redirects_to_target() {
        let mut b = Bus::new();
        b.map_ram(0x8000_0200_0000_0000, 0x100);
        b.map_alias(0x0200_0000, 0x100, 0x8000_0200_0000_0000);
        b.write(0x8000_0200_0000_0010, 4, 0xDEADBEEF).unwrap();
        assert_eq!(b.read(0x0200_0010, 4).unwrap(), 0xDEADBEEF);
        assert!(b.read(0x0200_0100, 4).is_err());
    }
}
