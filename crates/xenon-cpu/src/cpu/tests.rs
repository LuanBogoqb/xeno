use super::*;
use crate::mem::{Bus, BusError};

const BASE: u64 = 0x1000;

fn d(op: u32, rt: u32, ra: u32, imm: u32) -> u32 {
    op << 26 | rt << 21 | ra << 16 | (imm & 0xFFFF)
}
fn x(op: u32, rt: u32, ra: u32, rb: u32, xo: u32, rc: u32) -> u32 {
    op << 26 | rt << 21 | ra << 16 | rb << 11 | xo << 1 | rc
}

fn setup(words: &[u32]) -> (Cpu, Bus) {
    let mut bus = Bus::new();
    bus.map_ram(0, 0x10000);
    for (i, w) in words.iter().enumerate() {
        bus.write(BASE + 4 * i as u64, 4, *w as u64).unwrap();
    }
    (Cpu::new(BASE), bus)
}

fn steps(cpu: &mut Cpu, bus: &mut Bus, n: usize) {
    for _ in 0..n {
        cpu.step(bus).unwrap();
    }
}

#[test]
    fn protected_store_and_nx_fetch_enter_guest_storage_vectors() {
        for nx in [false, true] {
            let mut b = Bus::new();
            let raw = 0x8000_0000_0000_0000;
            b.map_ram(raw, 0x2000);
            let word = d(36, 3, 4, 0) as u64;
            b.write(raw + 0x1000, 4, word).unwrap();
            let mut c = Cpu::new(0x80001000);
            c.slb[0] = (0x88000000, 0x8000);
            c.msr = 0x3030;
            c.gpr[4] = c.pc;
            c.gpr[3] = 0xDEADBEEF;
            b.enable_software_mmu(c.slb);
            b.mmu.insert(0x18, if nx { 0x1196 } else { 0x1193 }, 0x8001);
            c.step(&mut b).unwrap();
            assert_eq!(c.pc, if nx { 0x400 } else { 0x300 });
            assert_eq!(c.spr[&0x1A], 0x80001000);
            if nx { assert_eq!(c.spr[&0x1B], 0x10003030); }
            else {
                assert_eq!(c.spr[&0x13], 0x80001000);
                assert_eq!(c.spr[&0x12], 0x0A000000);
            }
            assert_eq!(b.read(raw + 0x1000, 4), Ok(word));
        }
    }

    #[test]
    fn decrementer_latches_crossing_and_write_rearms() {
        let (mut c, mut b) = setup(&[x(31, 3, 0x16, 0, 467, 0)]);
        b.multicore = true;
        c.spr.insert(0x16, 0); // observing the default register must not arm it
        c.tick_decrementer(100);
        assert!(!c.dec_pending);
        c.spr.insert(0x16, 3);
        c.dec_armed = true;
        c.tick_decrementer(3);
        assert_eq!(c.spr[&0x16], 0);
        assert!(!c.dec_pending);
        c.tick_decrementer(1);
        assert_eq!(c.spr[&0x16], u32::MAX as u64);
        assert!(c.dec_pending);
        c.gpr[3] = 100;
        c.step(&mut b).unwrap();
        assert!(!c.dec_pending);
        c.tick_decrementer(101);
        assert!(c.dec_pending);
        c.pc = BASE;
        c.gpr[3] = 0xFFFFFFFF;
        c.step(&mut b).unwrap();
        assert!(c.dec_pending);
    }

    #[test]
    fn indexed_update_memory_ops_update_base_only_after_success() {
        for (load, store, size) in [(53, 181, 8), (55, 183, 4), (119, 247, 1), (311, 439, 2), (375, 439, 2)] {
            let (mut c, mut b) = setup(&[x(31, 3, 4, 5, store, 0), x(31, 6, 4, 0, load, 0)]);
            c.gpr[3] = u64::MAX;
            c.gpr[4] = 0x2000;
            c.gpr[5] = 8;
            steps(&mut c, &mut b, 2);
            assert_eq!(c.gpr[4], 0x2008);
            assert_eq!(c.gpr[6], if load == 375 { u64::MAX } else { u64::MAX >> (8 * (8 - size)) });
            c.pc = BASE;
            c.gpr[4] = 0xF00000;
            assert!(c.step(&mut b).is_err());
            assert_eq!(c.gpr[4], 0xF00000);
        }
    }

    #[test]
    fn andis_masks_shifted_immediate_and_always_records() {
        let (mut c, mut b) = setup(&[d(29, 3, 4, 0xA000), d(29, 3, 5, 0x1000)]);
        c.gpr[3] = 0xFFFF_FFFF_8000_1234;
        c.xer = XER_SO;
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[4], 0x8000_0000);
        assert_eq!(c.cr >> 28, 5); // legacy test setup is doubleword record mode
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[5], 0);
        assert_eq!(c.cr >> 28, 3);
    }

    #[test]
    fn kernel_word_mode_records_zero_and_carry_at_the_32_bit_boundary() {
        for sf in [false, true] {
            let mut b = Bus::new();
            let base = 0x8000_0000_0000_0000;
            b.map_ram(base, 0x2000);
            b.write(base + BASE, 4, d(13, 3, 3, 1) as u64).unwrap(); // addic. r3,r3,1
            b.write(base + BASE + 4, 4, x(31, 5, 0, 4, 138, 0) as u64).unwrap(); // adde r5,r0,r4
            b.enable_software_mmu([(0, 0); 64]);
            let mut c = Cpu::new(BASE);
            c.msr = if sf { 1 << 63 } else { 0 };
            c.gpr[3] = u32::MAX as u64;
            c.step(&mut b).unwrap();
            assert_eq!(c.cr >> 28, if sf { 4 } else { 2 });
            assert_eq!(c.xer & XER_CA != 0, !sf);
            c.step(&mut b).unwrap();
            assert_eq!(c.gpr[5], if sf { 0 } else { 1 });
        }
    }

    #[test]
    fn word_mode_subfc_uses_word_borrow_not_upper_register_bits() {
        let mut b = Bus::new();
        b.map_ram(0x8000_0000_0000_0000, 0x2000);
        b.write(0x8000_0000_0000_0000 + BASE, 4, x(31, 3, 4, 5, 8, 1) as u64).unwrap();
        b.enable_software_mmu([(0, 0); 64]);
        let mut c = Cpu::new(BASE);
        c.gpr[4] = 1 << 32;
        c.step(&mut b).unwrap();
        assert_ne!(c.xer & XER_CA, 0);
        assert_eq!(c.cr >> 28, 2);
    }

    #[test]
    fn software_mmu_system_call_enters_guest_hv_and_returns_after_sc() {
        for word in [0x4400_0002, 0x4400_0042] {
            let mut b = Bus::new();
            b.map_ram(0x8000_0000_0000_0000, 0x10000);
            b.write(0x8000_0000_0000_1000, 4, word).unwrap();
            b.write(0x8000_0000_0000_0C00, 4, 0x4C00_0024).unwrap();
            b.enable_software_mmu([(0, 0); 64]);
            let mut c = Cpu::new(0x1000);
            c.msr = 0x1000;
            c.step(&mut b).unwrap();
            assert_eq!(c.pc, 0xC00);
            assert_eq!(c.spr[&0x1A], 0x1004);
            assert_eq!(c.spr[&0x1B], 0x1000);
            assert_ne!(c.msr & MSR_HV, 0);
            c.step(&mut b).unwrap();
            assert_eq!(c.pc, 0x1004);
            assert_eq!(c.msr, 0x1000);
        }
    }

    #[test]
    fn software_mmu_misses_enter_hv_vectors_and_preserve_restart_state() {
        for segment in [true, false] {
            let mut c = Cpu::new(0x8007_8DB0);
            let mut b = Bus::new();
            if !segment { c.slb[8] = (0x8800_0008, 0x8100); }
            b.enable_software_mmu(c.slb);
            c.msr = 0x0200_3030;
            c.step(&mut b).unwrap();
            assert_eq!(c.pc, if segment { 0x480 } else { 0x400 });
            assert_eq!(c.spr[&0x1A], 0x8007_8DB0);
            assert_eq!(c.spr[&0x1B], 0x0200_3030 | if segment { 0 } else { 0x4000_0000 });
            assert_eq!(c.msr, MSR_HV | (1 << 63) | 0x1000);
            assert_eq!(c.spr[&0x3B2], 8);
            assert!(!b.translated);
        }
    }

    #[test]
    fn software_mmu_data_fault_is_not_mistaken_for_fetch_and_dar_is_32_bit() {
        let mut b = Bus::new();
        b.map_ram(0x8000_0000_0000_0000, 0x10000);
        b.write(0x8000_0000_0000_1000, 4, d(32, 3, 4, 0) as u64).unwrap();
        b.enable_software_mmu([(0, 0); 64]);
        let mut c = Cpu::new(0x1000);
        c.msr = MSR_HV | 0x10;
        c.gpr[4] = c.pc; // data EA equals PC, but this is a data segment miss
        c.step(&mut b).unwrap();
        assert_eq!(c.pc, 0x380);
        assert_eq!(c.spr[&0x13], 0x1000);
        c.pc = 0x1000;
        c.msr = MSR_HV | 0x10;
        c.gpr[4] = 0xFFFF_FFFF_8E04_8608;
        c.step(&mut b).unwrap();
        assert_eq!(c.spr[&0x13], 0x8E04_8608);
    }

    #[test]
    fn software_mmu_rfid_truncates_32_bit_kernel_entry_pc() {
        let mut b = Bus::new();
        b.map_ram(0x8000_0000_0000_0000, 0x10000);
        b.write(0x8000_0000_0000_1000, 4, 0x4C00_0024).unwrap();
        b.enable_software_mmu([(0, 0); 64]);
        let mut c = Cpu::new(0x1000);
        c.spr.insert(0x1A, 0xFFFF_FFFF_8007_8DB0);
        c.spr.insert(0x1B, 0x0200_3030);
        c.step(&mut b).unwrap();
        assert_eq!(c.pc, 0x8007_8DB0);
    }

    #[test]
    fn carry_arithmetic_handles_both_overflow_stages() {
        let cases = [
            (10, u64::MAX, 1, false, 0, true),
            (10, 1, 2, true, 3, false),
            (138, u64::MAX, 0, true, 0, true),
            (138, u64::MAX, u64::MAX, true, u64::MAX, true),
            (138, 1, 2, false, 3, false),
            (202, u64::MAX, 99, true, 0, true),
            (202, 5, 99, false, 5, false),
            (200, 0, 99, true, 0, true),
            (200, 1, 99, true, u64::MAX, false),
            (200, 0, 99, false, u64::MAX, false),
        ];
        for (xo, a, b, carry, expected, carry_out) in cases {
            let (mut c, mut bus) = setup(&[x(31, 3, 4, 5, xo, 1)]);
            c.gpr[4] = a;
            c.gpr[5] = b;
            c.xer = XER_SO | if carry { XER_CA } else { 0 };
            steps(&mut c, &mut bus, 1);
            assert_eq!(c.gpr[3], expected, "XO {xo}");
            assert_eq!(c.xer & XER_CA != 0, carry_out, "XO {xo}");
            assert_ne!(c.xer & XER_SO, 0);
            assert_eq!(c.cr >> 28, if expected == 0 { 3 } else if expected >> 63 != 0 { 9 } else { 5 });
        }
    }

    #[test]
    fn cr_logical_truth_tables_preserve_other_bits() {
        // Truth table bit index = 2*A+B.
        for (xo, truth) in [(33, 0b0001), (129, 0b0100), (193, 0b0110), (225, 0b0111),
            (257, 0b1000), (289, 0b1001), (417, 0b1101), (449, 0b1110)] {
            for inputs in 0..4 {
                let (mut c, mut b) = setup(&[x(19, 31, 4, 24, xo, 0)]);
                c.cr = 0xA555_5555 & !((1 << 27) | (1 << 7));
                c.cr |= ((inputs >> 1) << 27) | ((inputs & 1) << 7);
                let before = c.cr;
                steps(&mut c, &mut b, 1);
                assert_eq!(c.cr, (before & !1) | (truth >> inputs & 1));
                assert_eq!(c.pc, BASE + 4);
                assert_eq!(c.tb, 1);
            }
        }
    }

    #[test]
    fn cntlzd_counts_all_64_bits_and_records() {
        for (value, expected) in [(0, 64), (1, 63), (0x10001, 47), (1 << 63, 0)] {
            let (mut c, mut b) = setup(&[x(31, 3, 4, 0, 58, 1)]);
            c.gpr[3] = value;
            c.xer = XER_SO;
            steps(&mut c, &mut b, 1);
            assert_eq!(c.gpr[4], expected);
            assert_eq!(c.cr >> 28, if expected == 0 { 3 } else { 5 });
        }
    }

    #[test]
    fn reserve_loads_and_conditional_stores_update_only_cr0() {
        for (load_xo, store_xo, size, value) in [(20, 150, 4, 0x1234_5678), (84, 214, 8, 0x1234_5678_9ABC_DEF0)] {
            let (mut c, mut b) = setup(&[
                x(31, 3, 0, 4, load_xo, 0),
                x(31, 5, 0, 4, store_xo, 1),
                x(31, 5, 0, 4, store_xo, 1),
            ]);
            c.gpr[4] = 0x2000;
            c.gpr[5] = value;
            c.xer = XER_SO;
            c.cr = 0x8ABC_DEF0;
            b.write(0x2000, size, 0xDEAD).unwrap();
            steps(&mut c, &mut b, 1);
            assert_eq!(c.gpr[3], 0xDEAD);
            steps(&mut c, &mut b, 1);
            assert_eq!(b.read(0x2000, size), Ok(value));
            assert_eq!(c.cr, 0x3ABC_DEF0); // EQ | SO
            steps(&mut c, &mut b, 1);
            assert_eq!(c.cr, 0x1ABC_DEF0); // reservation consumed
        }
    }

    #[test]
    fn lis_ori_build_constant() {
        let (mut c, mut b) = setup(&[d(15, 3, 0, 0x8000), d(24, 3, 3, 0x1234)]);
    steps(&mut c, &mut b, 1);
    assert_eq!(c.gpr[3], 0xFFFF_FFFF_8000_0000);
    steps(&mut c, &mut b, 1);
    assert_eq!(c.gpr[3], 0xFFFF_FFFF_8000_1234);
}

