//! End-to-end checks against the owner's dumps (`dumps/`, not in git): run with `cargo test -- --ignored`.
use std::path::Path;
use std::process::Command;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap()
}

/// The dumps are the owner's and are not distributed, so CI has none. Every
/// test that needs them skips with a message instead of failing; locally the
/// same suite runs for real when `dumps/` is present.
fn dumps_present() -> bool {
    ["dumps/stages/CB_A.mem", "dumps/FLASH_1.bin"]
        .iter()
        .all(|p| root().join(p).exists())
}

macro_rules! require_dumps {
    () => {
        if !dumps_present() {
            eprintln!("skipping: dumps/ is not present (see docs/getting-started.md)");
            return;
        }
    };
}

/// Runs `runcb` and returns (stdout lines, the hex dump requested as `addr:len`).
fn runcb(extra: &[&str], dump: &str) -> (String, Vec<u8>) {
    let out = Command::new(env!("CARGO_BIN_EXE_runcb"))
        .current_dir(root())
        .args(["dumps/stages/CB_A.mem", "dumps/FLASH_1.bin"])
        .args(extra)
        .arg(dump)
        .output()
        .expect("run runcb");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let hex = text.lines().rev().find(|l| l.starts_with("dump ")).and_then(|l| l.split(": ").nth(1)).expect("dump line");
    let bytes = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect();
    (text, bytes)
}

#[test]
#[ignore]
fn cd_in_ram_matches_stage_file() {
    require_dumps!();
    let (text, mem) = runcb(&["--stage", "cb_b", "--max-steps", "30000000"], "8000017c04000000:5400");
    assert!(text.contains("HANDOFF pc=0x4000300"), "{text}");
    let cd = std::fs::read(root().join("dumps/stages/CD.bin")).unwrap();
    assert_eq!(mem.len(), cd.len());
    // header bytes 0x7 and 0x10..0x20 are rewritten by CB_B (flags, derived key); the rest must match
    assert_eq!(&mem[..4], &cd[..4]);
    // 0x24A: the flag byte after the "XBOX_ROM_6" string, 0 in the file and 1 once CB_B has run
    assert_eq!((mem[0x24A], cd[0x24A]), (1, 0));
    let first_diff = (0x20..cd.len()).find(|&i| i != 0x24A && mem[i] != cd[i]);
    assert_eq!(first_diff, None, "first differing offset");
}

#[test]
#[ignore]
fn cd_decompresses_base_kernel_byte_identically() {
    require_dumps!();
    let (text, mem) = runcb(&["--stage", "cd", "--max-steps", "60000000", "--cd-stop-pc", "4000de4"], "8000000000000000:120000");
    assert!(text.contains("CD end: Handoff { pc: 67112420 }"));
    let expected = std::fs::read(root().join("dumps/stages/kernel_base.bin")).unwrap();
    assert_eq!(mem.len(), expected.len());
    assert_eq!(mem.iter().zip(&expected).position(|(a, b)| a != b), None, "first differing base-kernel byte");
}

#[test]
#[ignore]
fn cd_executes_cf_update_byte_identically_before_finalization() {
    require_dumps!();
    let (text, mem) = runcb(&["--stage", "cd", "--max-steps", "400000000", "--cd-stop-pc", "4000950"], "8000000000000000:1f0000");
    assert!(text.contains("CD end: Handoff { pc: 67111248 }"));
    let expected = std::fs::read(root().join("dumps/stages/kernel_17559.bin")).unwrap();
    assert_eq!(mem.len(), expected.len());
    assert_eq!(mem.iter().zip(&expected).position(|(a, b)| a != b), None, "first differing updated-kernel byte");
}

