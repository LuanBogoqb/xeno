use xenon_cpu::cpu::{Cpu, Stop};
use xenon_cpu::mem::BusError;
use xenon_cpu::dev::fuses::FuseLines;
use xenon_cpu::hooks::{Action, Hooks};
use xenon_cpu::runner::{run_until, StageEnd};
use xenon_cpu::map::{build_bus, CACHE_WINDOW2_EA, CD_FLASH_OFFSET, CB_B_VM_LOOP, CB_B_VM_OK_EXIT, initial_regs, CB_A_DIGEST_HOOK, CB_A_LOAD, CB_NEXT_FLASH_OFFSET};

fn parse_security_keys(text: &str) -> Option<([u8; 16], [u8; 16], [u8; 16])> {
    let mut white: Option<[u8; 16]> = None;
    let mut aes: Option<[u8; 16]> = None;
    let mut hash: Option<[u8; 16]> = None;
    for line in text.lines() {
        let Some(i) = line.find("<-") else { continue; };
        let keyname = line[i + 2..].trim().to_lowercase();
        let rest = &line[..i];
        let all: Vec<u8> = rest.split_whitespace().filter_map(|t| u8::from_str_radix(t, 16).ok()).collect();
        if all.len() < 16 { continue; }
        let last16: [u8; 16] = all[all.len() - 16..].try_into().ok()?;
        if keyname.contains("white") { white = Some(last16); }
        else if keyname.contains("aes") { aes = Some(last16); }
        else if keyname.contains("hash") { hash = Some(last16); }
    }
    Some((white?, aes?, hash?))
}

const MAX_STEPS: u64 = 5_000_000;

fn fmt_end(e: &StageEnd) -> String {
    match e {
        StageEnd::Stop(Stop::Unmapped(BusError::Unmapped { addr, size, write })) => {
            format!("Unmapped {} {size} bytes at {addr:#x}", if *write { "write" } else { "read" })
        }
        StageEnd::Stop(Stop::UnknownOpcode { pc, word }) => format!("UnknownOpcode {word:08x} at {pc:#x}"),
        e => format!("{e:?}"),
    }
}