#[test]
fn rlwinm_ignores_high_bits() {
    // rlwinm r4,r3,0,24,31
    let w = 21 << 26 | 3 << 21 | 4 << 16 | 24 << 6 | 31 << 1;
    let (mut c, mut b) = setup(&[w]);
    c.gpr[3] = 0xDEAD_BEEF_0000_00F0;
    steps(&mut c, &mut b, 1);
    assert_eq!(c.gpr[4], 0xF0);
}

#[test]
fn instruction_and_data_translation_are_independent_and_errors_reset_bus_mode() {
    let (mut c, mut b) = setup(&[0x8064_0000]); // lwz r3,0(r4)
    b.map_ram(0x8000_0200_0000_0000, 0x10000);
    b.write(0x8000_0200_0000_00FC, 4, 0x1234).unwrap();
    b.write_tlb(0x102, 0x0200_0000_11BE, 0x1005);
    c.gpr[4] = 0x1000_00FC;
    c.msr = 0x10; // DR on, IR off
    c.step(&mut b).unwrap();
    assert_eq!(c.gpr[3], 0x1234);
    c.pc = BASE;
    c.gpr[4] = 0x2000_0000;
    assert!(c.step(&mut b).is_err());
    assert_eq!(b.read(BASE, 4), Ok(0x8064_0000));

    b.write(0x8000_0200_0000_0000, 4, 0x3860_0007).unwrap();
    c.pc = 0x1000_0000;
    c.msr = 0x20; // IR on, DR off
    c.step(&mut b).unwrap();
    assert_eq!(c.gpr[3], 7);
}