#[test]
#[ignore]
fn cd_completes_real_handoff_with_nand_patches_and_original_fuses() {
    require_dumps!();
    let (text, mem) = runcb(&["--stage", "cd", "--max-steps", "400000000",
        "800002000001fd00:200", "4000260:90"], "8000000000000000:1f0000");
    assert!(text.contains("CD end: Handoff { pc: 256 }"));
    assert!(text.contains("msr=1000000000000000 hrmor=10000000000"));
    assert!(text.contains("digest_overrides=1")); // only the existing CB_A no-glitch compare
    let mut expected = std::fs::read(root().join("dumps/stages/kernel_17559.bin")).unwrap();
    // CD+0xFF8 / +0xFFC: finalized boot flags and this console's fuse-derived LDV.
    expected[6..8].copy_from_slice(&0xD81Eu16.to_be_bytes());
    expected[0x14..0x18].copy_from_slice(&7u32.to_be_bytes());
    // CD+0xAB0 copies CB_B's boot arguments, merging missing key slots from
    // its own header. Validate the transport against both live source blocks.
    let dump = |prefix: &str| -> Vec<u8> {
        let hex = text.lines().find(|line| line.starts_with(prefix)).unwrap().split(": ").nth(1).unwrap();
        (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2*i..2*i+2], 16).unwrap()).collect()
    };
    let mut args = dump("dump 800002000001fd00+");
    let keys = dump("dump 0000000004000260+");
    assert_eq!(&args[..4], b"CB3A");
    for i in 0..8 {
        let slot = 0x170 + 2*i;
        if args[slot..slot+2] == [0, 0] && keys[2*i..2*i+2] != [0, 0] {
            args[slot..slot+2].copy_from_slice(&keys[2*i..2*i+2]);
            args[0x180+16*i..0x190+16*i].copy_from_slice(&keys[0x10+16*i..0x20+16*i]);
        }
    }
    let args_dest = u32::from_be_bytes(expected[0x48..0x4C].try_into().unwrap()) as usize;
    expected[args_dest..args_dest+0x200].copy_from_slice(&args);
    let flash = std::fs::read(root().join("dumps/FLASH_1.bin")).unwrap();
    let word = |offset| u32::from_be_bytes(flash[offset..offset + 4].try_into().unwrap()) as usize;
    // Same descriptor format consumed by CD+0x5384..0x53FC; no patches are injected into the emulator.
    let slot_size = match word(0x70) { 0 => 0x10000, size => size };
    let mut cursor = word(0x64) + slot_size + 0x10;
    loop {
        let dest = word(cursor);
        cursor += 4;
        if dest == u32::MAX as usize { break; }
        let size = word(cursor) * 4;
        cursor += 4;
        expected[dest..dest + size].copy_from_slice(&flash[cursor..cursor + size]);
        cursor += size;
    }
    assert_eq!(mem.len(), expected.len());
    assert_eq!(mem.iter().zip(&expected).position(|(a, b)| a != b), None, "first differing finalized-kernel byte");
}

#[test]
#[ignore]
fn hv_boots_virtual_kernel_and_enters_the_guest_system_call_handler() {
    require_dumps!();
    let (text, code) = runcb(&["--stage", "hv", "--max-steps", "400000000",
        "--hv-stop-pc", "c00"], "8000010000000100:20");
    assert!(text.contains("CD end: Handoff { pc: 256 }"), "{text}");
    assert!(text.contains("HV end: Handoff { pc: 3072 }"), "{text}");
    // Kernel 17559's first sc (r0=15) is at 0x8007F97C. A real software
    // translation miss/reload and guest kernel instructions precede this call.
    assert!(text.contains("SRR0=0x8007f980"), "{text}");
    assert!(text.contains("r0 =000000000000000f"), "{text}");
    assert!(text.contains("digest_overrides=1"), "{text}");
    let kernel = std::fs::read(root().join("dumps/stages/kernel_17559.bin")).unwrap();
    assert_eq!(code, kernel[0x100..0x120]);
}

