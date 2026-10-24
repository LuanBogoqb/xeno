use xenon_cpu::cpu::Cpu;
use xenon_cpu::dev::flash::FlashWindow;
use xenon_cpu::dev::soc::SocRegs;
use xenon_cpu::hooks::{Action, Hooks};
use xenon_cpu::runner::{run_until, StageEnd};
use xenon_cpu::map::{CB_A_DIGEST_HOOK, SRAM_ALIAS, initial_regs, CB_A_LOAD, CB_NEXT_FLASH_OFFSET, FLASH_BASE, FLASH_SIZE, SOC_BASE, SOC_SIZE, SRAM_BASE, SRAM_SIZE};
use xenon_cpu::mem::Bus;

const MAX_STEPS: u64 = 5_000_000;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: runcb <CB_A.bin> <FLASH.bin>");
    let flash = std::fs::read(args.next().expect("usage: runcb <CB_A.bin> <FLASH.bin>")).expect("read flash");
    let image = std::fs::read(&path).expect("read CB_A");
    let entry = u32::from_be_bytes(image[8..12].try_into().unwrap()) as u64;

    let next = CB_NEXT_FLASH_OFFSET as usize;
    let next_entry = u32::from_be_bytes(flash[next + 8..next + 12].try_into().unwrap()) as u64;
    let handoff_pc = 0x0200_0000 + (next_entry & 0xFFFF);

    let mut bus = Bus::new();
    bus.map_ram(SRAM_BASE, SRAM_SIZE);
    bus.map_device(FLASH_BASE, FLASH_SIZE, Box::new(FlashWindow::new(flash)));
    bus.map_device(SOC_BASE, SOC_SIZE, Box::new(SocRegs::default()));
    bus.load(CB_A_LOAD, &image);

    let mut cpu = Cpu::new(CB_A_LOAD + entry);
    initial_regs(&mut cpu);

    bus.map_alias(SRAM_ALIAS, 0x1_0000, CB_A_LOAD);
    let mut hooks = Hooks::new();
    // Scope decision "no glitch": the RGH3 CB_B does not match the digest stored in CB_A (the real
    // console glitches this compare), so force the compare result to "equal".
    hooks.add(CB_A_DIGEST_HOOK, Action::SetGpr { reg: 3, val: 0 });

    let (end, steps) = run_until(&mut cpu, &mut bus, &hooks, handoff_pc, MAX_STEPS);
    let hash_ok = hooks.overrides().is_empty();
    match &end {
        StageEnd::Handoff { pc } => println!("HANDOFF pc={pc:#x} hash_ok={hash_ok}"),
        e => println!("end: {e:?}"),
    }
    println!("steps: {steps}");
    for spec in args {
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