#[test]
fn extsw_sign_extends() {
    let (mut c, mut b) = setup(&[x(31, 3, 4, 0, 986, 0)]);
    c.gpr[3] = 0x0000_0000_8000_0000;
    steps(&mut c, &mut b, 1);
    assert_eq!(c.gpr[4], 0xFFFF_FFFF_8000_0000);
}

#[test]
fn cd_integer_instructions() {
    for (xo, value, expected) in [(954, 0x80u64, 0xFFFFFFFFFFFFFF80u64), (922, 0x8000u64, 0xFFFFFFFFFFFF8000u64)] {
        let (mut c, mut b) = setup(&[x(31, 3, 4, 0, xo, 1)]);
        c.gpr[3] = value;
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[4], expected);
        assert_eq!(c.cr >> 28, 8);
    }
    let (mut c, mut b) = setup(&[x(31, 3, 4, 0, 104, 0), x(31, 5, 4, 3, 8, 0)]);
    c.gpr[4] = 1;
    steps(&mut c, &mut b, 1);
    assert_eq!(c.gpr[3], u64::MAX);
    steps(&mut c, &mut b, 1);
    assert_eq!(c.gpr[5], u64::MAX - 1);
    assert_ne!(c.xer & XER_CA, 0);
}

#[test]
fn srawi_sign_extends_and_updates_carry() {
    for (value, shift, expected, carry) in [(0x8000_0001u64, 3u64, 0xFFFFFFFFF0000000u64, true), (0x8000_0000u64, 0u64, 0xFFFFFFFF80000000u64, false), (8u64, 3u64, 1u64, false)] {
        let (mut c, mut b) = setup(&[x(31, 3, 4, shift as u32, 824, 0)]);
        c.gpr[3] = value;
        c.xer = XER_CA;
        steps(&mut c, &mut b, 1);
        assert_eq!(c.gpr[4], expected);
        assert_eq!(c.xer & XER_CA != 0, carry);
    }
}