#[test]
#[ignore]
fn kernel_initializes_memory_and_threads_up_to_the_multicore_barrier() {
    require_dumps!();
    let (text, hv_code) = runcb(&["--stage", "hv", "--max-steps", "400000000",
        "--hv-max-steps", "20000000", "--hv-stop-pc", "8015d440",
        "80000000001a5748:4"], "8000010000001248:4");
    assert!(text.contains("HV end: Handoff { pc: 2148914240 }"), "{text}");
    // Five secondary PCRs prepared, but no secondary CPU has run yet.
    assert!(text.contains("r31=0000000000000006"), "{text}");
    assert!(text.contains("dump 80000000001a5748+4: 00000000"), "{text}");
    // Kernel's physical-page initialization must leave the separate HV
    // plaintext intact (this location was zeroed with the old shared view).
    let kernel = std::fs::read(root().join("dumps/stages/kernel_17559.bin")).unwrap();
    assert_eq!(hv_code, kernel[0x1248..0x124C]);
    assert!(text.contains("digest_overrides=1"), "{text}");
}
#[test]
#[ignore]
fn secondary_threads_execute_guest_startup_and_release_the_barrier() {
    require_dumps!();
    let (text, count) = runcb(&["--stage", "hv", "--max-steps", "400000000",
        "--hv-threads", "6", "--hv-max-steps", "20000000", "--hv-stop-pc", "8015d44c"],
        "80000000001a5748:4");
    assert!(text.contains("HV end: Handoff { pc: 2148914252 }"), "{text}");
    assert_eq!(count, [0, 0, 0, 5]);
    for t in 1..6 {
        let line = text.lines().find(|l| l.starts_with(&format!("CPU{t}:"))).expect("CPU diagnostics");
        assert!(!line.contains("steps=0 "), "{line}");
        assert!(line.contains("ctrl=0xc00000"), "{line}");
    }
    assert!(text.contains("digest_overrides=1"), "{text}");
}
#[test]
#[ignore]
fn all_six_threads_reach_idle_after_the_guest_barrier() {
    require_dumps!();
    let (text, hv_code) = runcb(&["--stage", "hv", "--max-steps", "400000000",
        "--hv-threads", "6", "--hv-max-steps", "40000000",
        "--log-pc", "8015d44c:10"], "8000010000000100:4");
    assert!(text.contains("HV end: StepLimit"), "{text}");
    // CPU0 visits idle transiently before starting secondary CPUs. Require
    // both the real barrier witness and every CPU's final idle PC instead.
    assert!(text.contains("pc 0x8015d44c: r10=0x5"), "{text}");
    for t in 0..6 {
        let line = text.lines().find(|l| l.starts_with(&format!("CPU{t}:"))).expect("CPU diagnostics");
        let pc = line.split("pc=0x").nth(1).unwrap().split_whitespace().next().unwrap();
        let pc = u64::from_str_radix(pc, 16).unwrap();
        assert!((0x8007588C..=0x8007589C).contains(&pc), "{line}");
    }
    let kernel = std::fs::read(root().join("dumps/stages/kernel_17559.bin")).unwrap();
    assert_eq!(hv_code, kernel[0x100..0x104]);
    assert!(!text.contains("UnknownOpcode"), "{text}");
}