fn main() {
    let usage = "usage: runcb <CB_A.mem> <FLASH.bin> [--stage cb_a|cb_b|cd|hv] [--max-steps N] [--hv-max-steps N] [--hv-threads 1|6] [--fuses FUSE.bin] [--security-engine off|generic|real] [--security-keys FILE] [--xosc-test FILE] [--log-pc PC:r,r] [--cd-stop-pc HEX] [--hv-stop-pc HEX] [addr:len ...]";
    let mut all_args: Vec<String> = std::env::args().skip(1).collect();
    let path = all_args.first().cloned().expect(usage);
    let flash_path = all_args.get(1).cloned().expect(usage);
    let flash = std::fs::read(&flash_path).expect("read flash");
    let flash_copy = flash.clone();
    let rest = all_args.split_off(2);
    let (mut stage, mut max_steps, mut dumps) = ("cb_a".to_string(), MAX_STEPS, Vec::new());
    let mut log_pcs: Vec<(u64, Vec<usize>)> = Vec::new();
    let mut fuses_path = "dumps/FUSE.bin".to_string();
    let mut cd_stop_pc = xenon_cpu::map::CD_HANDOFF_PC;
    let mut hv_stop_pc = u64::MAX;
    let mut hv_max_steps = MAX_STEPS;
    let mut hv_threads = 1;
    let mut security_mode = "auto".to_string();
    let mut security_keys: Option<String> = None;
    let mut xosc_test: Option<String> = None;
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i].clone();
        match a.as_str() {
            "--stage" => { stage = rest[i+1].clone(); i += 1; }
            "--log-pc" => {
                let spec = &rest[i+1];
                let (pc, regs) = spec.split_once(':').expect(usage);
                log_pcs.push((u64::from_str_radix(pc, 16).expect(usage), regs.split(',').map(|r| r.parse().expect(usage)).collect::<Vec<usize>>()));
                i += 1;
            }
            "--fuses" => { fuses_path = rest[i+1].clone(); i += 1; }
            "--cd-stop-pc" => { cd_stop_pc = u64::from_str_radix(&rest[i+1], 16).expect(usage); i += 1; }
            "--hv-stop-pc" => { hv_stop_pc = u64::from_str_radix(&rest[i+1], 16).expect(usage); i += 1; }
            "--hv-max-steps" => { hv_max_steps = rest[i+1].parse().expect("--hv-max-steps N"); i += 1; }
            "--hv-threads" => { hv_threads = rest[i+1].parse::<usize>().expect("--hv-threads 1|6"); assert!(matches!(hv_threads, 1 | 6)); i += 1; }
            "--security-engine" => {
                if i + 1 < rest.len() && matches!(rest[i+1].as_str(), "off" | "generic" | "real" | "auto") {
                    security_mode = rest[i+1].clone();
                    i += 1;
                } else {
                    security_mode = "generic".into();
                }
            }
            "--security-keys" => { security_keys = Some(rest[i+1].clone()); i += 1; }
            "--xosc-test" => { xosc_test = Some(rest[i+1].clone()); i += 1; }
            "--max-steps" => { max_steps = rest[i+1].parse().expect("--max-steps N"); i += 1; }
            _ => dumps.push(a),
        }
        i += 1;
    }
    // Resolve security-engine mode. Default (auto): use real captured keys when
    // available, otherwise the generic all-0x55 model (distribution-safe).
    let default_keys_path = "dumps/console/sec_keys_v25.txt".to_string();
    let keys_path = security_keys.clone().unwrap_or_else(|| default_keys_path.clone());
    let mut real_keys: Option<([u8; 16], [u8; 16], [u8; 16])> = None;
    if security_mode == "auto" || security_mode == "real" {
        if let Ok(text) = std::fs::read_to_string(&keys_path) {
            real_keys = parse_security_keys(&text);
        }
    }
    let (security_mode, security_log) = match security_mode.as_str() {
        "off" => ("off".to_string(), "disabled (plaintext-backed)".to_string()),
        "generic" => ("generic".to_string(), "generic (all-0x55)".to_string()),
        "real" => {
            if real_keys.is_some() {
                ("real".to_string(), format!("real (keys from {})", keys_path))
            } else {
                ("generic".to_string(), format!("generic (all-0x55; real keys unavailable at {})", keys_path))
            }
        }
        _ => { // auto
            if real_keys.is_some() {
                ("real".to_string(), format!("real (keys from {})", keys_path))
            } else {
                ("generic".to_string(), "generic (all-0x55)".to_string())
            }
        }
    };
    let image = std::fs::read(&path).expect("read CB_A");
    let entry = u32::from_be_bytes(image[8..12].try_into().unwrap()) as u64;

    let next = CB_NEXT_FLASH_OFFSET as usize;
    let next_entry = u32::from_be_bytes(flash[next + 8..next + 12].try_into().unwrap()) as u64;
    let handoff_pc = 0x0200_0000 + (next_entry & 0xFFFF);

    let mut bus = build_bus(flash, FuseLines::from_text(&std::fs::read_to_string(&fuses_path).unwrap_or_default()));
    bus.load(CB_A_LOAD, &image);

    let mut cpu = Cpu::new(CB_A_LOAD + entry);
    initial_regs(&mut cpu);

    let mut hooks = Hooks::new();
    // Scope decision "no glitch": the RGH3 CB_B does not match the digest stored in CB_A (the real
    // console glitches this compare), so force the compare result to "equal".
    hooks.add(CB_A_DIGEST_HOOK, Action::SetGpr { reg: 3, val: 0 });

    let (mut end, mut steps) = run_until(&mut cpu, &mut bus, &hooks, handoff_pc, MAX_STEPS);
    let hash_ok = hooks.overrides().is_empty();
    match &end {
        StageEnd::Handoff { pc } => println!("HANDOFF pc={pc:#x} hash_ok={hash_ok}"),
        e => println!("end: {e:?}"),
    }
    println!("steps: {steps}");
    let depth = ["cb_a", "cb_b", "cd", "hv"].iter().position(|n| *n == stage).expect(usage);
    assert!(depth < 3 || cd_stop_pc == xenon_cpu::map::CD_HANDOFF_PC,
        "--stage hv requires the final CD handoff (no internal --cd-stop-pc)");
    if depth >= 1 && matches!(end, StageEnd::Handoff { .. }) {
        // The RGH3 image stores CB_B in plaintext, so CB_A's RC4 pass left noise in the SRAM: put the real
        // image where CB_A would have left it (decisions.md 3, no glitch).
        let size = u32::from_be_bytes(flash_copy[next + 12..next + 16].try_into().unwrap()) as usize;
        bus.load(CB_A_LOAD, &flash_copy[next..next + size]);
        hooks.add(CB_B_VM_LOOP, Action::Jump(CB_B_VM_OK_EXIT));
        for (pc, regs) in log_pcs {
            hooks.add(pc, Action::LogRegs(regs));
        }
        // CB_B jumps to 0x0400_0000 + (CD entry & 0xFFFF), the same shape as the CB_A handoff
        let cd = CD_FLASH_OFFSET as usize;
        let cd_entry = u32::from_be_bytes(flash_copy[cd + 8..cd + 12].try_into().unwrap()) as u64;
        let (e2, s2) = run_until(&mut cpu, &mut bus, &hooks, CACHE_WINDOW2_EA + (cd_entry & 0xFFFF), max_steps);
        match &e2 {
            StageEnd::Handoff { pc } => println!("HANDOFF pc={pc:#x} digest_overrides={}", hooks.overrides().len()),
            e => println!("CB_B end: {}", fmt_end(e)),
        }
        println!("CB_B steps: {s2}");
        (end, steps) = (e2, steps + s2);
    }
    if depth >= 2 && matches!(end, StageEnd::Handoff { .. }) {
        let (e3, s3) = run_until(&mut cpu, &mut bus, &hooks, cd_stop_pc, max_steps);
        println!("CD end: {}
CD steps: {s3}", fmt_end(&e3));
        (end, steps) = (e3, steps + s3);
    }
    if depth >= 3 && matches!(end, StageEnd::Handoff { .. }) {
        match security_mode.as_str() {
            "off" => {
                println!("HV model: {hv_threads} thread(s), security engine disabled (plaintext-backed, no integrity/encryption)");
            }
            "generic" => {
                bus.enable_security_engine(0);
                bus.enable_security_cipher();
                println!("HV model: {hv_threads} thread(s), security engine generic (all-0x55, cipher on data path)");
            }
            "real" => {
                if let Some((w, a, h)) = real_keys {
                    bus.enable_security_engine_with_keys(0, w, a, h);
                    bus.enable_security_cipher();
                    println!("HV model: {hv_threads} thread(s), security engine real ({}, cipher on data path)", security_log);
                } else {
                    bus.enable_security_engine(0);
                    bus.enable_security_cipher();
                    println!("HV model: {hv_threads} thread(s), security engine generic (all-0x55; real keys unavailable, cipher on data path)");
                }
            }
            _ => {}
        }
        println!("HV security path: {}", security_log);
        bus.isolate_hashed_ram_plaintext(0x9000_0000_0000_0000);
        bus.enable_software_mmu(cpu.slb);
        let (e4, s4) = if hv_threads == 6 {
            println!("HV topology: experimental 3 PPE / 6 threads, IIC enabled");
            let mut machine = xenon_cpu::multicore::Machine::from_cd(cpu, &mut bus);
            let result = machine.run(&mut bus, &hooks, hv_stop_pc, hv_max_steps);
            for (t, c) in machine.cpus.iter().enumerate() {
                let dec = c.spr.get(&0x16).map(|v| format!("{v:#x}")).unwrap_or_else(|| "not-programmed".into());
                println!("CPU{t}: pc={:#x} msr={:#x} steps={} ctrl={:#x} dec={dec}", c.pc, c.msr, machine.steps[t], bus.ctrl_value(t / 2));
            }
            cpu = machine.cpus.into_iter().nth(machine.stopped_thread.unwrap_or(0)).unwrap();
            result
        } else { run_until(&mut cpu, &mut bus, &hooks, hv_stop_pc, hv_max_steps) };
        println!("HV end: {}\nHV steps: {s4}", fmt_end(&e4));
        (end, steps) = (e4, steps + s4);
    }
    if !matches!(end, StageEnd::Handoff { .. }) {
        println!("pc={:#x}; words before pc:", cpu.pc);
        for i in (1..=12u64).rev() {
            let a = cpu.pc.wrapping_sub(4 * i);
            println!("  {a:016x}: {:08x}", bus.read(a, 4).unwrap_or(0));
        }
        println!("TLB entries ({}):", bus.tlb_entries().len());
        for e in bus.tlb_entries() {
            println!("  ea={:#x} size={:#x} target={:#x}", e.0, e.1, e.2);
        }
    }
    let _ = steps;
    if depth >= 3 {
        println!("SRR0={:#x} SRR1={:#x} DAR={:#x} DSISR={:#x}",
            cpu.spr.get(&0x1A).copied().unwrap_or(0), cpu.spr.get(&0x1B).copied().unwrap_or(0),
            cpu.spr.get(&0x13).copied().unwrap_or(0), cpu.spr.get(&0x12).copied().unwrap_or(0));
        println!("HID6={:#x}; software TLB (last 8):", cpu.spr.get(&0x3F9).copied().unwrap_or(0));
        for (index, rpn, vpn) in bus.software_tlb_entries().iter().rev().take(8) {
            println!("  index={index:#x} rpn={rpn:#x} vpn={vpn:#x}");
        }
        for (rb, rs) in cpu.slb.iter().filter(|(rb, _)| rb & 0x0800_0000 != 0) {
            println!("  SLB rb={rb:#x} rs={rs:#x}");
        }
    }
    if let Some(path) = &xosc_test {
        use std::fs;
        let data = fs::read(path).expect("read xosc challenge file");
        // Challenge format: 16 bytes nonce + 4 bytes flags (big-endian). CPU key
        // is a placeholder until a real console vector is available.
        let mut nonce = [0u8; 16];
        if data.len() >= 16 { nonce.copy_from_slice(&data[..16]); }
        let flags = if data.len() >= 20 {
            u32::from_be_bytes(data[16..20].try_into().unwrap())
        } else { 0 };
        let challenge = xenon_cpu::xosc::XoscChallenge { nonce, flags };
        let engine = xenon_cpu::xosc::XoscEngine::new(0x0123_4567_89AB_CDEF);
        let response = engine.respond(&challenge);
        println!("XOSC challenge: nonce={} flags={:#x}", nonce.iter().map(|b| format!("{b:02x}")).collect::<String>(), flags);
        println!("XOSC response: status={:#x} digest={}", response.status,
            response.digest.iter().map(|b| format!("{b:02x}")).collect::<String>());
        if data.len() >= 52 {
            let expected = &data[20..52];
            let ok = expected == response.digest;
            println!("XOSC compare: {}", if ok { "MATCH" } else { "MISMATCH" });
        }
    }
    for spec in dumps {
        let (a, n) = spec.split_once(':').expect("--dump addr:len (hex)");
        let (a, n) = (u64::from_str_radix(a, 16).unwrap(), usize::from_str_radix(n, 16).unwrap());
        let bytes: Vec<u8> = (0..n as u64).map(|i| bus.read(a + i, 1).unwrap_or(0) as u8).collect();
        println!("dump {a:016x}+{n:x}: {}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());
    }
    for (i, r) in cpu.gpr.iter().enumerate() {
        print!("r{i:<2}={r:016x}{}", if i % 4 == 3 { "\n" } else { " " });
    }
    println!("lr={:016x} ctr={:016x} cr={:08x} msr={:016x} hrmor={:x}", cpu.lr, cpu.ctr, cpu.cr, cpu.msr, bus.hrmor);
}