#[test]
fn indexed_halfword_loads_and_store() {
    let (mut c, mut b) = setup(&[x(31, 3, 4, 5, 407, 0), x(31, 6, 4, 5, 279, 0), x(31, 7, 4, 5, 343, 0)]);
    c.gpr[3] = 0x1234_8001;
    c.gpr[4] = 0x2000;
    c.gpr[5] = 2;
    steps(&mut c, &mut b, 3);
    assert_eq!(b.read(0x2002, 2), Ok(0x8001));
    assert_eq!(c.gpr[6], 0x8001);
    assert_eq!(c.gpr[7], 0xFFFFFFFFFFFF8001);
}

#[test]
fn cmpw_uses_low_32_bits() {
    // cmpw cr0,r3,r4
    let (mut c, mut b) = setup(&[x(31, 0, 3, 4, 0, 0)]);
    c.gpr[3] = 0x1111_1111_0000_0005;
    c.gpr[4] = 0x2222_2222_0000_0005;
    steps(&mut c, &mut b, 1);
    assert_eq!(c.cr >> 28, 0b0010);
}

#[test]
fn bl_sets_lr_and_blr_returns() {
    // bl +8 ; (skipped) ; blr
    let (mut c, mut b) = setup(&[18 << 26 | 8 | 1, 0, 19 << 26 | 20 << 21 | 16 << 1]);
    steps(&mut c, &mut b, 1);
    assert_eq!(c.pc, BASE + 8);
    assert_eq!(c.lr, BASE + 4);
    steps(&mut c, &mut b, 1);
    assert_eq!(c.pc, BASE + 4);
}

#[test]
fn bdnz_loop_ends_with_ctr_zero() {
    // bdnz . (self-branch while ctr != 0)
    let (mut c, mut b) = setup(&[16 << 26 | 16 << 21 | 0, 0]);
    c.ctr = 5;
    steps(&mut c, &mut b, 5);
    assert_eq!(c.ctr, 0);
    assert_eq!(c.pc, BASE + 4);
}

#[test]
fn lwz_stw_roundtrip_big_endian() {
    let (mut c, mut b) = setup(&[d(36, 3, 5, 0), d(32, 4, 5, 0)]);
    c.gpr[3] = 0xAABB_CCDD;
    c.gpr[5] = 0x2000;
    steps(&mut c, &mut b, 2);
    assert_eq!(b.read(0x2000, 1).unwrap(), 0xAA);
    assert_eq!(c.gpr[4], 0xAABB_CCDD);
}