/// Captures a plaintext/ciphertext pair AND the security-engine keys (SECRAM
/// 0x10100, 48 bytes = 3x16) in a single boot, then validates the implemented
/// AES-128 XEX + Toeplitz algorithm against the real hardware oracle without
/// patching the HV.  Run only after capturing `dumps/console/seccap_keys.bin`
/// with the updated `tools/seccapture.cpp`.
#[test]
#[ignore]
fn captured_pair_with_real_keys_validates_cipher() {
    // Prefer the newest capture (v25: full HV dump + same-boot pair).
    let path = {
        let v25 = root().join("dumps/console/seccap_v25.bin");
        if v25.exists() { v25 } else { root().join("dumps/console/seccap_keys.bin") }
    };
    if !path.exists() {
        eprintln!("skipping: {} not present", path.display());
        return;
    }
    let d = std::fs::read(&path).unwrap();
    let mut r1parts: Vec<(usize,Vec<u8>)> = Vec::new();
    let mut r0parts: Vec<(usize,Vec<u8>)> = Vec::new();
    let mut key0: Vec<(usize,Vec<u8>)> = Vec::new();
    let mut key1: Vec<(usize,Vec<u8>)> = Vec::new();
    let mut scrparts: Vec<(usize,Vec<u8>)> = Vec::new();
    // Region-1 records around the HV key save area (r2+0x16360).  v22 reads
    // 0x16000..0x16400 so the whole save area is in the capture.
    let mut saveparts: Vec<(usize,Vec<u8>)> = Vec::new();
    // v19+ headers are 16 bytes (tag, window, offset, len); older captures are
    // 12 bytes (tag, offset, len).
    let version = u32::from_be_bytes(d[4..8].try_into().unwrap());
    let hdr_len = if version >= 19 { 16 } else { 12 };
    let mut off = 16usize;
    while off + hdr_len <= d.len() {
        let tag = u32::from_be_bytes(d[off..off+4].try_into().unwrap());
        let a = u32::from_be_bytes(d[off+hdr_len-8..off+hdr_len-4].try_into().unwrap()) as usize;
        let ln = u32::from_be_bytes(d[off+hdr_len-4..off+hdr_len].try_into().unwrap()) as usize;
        off += hdr_len;
        let data = d[off..off+ln].to_vec();
        off += ln;
        let region = tag & 0xFFF;
        // v25 embeds the pair inside 4 KiB block records, so take the part of
        // each record that overlaps 0x3B80..0x4B80 rather than requiring a
        // record to start exactly there.
        const PAIR_LO: usize = 0x3B80;
        const PAIR_HI: usize = 0x4B80;
        let overlap = |a: usize, ln: usize| -> Option<(usize, usize)> {
            let s = a.max(PAIR_LO);
            let e = (a + ln).min(PAIR_HI);
            if s < e { Some((s, e)) } else { None }
        };
        if region == 1 {
            if let Some((s, e)) = overlap(a, ln) { r1parts.push((s, data[s - a..e - a].to_vec())); }
        }
        if region == 0 {
            if let Some((s, e)) = overlap(a, ln) { r0parts.push((s, data[s - a..e - a].to_vec())); }
        }
        if region == 0 && (0x61000..=0x61400).contains(&a) { key0.push((a,data.clone())); }
        if region == 1 && (0x61000..=0x61400).contains(&a) { key1.push((a,data.clone())); }
        if region == 2 { scrparts.push((a, data.clone())); }
        // Key-material windows: the HV save area at r2+0x16360 and the live DRAM
        // key table at 0x80000102_000100F0 (hash/white/aes).  Slice records that
        // overlap the window; v25 captures them inside 4 KiB block records.
        for (lo, hi) in [(0x16000usize, 0x16400usize), (0x100C0usize, 0x10130usize)] {
            if region == 1 {
                let s = a.max(lo);
                let e = (a + ln).min(hi);
                if s < e { saveparts.push((s, data[s - a..e - a].to_vec())); }
            }
        }
    }
    r1parts.sort(); r0parts.sort(); key0.sort(); key1.sort(); scrparts.sort(); saveparts.sort();
    let mut r1 = Vec::new(); for (_,x) in &r1parts { r1.extend_from_slice(x); }
    let mut r0 = Vec::new(); for (_,x) in &r0parts { r0.extend_from_slice(x); }
    let mut k0 = Vec::new(); for (_,x) in &key0 { k0.extend_from_slice(x); }
    let mut k1 = Vec::new(); for (_,x) in &key1 { k1.extend_from_slice(x); }
    let mut scr = Vec::new(); for (_,x) in &scrparts { scr.extend_from_slice(x); }
    assert_eq!(r1.len(), 0x1000, "region1 len");
    assert_eq!(r0.len(), 0x1000, "region0 len");
    let kernel = std::fs::read(root().join("dumps/stages/kernel_17559.bin")).unwrap();
    let expected: Vec<u8> = kernel[0x3B80..0x4B80].to_vec();
    assert_eq!(r1, expected, "region1 must equal kernel (sanity)");

    let mut candidates: Vec<(String, Vec<u8>)> = Vec::new();
    if k0.len() >= 0x30 { candidates.push(("region0@0x61000".into(), k0[..0x30].to_vec())); }
    if k1.len() >= 0x30 { candidates.push(("region1@0x61000".into(), k1[..0x30].to_vec())); }
    for (base,name) in [(0x6000usize,"region2@0x6000"),(0x10000,"region2@0x10000"),(0x16000,"region2@0x16000")].iter() {
        for (off,data) in &scrparts {
            if *off == *base && data.len() >= 0x30 { candidates.push((name.to_string(), data[..0x30].to_vec())); }
        }
    }
    for (off,data) in &scrparts {
        if *off == 0x10000 && data.len() >= 0x130 { candidates.push(("region2@0x10100".into(), data[0x100..0x130].to_vec())); }
    }

    // HV key save area at r2+0x16360 (r2 = 0x8000010200000000).  The layout of
    // the three 16-byte keys is not documented, so try every 16-byte-aligned
    // 48-byte window across the captured save-area range.
    {
        use std::collections::BTreeMap;
        let mut m: BTreeMap<usize, u8> = BTreeMap::new();
        for (off, data) in &saveparts {
            for (i, b) in data.iter().enumerate() { m.insert(off + i, *b); }
        }
        if !m.is_empty() {
            let lo = *m.keys().next().unwrap();
            let hi = *m.keys().next_back().unwrap();
            let mut runs: Vec<(usize, Vec<u8>)> = Vec::new();
            let mut cur: Vec<u8> = Vec::new();
            let mut cur_base = lo;
            for a in lo..=hi {
                match m.get(&a) {
                    Some(b) => {
                        if cur.is_empty() { cur_base = a; }
                        cur.push(*b);
                    }
                    None => {
                        if cur.len() >= 0x30 { runs.push((cur_base, std::mem::take(&mut cur))); }
                        else { cur.clear(); }
                    }
                }
            }
            if cur.len() >= 0x30 { runs.push((cur_base, cur)); }
            for (base, data) in &runs {
                let mut s = 0usize;
                while s + 0x30 <= data.len() {
                    let name = format!("region1@0x{:x}", base + s);
                    candidates.push((name, data[s..s+0x30].to_vec()));
                    s += 16;
                }
            }
        }
    }

    for (name, keys48) in &candidates {
        eprintln!("trying key source: {}", name);
        let chunks: Vec<[u8;16]> = (0..3).map(|i| keys48[i*16..(i+1)*16].try_into().unwrap()).collect();
        for a in 0..3usize {
            for b in 0..3usize {
                for c in 0..3usize {
                    if a==b || a==c || b==c { continue; }
                    let w: [u8;16] = chunks[a];
                    let ae: [u8;16] = chunks[b];
                    let h: [u8;16] = chunks[c];
                    let e = xenon_cpu::security::SecurityEngine::with_keys(0, w, ae, h);
                    let dec = e.xex_decrypt(&r0, 0x3B80);
                    if dec == expected {
                        eprintln!("VALIDATED: source={} white=chunk{} aes=chunk{} hash=chunk{}", name, a, b, c);
                        eprintln!("white={:02x?}", w);
                        eprintln!("aes  ={:02x?}", ae);
                        eprintln!("hash ={:02x?}", h);
                        return;
                    }
                }
            }
        }
    }
    panic!("no candidate/permutation decrypted the pair");
}
/// Dashlaunch kernel plugin (see research/hv/README.md).  Region 1 (hashed) is
/// the decrypted kernel; region 0 (physical) at the same offsets is ciphertext.
/// This is the oracle the placeholder security model must eventually match.
#[test]
#[ignore]
fn captured_security_pair_is_real_and_not_reproduced_by_the_placeholder() {
    let path = root().join("dumps/console/sec_pair_0x3b80.bin");
    if !path.exists() {
        eprintln!("skipping: {} not present", path.display());
        return;
    }
    let pair = std::fs::read(path).unwrap();
    assert_eq!(&pair[..4], b"SECD");
    let offset = u32::from_be_bytes(pair[4..8].try_into().unwrap()) as usize;
    let len = u32::from_be_bytes(pair[8..12].try_into().unwrap()) as usize;
    assert_eq!(offset, 0x3B80);
    assert_eq!(len, 4096);
    let pt = &pair[12..12 + len];
    let ct = &pair[12 + len..12 + 2 * len];

    // The plaintext view is the known decrypted kernel at the same offsets.
    let kernel = std::fs::read(root().join("dumps/stages/kernel_17559.bin")).unwrap();
    let offsets = [0x3B80usize, 0x3F80, 0x4380, 0x4780];
    let expected: Vec<u8> = offsets.iter().flat_map(|&o| kernel[o..o + 0x400].to_vec()).collect();
    assert_eq!(pt, &expected[..], "region 1 plaintext must equal kernel_17559");

    // Physical memory is ciphertext and is not produced by a simple XOR stream
    // or ECB.  Repeated plaintext blocks must not collapse to equal ciphertext.
    assert_ne!(pt, ct);
    let mut seen: std::collections::HashMap<&[u8], &[u8]> = std::collections::HashMap::new();
    let mut ecb = true;
    for (p, c) in pt.chunks(16).zip(ct.chunks(16)) {
        if let Some(prev) = seen.insert(p, c) {
            if prev != c {
                ecb = false;
                break;
            }
        }
    }
    assert!(!ecb, "real transform is not ECB; model must be tweakable/chained");
}


