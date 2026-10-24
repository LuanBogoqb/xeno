use xenon_cpu::cpu::{Cpu, Stop};
use xenon_cpu::mem::BusError;
use xenon_cpu::dev::fuses::FuseLines;
use xenon_cpu::hooks::{Action, Hooks};
use xenon_cpu::runner::{run_until, StageEnd};
use xenon_cpu::map::{build_bus, CACHE_WINDOW2_EA, CD_FLASH_OFFSET, CB_B_VM_LOOP, CB_B_VM_OK_EXIT, initial_regs, CB_A_DIGEST_HOOK, CB_A_LOAD, CB_NEXT_FLASH_OFFSET};

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
    let usage = "usage: runcb <CB_A.mem> <FLASH.bin> [--stage cb_a|cb_b] [--max-steps N] [--fuses FUSE.bin] [--log-pc PC:r,r] [addr:len ...]";
    let mut args = std::env::args().skip(1);
    let path = args.next().expect(usage);
    let flash = std::fs::read(args.next().expect(usage)).expect("read flash");
    let flash_copy = flash.clone();
    let (mut stage, mut max_steps, mut dumps) = ("cb_a".to_string(), MAX_STEPS, Vec::new());
    let mut log_pcs: Vec<(u64, Vec<usize>)> = Vec::new();
    let mut fuses_path = "dumps/FUSE.bin".to_string();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--stage" => stage = args.next().expect(usage),
            "--log-pc" => {
                // --log-pc <pc hex>:<r,r,...>
                let spec = args.next().expect(usage);
                let (pc, regs) = spec.split_once(':').expect(usage);
                log_pcs.push((u64::from_str_radix(pc, 16).expect(usage), regs.split(',').map(|r| r.parse().expect(usage)).collect::<Vec<usize>>()));
            }
            "--fuses" => fuses_path = args.next().expect(usage),
            "--max-steps" => max_steps = args.next().expect(usage).parse().expect("--max-steps N"),
            _ => dumps.push(a),
        }
    }
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
    if stage == "cb_b" && matches!(end, StageEnd::Handoff { .. }) {
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
    if !matches!(end, StageEnd::Handoff { .. }) {
        println!("pc={:#x}; words before pc:", cpu.pc);
        for i in (1..=12u64).rev() {
            let a = cpu.pc.wrapping_sub(4 * i);
            println!("  {a:016x}: {:08x}", bus.read(a, 4).unwrap_or(0));
        }
    }
    let _ = steps;
    for spec in dumps {
        let (a, n) = spec.split_once(':').expect("--dump addr:len (hex)");
        let (a, n) = (u64::from_str_radix(a, 16).unwrap(), usize::from_str_radix(n, 16).unwrap());
        let bytes: Vec<u8> = (0..n as u64).map(|i| bus.read(a + i, 1).unwrap_or(0) as u8).collect();
        println!("dump {a:016x}+{n:x}: {}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());
    }
    for (i, r) in cpu.gpr.iter().enumerate() {
        print!("r{i:<2}={r:016x}{}", if i % 4 == 3 { "\n" } else { " " });
    }
    println!("lr={:016x} ctr={:016x} cr={:08x}", cpu.lr, cpu.ctr, cpu.cr);
}