#[test]
fn ld_std_roundtrip() {
    let (mut c, mut b) = setup(&[d(62, 3, 5, 8), d(58, 4, 5, 8)]);
    c.gpr[3] = 0x0102_0304_0506_0708;
    c.gpr[5] = 0x2000;
    steps(&mut c, &mut b, 2);
    assert_eq!(b.read(0x2008, 1).unwrap(), 0x01);
    assert_eq!(c.gpr[4], 0x0102_0304_0506_0708);
}

#[test]
fn unknown_opcode_stops() {
    let (mut c, mut b) = setup(&[0]);
    assert_eq!(c.run(&mut b, 10), Stop::UnknownOpcode { pc: 0x1000, word: 0 });
}

#[test]
fn step_limit_stops() {
    let (mut c, mut b) = setup(&[18 << 26]); // b .
    assert_eq!(c.run(&mut b, 100), Stop::StepLimit);
}

#[test]
fn unmapped_load_stops() {
    let (mut c, mut b) = setup(&[d(32, 4, 5, 0)]);
    c.gpr[5] = 0x9000_0000;
    assert_eq!(
        c.run(&mut b, 10),
        Stop::Unmapped(BusError::Unmapped { addr: 0x9000_0000, size: 4, write: false })
    );
}

#[test]
fn mtspr_mfspr_roundtrip_unknown_spr() {
    // mtspr 0x98,r3 ; mfspr r4,0x98  (spr field is split: low5 | high5<<5)
    let f = |xo: u32, rt: u32| 31 << 26 | rt << 21 | (0x98 & 31) << 16 | (0x98 >> 5) << 11 | xo << 1;
    let (mut c, mut b) = setup(&[f(467, 3), f(339, 4)]);
    c.gpr[3] = 0xCAFE;
    steps(&mut c, &mut b, 2);
    assert_eq!(c.gpr[4], 0xCAFE);
}

#[test]
fn mflr_mtlr() {
    let (mut c, mut b) = setup(&[31 << 26 | 3 << 21 | 8 << 16 | 467 << 1, 31 << 26 | 4 << 21 | 8 << 16 | 339 << 1]);
    c.gpr[3] = 0x1234;
    steps(&mut c, &mut b, 2);
    assert_eq!((c.lr, c.gpr[4]), (0x1234, 0x1234));
}

#[test]
fn msr_hv_bit_is_bit60() {
    assert_eq!(MSR_HV, 0x1000_0000_0000_0000);
}

#[test]
fn sc_lev2_traps() {
    let (mut c, mut b) = setup(&[0x4400_0042]);
    assert_eq!(c.run(&mut b, 10), Stop::Trap { pc: BASE });
}

#[test]
fn stbu_updates_base() {
    let (mut c, mut b) = setup(&[d(39, 7, 3, 1)]);
    c.gpr[3] = 0x2000;
    c.gpr[7] = 0x1AB;
    steps(&mut c, &mut b, 1);
    assert_eq!((c.gpr[3], b.read(0x2001, 1).unwrap()), (0x2001, 0xAB));
}

fn xl(xo: u32, bt: u32, ba: u32, bb: u32) -> u32 {
    19 << 26 | bt << 21 | ba << 16 | bb << 11 | xo << 1
}

fn run1(w: u32, init: impl FnOnce(&mut Cpu, &mut Bus)) -> (Cpu, Bus) {
    let (mut c, mut b) = setup(&[w]);
    init(&mut c, &mut b);
    steps(&mut c, &mut b, 1);
    (c, b)
}

#[test]
fn op_mulli() {
    let (c, _) = run1(d(7, 3, 4, (-3i16) as u32), |c, _| c.gpr[4] = 7);
    assert_eq!(c.gpr[3] as i64, -21);
}

#[test]
fn op_addic_sets_carry() {
    let (c, _) = run1(d(12, 3, 4, 1), |c, _| c.gpr[4] = u64::MAX);
    assert_eq!(c.gpr[3], 0);
    assert_ne!(c.xer & XER_CA, 0);
    let (c, _) = run1(d(13, 3, 4, 0), |c, _| c.gpr[4] = 5);
    assert_eq!(c.xer & XER_CA, 0);
    assert_eq!(c.cr >> 28, 0b0100);
}

#[test]
fn op_xori_xoris() {
    let (c, _) = run1(d(26, 3, 4, 0xFF00), |c, _| c.gpr[3] = 0x0FF0);
    assert_eq!(c.gpr[4], 0xF0F0);
    let (c, _) = run1(d(27, 3, 4, 0x0001), |c, _| c.gpr[3] = 0x1_0000);
    assert_eq!(c.gpr[4], 0);
}

#[test]
fn op_rlwnm_and_rotlw() {
    // rotlw r4,r3,r5 = rlwnm r4,r3,r5,0,31; the amount is the low 5 bits of rb
    let w = 23 << 26 | 3 << 21 | 4 << 16 | 5 << 11 | 31 << 1;
    let (c, _) = run1(w, |c, _| {
        c.gpr[3] = 0xAAAA_AAAA_8000_0001;
        c.gpr[5] = 0x21;
    });
    assert_eq!(c.gpr[4], 0x0000_0003);
}