/// Full-HV dump key search (v25 capture).  The DRAM keys are regenerated every
/// boot, so the 256 KiB hypervisor dump and the ciphertext pair must come from
/// the same boot; this searches every 8-byte-aligned 48-byte window of the dump
/// for the whitening/AES/hash key triple, trying all six assignments.  Only the
/// first 16-byte block is decrypted per candidate, so the scan is fast.
#[test]
#[ignore]
fn hv_dump_key_search() {
    use std::collections::BTreeMap;
    let path = root().join("dumps/console/seccap_v25.bin");
    if !path.exists() {
        eprintln!("skipping: {} not present", path.display());
        return;
    }
    let d = std::fs::read(&path).unwrap();
    let version = u32::from_be_bytes(d[4..8].try_into().unwrap());
    assert!(version >= 19, "v25 capture expected, got version {version}");

    // Sparse byte map per region: (region, window) -> offset -> byte.
    let mut maps: BTreeMap<(u32, u32), BTreeMap<usize, u8>> = BTreeMap::new();
    let mut off = 16usize;
    while off + 16 <= d.len() {
        let tag = u32::from_be_bytes(d[off..off + 4].try_into().unwrap());
        let win = u32::from_be_bytes(d[off + 4..off + 8].try_into().unwrap());
        let a = u32::from_be_bytes(d[off + 8..off + 12].try_into().unwrap()) as usize;
        let ln = u32::from_be_bytes(d[off + 12..off + 16].try_into().unwrap()) as usize;
        off += 16;
        let data = &d[off..off + ln];
        off += ln;
        let m = maps.entry((tag & 0xFFF, win)).or_default();
        for (i, b) in data.iter().enumerate() {
            m.insert(a + i, *b);
        }
    }

    let flatten = |m: &BTreeMap<usize, u8>, base: usize, len: usize| -> Vec<u8> {
        (base..base + len).map(|a| *m.get(&a).unwrap_or(&0)).collect()
    };

    let ct = {
        let m = maps.get(&(0, 0)).expect("ciphertext pair missing");
        flatten(m, 0x3B80, 0x1000)
    };
    let pt = {
        let m = maps.get(&(1, 0)).expect("plaintext pair missing");
        flatten(m, 0x3B80, 0x1000)
    };
    // The plaintext view must equal the retail HV image, otherwise the capture
    // is not what we think it is.
    let kernel = std::fs::read(root().join("dumps/stages/kernel_17559.bin")).unwrap();
    assert_eq!(pt, kernel[0x3B80..0x4B80].to_vec(), "plaintext sanity");

    let ct0: [u8; 16] = ct[..16].try_into().unwrap();
    let pt0: [u8; 16] = pt[..16].try_into().unwrap();
    let perms = [(0usize, 1usize, 2usize), (0, 2, 1), (1, 0, 2), (1, 2, 0), (2, 0, 1), (2, 1, 0)];

    let mut tried = 0usize;
    for ((region, win), m) in &maps {
        if *region != 1 {
            continue;
        }
        let lo = *m.keys().next().unwrap();
        let hi = *m.keys().next_back().unwrap();
        let block = flatten(m, lo, hi + 1 - lo);
        let mut p = 0usize;
        while p + 0x30 <= block.len() {
            let k = &block[p..p + 0x30];
            if k.iter().any(|&b| b != 0) {
                let chunks: [[u8; 16]; 3] =
                    core::array::from_fn(|i| k[i * 16..(i + 1) * 16].try_into().unwrap());
                for (a, b, c) in perms {
                    let e = xenon_cpu::security::SecurityEngine::with_keys(
                        0, chunks[a], chunks[b], chunks[c]);
                    tried += 1;
                    if e.xex_decrypt_block(&ct0, 0x3B80) == pt0 {
                        let dec = e.xex_decrypt(&ct, 0x3B80);
                        if dec == pt {
                            eprintln!(
                                "FOUND key at region1 win{win} +0x{:x}: white=chunk{a} aes=chunk{b} hash=chunk{c}",
                                lo + p
                            );
                            eprintln!("white={:02x?}", chunks[a]);
                            eprintln!("aes  ={:02x?}", chunks[b]);
                            eprintln!("hash ={:02x?}", chunks[c]);
                            return;
                        }
                    }
                }
            }
            p += 8;
        }
    }
    panic!("key not found in the HV dump ({tried} candidates tried)");
}
