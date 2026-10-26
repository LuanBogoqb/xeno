use super::*;

#[test]
    fn hardware_page_walker_installs_hashed_pte() {
        let mut b = Bus::new();
        let dram = 0x8000_0000_0000_0000u64;
        b.map_ram(dram, 0x80000);
        // Craft a primary PTEG at hash 0x70 (VSID=8, page 0x78, p=4K).
        let pte0 = 0x8001u64; // V=1, H=0, L=0, AVPN=0x8000
        let pte1 = 0x8192u64; // RPN=0x8000, PP=2, flags
        b.write(dram + 0x43800, 8, pte0).unwrap();
        b.write(dram + 0x43808, 8, pte1).unwrap();
        // Data that the translated read must return.
        b.write(dram + 0x8800, 4, 0xDEAD_BEEF).unwrap();

        b.mmu.enabled = true;
        b.mmu.sf = true;
        b.mmu.slb[0] = (0x8800_0000, 0x8000);
        b.mmu.sdr1 = 0x40000; // HTABORG=0x40000, HTABSIZE=0
        b.lpcr = 0; // LPCR[TL]=0 -> hardware page-table walk
        b.translated = true;

        // No TLB entry yet: the walk must find the PTE and install it.
        assert_eq!(b.read(0x8007_8800, 4), Ok(0xDEAD_BEEF));
        assert_eq!(b.software_tlb_entries().len(), 1);
        // Referenced bit is set after the successful walk.
        assert_eq!(b.read_real(0x43808, 8).unwrap() & 0x100, 0x100);
    }

    #[test]
    fn hardware_page_walker_honours_sdr1_secondary_pteg() {
        let mut b = Bus::new();
        let dram = 0x8000_0000_0000_0000u64;
        b.map_ram(dram, 0x80000);
        // Put the PTE in the secondary PTEG (H=1): hash1 = ~hash0.
        let hash0 = 0x70u64;
        let hash1 = (!hash0) & 0x7FF;
        let pteg1 = 0x40000 | (hash1 << 7);
        b.write(dram + pteg1, 8, 0x8003).unwrap(); // V=1, H=1, AVPN=0x8000
        b.write(dram + pteg1 + 8, 8, 0x8192).unwrap();
        b.write(dram + 0x8800, 4, 0x1234_5678).unwrap();

        b.mmu.enabled = true;
        b.mmu.sf = true;
        b.mmu.slb[0] = (0x8800_0000, 0x8000);
        b.mmu.sdr1 = 0x40000;
        b.lpcr = 0;
        b.translated = true;

        assert_eq!(b.read(0x8007_8800, 4), Ok(0x1234_5678));
        assert_eq!(b.software_tlb_entries().len(), 1);
    }

    #[test]
    fn iic_accesses_do_not_fall_back_on_invalid_register_widths() {
        let mut b = Bus::new();
        b.multicore = true;
        let base = 0x8000_0200_0005_0000;
        b.write(base, 8, 1).unwrap();
        assert_eq!(b.read(base, 8), Ok(1));
        assert!(b.read(base + 7, 2).is_err());
        assert!(b.write(base + 7, 2, 0).is_err());
        assert_eq!(b.read(base, 8), Ok(1));
        b.write(base + 0x10, 8, 0x10078).unwrap();
        assert!(b.iic.pending(0));
        assert_eq!(b.read(base + 0x50, 8), Ok(0x78));
        assert!(!b.iic.pending(0));
    }

    #[test]
    fn isolated_hashed_view_keeps_hv_plaintext_when_kernel_zeroes_physical_pages() {
        let mut b = Bus::new();
        let raw = 0x8000_0000_0000_0000;
        b.map_ram(raw, 0x4000);
        b.map_security_ram_plaintext(raw, 0x4000);
        b.write(raw + 0x100, 4, 0x12345678).unwrap();
        b.write(raw + 0x1FFF, 8, 0x1122334455667788).unwrap();
        b.isolate_hashed_ram_plaintext(0x9000_0000_0000_0000);
        assert_eq!(b.read(0x100_0000_0100, 4), Ok(0x12345678));
        assert_eq!(b.read(0x100_0000_1FFF, 8), Ok(0x1122334455667788));
        b.write(raw + 0x100, 4, 0).unwrap();
        b.write(0x300_0000_1FFF, 8, 0).unwrap();
        assert_eq!(b.read(0x100_0000_0100, 4), Ok(0x12345678));
        assert_eq!(b.read(0x100_0000_1FFF, 8), Ok(0x1122334455667788));
        b.write(0x168_0000_2FFF, 8, 0xAABBCCDDEEFF0011).unwrap();
        assert_eq!(b.read(0x100_0000_2FFF, 8), Ok(0xAABBCCDDEEFF0011));
        assert_eq!(b.read(raw + 0x2FFF, 8), Ok(0));
        assert_eq!(b.read(0x300_0000_2FFF, 8), Ok(0));
        assert!(b.write(0x100_0000_4000, 1, 1).is_err());
    }

    #[test]
    fn security_plaintext_routing_is_opt_in_and_bounded() {
        let mut b = Bus::new();
        b.map_ram(0x8000_0000_0000_0000, 0x1000);
        assert!(b.read(0x100_0000_0010, 8).is_err());
        assert!(b.read(0x300_0000_0010, 8).is_err());
        b.map_security_ram_plaintext(0x8000_0000_0000_0000, 0x1000);
        b.write(0x8000_0368_0000_0010, 8, 0x1234).unwrap();
        assert_eq!(b.read(0x162_0000_0010, 8), Ok(0x1234));
        assert_eq!(b.read(0x100_C000_0010, 8), Ok(0x1234)); // low 30 address bits
        assert!(b.read(0x100_0000_0FFF, 2).is_err());
        assert!(b.write(0x300_0000_1000, 4, 0).is_err());
        assert!(b.read(0x200_0000_0010, 8).is_err());
        assert!(b.read(0x500_0000_0010, 8).is_err());
        b.translated = true;
        assert!(b.read(0x100_0000_0010, 8).is_err()); // no TLB fallback
    }

    #[test]
    fn reservations_use_physical_aliases_and_stores_invalidate_the_line() {
        let mut b = Bus::new();
        b.map_ram(0x1000, 0x1000);
        b.map_alias(0x3000, 0x1000, 0x1000);
        b.load_reserve(0x3000, 4).unwrap();
        assert_eq!(b.store_conditional(0x1000, 4, 1), Ok(true));
        assert_eq!(b.store_conditional(0x1000, 4, 2), Ok(false));
        assert_eq!(b.read(0x1000, 4), Ok(1));
        b.load_reserve(0x1000, 4).unwrap();
        b.write(0x307F, 1, 1).unwrap();
        assert_eq!(b.store_conditional(0x1000, 4, 2), Ok(false));
        b.load_reserve(0x1000, 8).unwrap();
        b.write(0x3080, 1, 1).unwrap();
        assert_eq!(b.store_conditional(0x3000, 8, 3), Ok(true));
        b.load_reserve(0x1000, 4).unwrap();
        assert_eq!(b.store_conditional(0x1080, 4, 4), Ok(false));
        assert_eq!(b.store_conditional(0x1000, 4, 4), Ok(false));
        assert!(b.load_reserve(0x5000, 4).is_err());
        assert!(b.store_conditional(0x5000, 4, 0).is_err());
    }

    #[test]
    fn executable_locked_page_fetches_before_hrmor_without_data_fallback() {
        let mut b = Bus::new();
        b.map_ram(0x8000_0000_003C_0000, 0x10000);
        b.write(0x8000_0000_003C_0360, 4, 0x3860_0001).unwrap();
        b.hrmor = 0x017A_0300_0000;
        b.write_tlb(0x6000_0000_03C4, 0x0178_003C_0193, 5);
        assert_eq!(b.fetch(0x3C_0360), Ok(0x3860_0001));
        assert!(b.read(0x3C_0360, 4).is_err());
        assert!(b.fetch(0x3D_0360).is_err());
        b.write_tlb(0x6000_0000_03C4, 0x0178_003C_0196, 5);
        assert!(b.fetch(0x3C_0360).is_err());
    }

    #[test]
    fn translated_reads_keep_real_fuses_and_kernel_header_bytes() {
        let mut b = Bus::new();
        b.map_ram(0x8000_0000_0000_0000, 0x10000);
        b.map_device(0x8000_0200_0002_0000, 0x2000, Box::new(
            crate::dev::fuses::FuseLines::from_text("fuseset 03: 0123456789abcdef")));
        b.write(0x8000_0000_0000_0014, 4, 0x1234_5678).unwrap();
        b.write_tlb(4, 0x0100_0000_0196, 5);
        b.write_tlb(0x102, 0x0200_0000_11BE, 0x1005);
        b.translated = true;
        assert_eq!(b.read(0x14, 1), Ok(0x12));
        assert_eq!(b.read(0x14, 4), Ok(0x1234_5678));
        assert_eq!(b.read(0x1002_0600, 8), Ok(0x0123_4567_89AB_CDEF));
    }

    #[test]
    fn experimental_cd_tlb_routes_sram_flash_and_dram() {
        let mut b = Bus::new();
    b.hrmor = 0x017A_0300_0000;
    b.map_ram(0x8000_0200_0000_0000, 0x10000);
    b.map_ram(0x8000_0200_C800_0000, 0x10000);
    b.map_ram(0x8000_0000_0028_0000, 0x10000);
    b.write_tlb(0x102, 0x0200_0000_11BE, 0x1005);
    b.write_tlb(0x182, 0xC800_11BF, 0x1805);
    b.write_tlb(0x4000_0000_0284, 0x0150_0028_0196, 5);
    b.translated = true;
    for (ea, value) in [(0x1000_00FC, 1), (0x1800_0020, 2), (0x28_0030, 3)] {
        b.write(ea, 4, value).unwrap();
        assert_eq!(b.read(ea, 4), Ok(value));
    }
    b.translated = false;
    assert_eq!(b.read(0x8000_0200_0000_00FC, 4), Ok(1));
    assert_eq!(b.read(0x8000_0200_C800_0020, 4), Ok(2));
    assert_eq!(b.read(0x8000_0000_0028_0030, 4), Ok(3));
}