#[test]
fn op_rldcr() {
    // rldcr r4,r3,r5,me=7: rotl by rb&63, keep bits 0..=7 (IBM numbering, MSB first)
    let w = 30 << 26 | 3 << 21 | 4 << 16 | 5 << 11 | 7 << 6 | 9 << 1;
    let (c, _) = run1(w, |c, _| {
        c.gpr[3] = 0x0123_4567_89AB_CDEF;
        c.gpr[5] = 8;
    });
    assert_eq!(c.gpr[4], 0x2300_0000_0000_0000);
}

#[test]
fn op_shifts() {
    let (c, _) = run1(x(31, 3, 4, 5, 24, 0), |c, _| {
        c.gpr[3] = 0xFFFF_FFFF_0000_0001;
        c.gpr[5] = 4;
    });
    assert_eq!(c.gpr[4], 0x10);
    let (c, _) = run1(x(31, 3, 4, 5, 24, 0), |c, _| {
        c.gpr[3] = 1;
        c.gpr[5] = 32;
    });
    assert_eq!(c.gpr[4], 0);
    let (c, _) = run1(x(31, 3, 4, 5, 536, 0), |c, _| {
        c.gpr[3] = 0xFFFF_FFFF_8000_0000;
        c.gpr[5] = 31;
    });
    assert_eq!(c.gpr[4], 1);
    let (c, _) = run1(x(31, 3, 4, 5, 27, 0), |c, _| {
        c.gpr[3] = 1;
        c.gpr[5] = 40;
    });
    assert_eq!(c.gpr[4], 1 << 40);
    let (c, _) = run1(x(31, 3, 4, 5, 539, 0), |c, _| {
        c.gpr[3] = 1 << 40;
        c.gpr[5] = 64;
    });
    assert_eq!(c.gpr[4], 0);
}

#[test]
fn op_andc_cntlzw() {
    let (c, _) = run1(x(31, 3, 4, 5, 60, 0), |c, _| {
        c.gpr[3] = 0xFF;
        c.gpr[5] = 0x0F;
    });
    assert_eq!(c.gpr[4], 0xF0);
    let (c, _) = run1(x(31, 3, 4, 0, 26, 0), |c, _| c.gpr[3] = 0xFFFF_FFFF_0001_0000);
    assert_eq!(c.gpr[4], 15);
}

#[test]
    fn mtcrf_and_mtocrf_preserve_unselected_fields() {
        for (fxm, single, expected) in [(0x02, true, 0x1234_56D8), (0x81, false, 0xA234_567F), (0, false, 0x1234_5678)] {
            let w = x(31, 5, 0, 0, 144, 0) | fxm << 12 | (single as u32) << 20;
            let (c, _) = run1(w, |c, _| {
                c.cr = 0x1234_5678;
                c.gpr[5] = 0xDEAD_BEEF_ABCD_EFDF;
            });
            assert_eq!(c.cr, expected);
        }
    }

    #[test]
    fn mulhdu_returns_unsigned_high_half_and_honors_record_bit() {
        for (a, b, expected) in [
            (0, u64::MAX, 0),
            (u64::MAX, u64::MAX, u64::MAX - 1),
            (1 << 63, 2, 1),
            (0x1234_5678_9ABC_DEF0, 0xFEDC_BA98_7654_3210, 0x121F_A00A_D77D_7422),
        ] {
            let (c, _) = run1(x(31, 3, 4, 5, 9, 1), |c, _| {
                c.gpr[4] = a;
                c.gpr[5] = b;
                c.xer = XER_SO | XER_CA;
            });
            assert_eq!(c.gpr[3], expected);
            assert_eq!(c.xer, XER_SO | XER_CA);
            assert_eq!(c.cr >> 28, if expected == 0 { 3 } else if expected >> 63 != 0 { 9 } else { 5 });
        }
    }

    #[test]
    fn op_mul_div() {
        let (c, _) = run1(x(31, 3, 4, 5, 235, 0), |c, _| {
        c.gpr[4] = 0xFFFF_FFFF_FFFF_FFFF; // -1 as 32-bit
        c.gpr[5] = 3;
    });
    assert_eq!(c.gpr[3] as i64, -3);
    let (c, _) = run1(x(31, 3, 4, 5, 233, 0), |c, _| {
        c.gpr[4] = 1 << 33;
        c.gpr[5] = 3;
    });
    assert_eq!(c.gpr[3], 3 << 33);
    let (c, _) = run1(x(31, 3, 4, 5, 459, 0), |c, _| {
        c.gpr[4] = 0xDEAD_BEEF_0000_0064;
        c.gpr[5] = 7;
    });
    assert_eq!(c.gpr[3], 14);
    let (c, _) = run1(x(31, 3, 4, 5, 457, 0), |c, _| {
        c.gpr[4] = 1 << 40;
        c.gpr[5] = 1 << 8;
    });
    assert_eq!(c.gpr[3], 1 << 32);
}

#[test]
fn op_subfe_uses_carry() {
    let (c, _) = run1(x(31, 3, 4, 5, 136, 0), |c, _| {
        c.gpr[4] = 1;
        c.gpr[5] = 10;
        c.xer |= XER_CA;
    });
    assert_eq!(c.gpr[3], 9); // ~1 + 10 + 1
    assert_ne!(c.xer & XER_CA, 0);
    let (c, _) = run1(x(31, 3, 4, 5, 136, 0), |c, _| {
        c.gpr[4] = 1;
        c.gpr[5] = 10;
    });
    assert_eq!(c.gpr[3], 8);
}

#[test]
fn op_lha_sign_extends() {
    let (c, _) = run1(d(42, 3, 4, 0), |c, b| {
        c.gpr[4] = 0x2000;
        b.write(0x2000, 2, 0x8001).unwrap();
    });
    assert_eq!(c.gpr[3], 0xFFFF_FFFF_FFFF_8001);
}

#[test]
fn op_indexed_dword_and_byte_reverse() {
    let (c, _) = run1(x(31, 3, 4, 5, 21, 0), |c, b| {
        c.gpr[4] = 0x2000;
        c.gpr[5] = 8;
        b.write(0x2008, 8, 0x1122_3344_5566_7788).unwrap();
    });
    assert_eq!(c.gpr[3], 0x1122_3344_5566_7788);
    let (_, mut b) = run1(x(31, 3, 4, 5, 149, 0), |c, _| {
        c.gpr[3] = 0xAABB_CCDD_EEFF_0011;
        c.gpr[4] = 0x2000;
        c.gpr[5] = 8;
    });
    assert_eq!(b.read(0x2008, 8).unwrap(), 0xAABB_CCDD_EEFF_0011);
    let (c, _) = run1(x(31, 3, 4, 5, 534, 0), |c, b| {
        c.gpr[4] = 0x2000;
        b.write(0x2000, 4, 0x1122_3344).unwrap();
    });
    assert_eq!(c.gpr[3], 0x4433_2211);
    let (_, mut b) = run1(x(31, 3, 4, 5, 662, 0), |c, _| {
        c.gpr[3] = 0x1122_3344;
        c.gpr[4] = 0x2000;
    });
    assert_eq!(b.read(0x2000, 4).unwrap(), 0x4433_2211);
    let (c, _) = run1(x(31, 3, 4, 5, 790, 0), |c, b| {
        c.gpr[4] = 0x2000;
        b.write(0x2000, 2, 0x1122).unwrap();
    });
    assert_eq!(c.gpr[3], 0x2211);
    let (_, mut b) = run1(x(31, 3, 4, 5, 918, 0), |c, _| {
        c.gpr[3] = 0xAA1122;
        c.gpr[4] = 0x2000;
    });
    assert_eq!(b.read(0x2000, 2).unwrap(), 0x2211);
}

#[test]
fn op_mcrf_mfocrf_mftb_mfmsr() {
    // mcrf cr1,cr0
    let (c, _) = run1(xl(0, 1 << 2, 0 << 2, 0), |c, _| c.cr = 0xA000_0000);
    assert_eq!(c.cr, 0xAA00_0000);
    // mfocrf r3, fxm=0x80 (cr0)
    let (c, _) = run1(x(31, 3, 0, 0, 19, 0) | 1 << 20 | 0x80 << 12, |c, _| c.cr = 0x1234_5678);
    assert_eq!(c.gpr[3], 0x1000_0000);
    // mftb r3 / mftb r4 (TBL = 268): the time base advances every step
    let tbr = (268 & 31) << 16 | (268 >> 5) << 11;
    let (mut c, mut b) = setup(&[31 << 26 | 3 << 21 | tbr | 371 << 1, 31 << 26 | 4 << 21 | tbr | 371 << 1]);
    steps(&mut c, &mut b, 2);
    assert!(c.gpr[4] > c.gpr[3]);
    let (c, _) = run1(x(31, 3, 0, 0, 83, 0), |c, _| c.msr = MSR_HV | 2);
    assert_eq!(c.gpr[3], MSR_HV | 2);
}

#[test]
fn op_barriers_are_noops_and_dcbz_zeroes() {
    for xo in [598u32, 854, 54, 86, 982, 470, 278, 246, 274, 306, 498] {
        let (c, _) = run1(x(31, 0, 4, 5, xo, 0), |c, _| c.gpr[4] = 0x2000);
        assert_eq!(c.pc, BASE + 4, "xo {xo}");
    }
    let (c, _) = run1(xl(150, 0, 0, 0), |_, _| {});
    assert_eq!(c.pc, BASE + 4);
    // dcbz zeroes the aligned 32 bytes, dcbzl the aligned 128
    let fill = |c: &mut Cpu, b: &mut Bus| {
        c.gpr[5] = 0x2013;
        for i in 0..0x100u64 {
            b.write(0x2000 + i, 1, 0xFF).unwrap();
        }
    };
    let (_, mut b) = run1(x(31, 0, 0, 5, 1014, 0), fill);
    assert_eq!(b.read(0x2000, 8).unwrap(), 0);
    assert_eq!(b.read(0x2018, 8).unwrap(), 0);
    assert_eq!(b.read(0x2020, 1).unwrap(), 0xFF);
    let (_, mut b) = run1(x(31, 1, 0, 5, 1014, 0), fill);
    assert_eq!(b.read(0x2000, 1).unwrap(), 0);
    assert_eq!(b.read(0x207F, 1).unwrap(), 0);
    assert_eq!(b.read(0x2080, 1).unwrap(), 0xFF);
}