#[test]
fn translated_misses_do_not_fall_back_and_page_crossings_fail() {
        let mut b = Bus::new();
    b.map_ram(0, 0x10000);
    b.write_tlb(0x284, 0x0150_0028_0196, 5);
    b.translated = true;
    assert!(b.read(0, 4).is_err());
    assert!(b.write(0, 4, 1).is_err());
    assert!(b.read(0x28_FFFF, 2).is_err());
    }

    #[test]
    fn translated_hits_to_absent_physical_memory_still_fault() {
        let mut b = Bus::new();
        b.write_tlb(0x284, 0x0150_0028_0196, 5);
        b.translated = true;
        assert!(b.read(0x28_0000, 4).is_err());
        assert!(b.write(0x28_0000, 4, 1).is_err());
    }

#[test]
fn tlb_rewrite_replaces_page_and_locked_cache_is_translatable() {
    let mut b = Bus::new();
    b.map_ram(0x8000_017C_0400_0000, 0x10000);
    b.write_tlb(0x408, 0x017C_0400_0193, 0x405);
    b.write_tlb(0x408, 0x017C_0400_0196, 0x405);
    assert_eq!(b.tlb.len(), 1);
    b.translated = true;
    b.write(0x0400_0010, 8, 0x1234).unwrap();
    assert_eq!(b.read(0x0400_0010, 8), Ok(0x1234));
}

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

#[test]
fn hrmor_offsets_only_addresses_without_bit_63() {
    let mut b = Bus::new();
    b.map_ram(0x8000_017A_0300_0000, 0x100);
    b.write(0x8000_017A_0300_0010, 4, 0xCAFE).unwrap();
    b.hrmor = 0x017A_0300_0000;
    assert!(b.read(0x10, 4).is_err()); // 0x017A_0300_0010 is not mapped until the alias exists
    b.map_alias(0x017A_0300_0000, 0x100, 0x8000_017A_0300_0000);
    assert_eq!(b.read(0x10, 4).unwrap(), 0xCAFE);
    assert_eq!(b.read(0x8000_017A_0300_0010, 4).unwrap(), 0xCAFE);
}