#[test]
fn op_rfid_loads_pc_and_msr_from_srr() {
    let (c, _) = run1(xl(18, 0, 0, 0), |c, _| {
        c.spr.insert(0x1A, 0x4003);
        c.spr.insert(0x1B, MSR_HV | 0x30);
    });
    assert_eq!(c.pc, 0x4000);
    assert_eq!(c.msr, MSR_HV | 0x30);
}

#[test]
fn hid6_lives_at_spr_3f9_and_is_stored_for_the_mmu() {
    // mtspr 0x3F9,r3 ; mfspr r4,0x3F9 -- HID6 (0x3F6 is HID5).
    let (mut c, mut b) = setup(&[
        x(31, 3, 0x3F9 & 31, 0x3F9 >> 5, 467, 0),
        x(31, 4, 0x3F9 & 31, 0x3F9 >> 5, 339, 0),
    ]);
    c.gpr[3] = 0x0001_0038_0000_0000;
    steps(&mut c, &mut b, 2);
    assert_eq!(c.gpr[4], 0x0001_0038_0000_0000);
    assert_eq!(b.mmu.hid6, 0x0001_0038_0000_0000);
    assert!(b.mmu.large_pages_enabled());
}

#[test]
fn hid5_spr_3f6_stays_independent_of_hid6() {
    let (mut c, mut b) = setup(&[
        x(31, 3, 0x3F6 & 31, 0x3F6 >> 5, 467, 0),
        x(31, 4, 0x3F6 & 31, 0x3F6 >> 5, 339, 0),
    ]);
    c.gpr[3] = 0x55;
    steps(&mut c, &mut b, 2);
    assert_eq!(c.gpr[4], 0x55);
    assert_eq!(b.mmu.hid6, 0);
}

#[test]
fn hrmor_takes_effect_at_the_next_context_synchronization() {
    // mtspr 0x139,r3 ; isync
    let (mut c, mut b) = setup(&[x(31, 3, 0x139 & 31, 0x139 >> 5, 467, 0), xl(150, 0, 0, 0)]);
    c.gpr[3] = 0x017A_0300_0000;
    steps(&mut c, &mut b, 1);
    assert_eq!(b.hrmor, 0);
    assert_eq!(c.spr[&0x139], 0x017A_0300_0000);
    steps(&mut c, &mut b, 1);
    assert_eq!(b.hrmor, 0x017A_0300_0000);
}

#[test]
fn hrmor_takes_effect_at_rfid_after_fetching_it_with_the_old_value() {
    // mtspr 0x139,r3 ; rfid -- the rfid is fetched under the old HRMOR
    let (mut c, mut b) = setup(&[x(31, 3, 0x139 & 31, 0x139 >> 5, 467, 0), xl(18, 0, 0, 0)]);
    c.gpr[3] = 0x4000;
    c.spr.insert(0x1A, 0x20);
    steps(&mut c, &mut b, 2);
    assert_eq!(b.hrmor, 0x4000);
    assert_eq!(c.pc, 0x20);
}

#[test]
fn hrmor_takes_effect_for_the_fetch_after_a_blr_that_follows_the_mtspr() {
    // mtspr 0x139,r3 ; blr  -- the target is fetched with the new HRMOR
    let (mut c, mut b) = setup(&[x(31, 3, 0x139 & 31, 0x139 >> 5, 467, 0), 19 << 26 | 20 << 21 | 16 << 1]);
    c.gpr[3] = 0x10000;
    c.lr = 0x40;
    steps(&mut c, &mut b, 1);
    assert_eq!(b.hrmor, 0);
    steps(&mut c, &mut b, 1);
    assert_eq!((c.pc, b.hrmor), (0x40, 0x10000));
}

#[test]
fn op_mtmsr_mtmsrd() {
    let (c, _) = run1(x(31, 3, 0, 0, 146, 0), |c, _| c.gpr[3] = MSR_HV | 0x30);
    assert_eq!(c.msr, MSR_HV | 0x30);
    let (c, _) = run1(x(31, 3, 0, 0, 178, 0), |c, _| {
        c.gpr[3] = u64::MAX;
        c.msr = MSR_HV;
    });
    assert_eq!(c.msr, u64::MAX);
    // L=1 (bit 16 of the word) only takes EE and RI
    let (c, _) = run1(x(31, 3, 1, 0, 178, 0), |c, _| {
        c.gpr[3] = u64::MAX;
        c.msr = MSR_HV;
    });
    assert_eq!(c.msr, MSR_HV | 0x8002);
}

#[test]
fn op_slbmte_and_back() {
    // slbmte r3(RS), r4(RB) ; slbmfee r5, r4 ; slbmfev r6, r4
    let (mut c, mut b) = setup(&[x(31, 3, 0, 4, 402, 0), x(31, 5, 0, 4, 915, 0), x(31, 6, 0, 4, 851, 0)]);
    c.gpr[3] = 0x1310;
    c.gpr[4] = 0x1800_0001;
    steps(&mut c, &mut b, 3);
    assert_eq!((c.gpr[5], c.gpr[6]), (0x1800_0001, 0x1310));
}
