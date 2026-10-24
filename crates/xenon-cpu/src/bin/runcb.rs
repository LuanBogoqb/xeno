use std::collections::VecDeque;
use xenon_cpu::cpu::Cpu;
use xenon_cpu::dev::flash::FlashWindow;
use xenon_cpu::dev::soc::SocRegs;
use xenon_cpu::map::{initial_regs, CB_A_LOAD, CB_NEXT_FLASH_OFFSET, FLASH_BASE, FLASH_SIZE, SOC_BASE, SOC_SIZE, SRAM_BASE, SRAM_SIZE};
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
    let mut hash_ok = true;

    let mut bus = Bus::new();
    bus.map_ram(SRAM_BASE, SRAM_SIZE);
    bus.map_device(FLASH_BASE, FLASH_SIZE, Box::new(FlashWindow::new(flash)));
    bus.map_device(SOC_BASE, SOC_SIZE, Box::new(SocRegs::default()));
    bus.load(CB_A_LOAD, &image);

    let mut cpu = Cpu::new(CB_A_LOAD + entry);
    initial_regs(&mut cpu);

    let mut trace = VecDeque::new();
    let mut steps = 0u64;
    let stop = loop {
        if steps == MAX_STEPS {
            break None;
        }
        let pc = cpu.pc;
        // Scope decision "no glitch": the RGH3 CB_B does not match the digest stored in CB_A (the real
        // console glitches this compare), so force the compare result at the relocated 0x7F0 to "equal".
        if pc == CB_A_LOAD + 0xC000 + 0x7F0 {
            eprintln!("hle: forcing CB_B digest compare to pass (r3 was {:#x})", cpu.gpr[3]);
            hash_ok = cpu.gpr[3] == 0;
            cpu.gpr[3] = 0;
        }
        if pc == handoff_pc {
            println!("HANDOFF pc={pc:#x} hash_ok={hash_ok}");
            break None;
        }
        let word = bus.read(pc, 4).unwrap_or(0);
        if trace.len() == 32 {
            trace.pop_front();
        }
        trace.push_back((pc, word));
        if let Err(s) = cpu.step(&mut bus) {
            break Some(s);
        }
        steps += 1;
    };

    println!("last {} instructions:", trace.len());
    for (pc, w) in &trace {
        println!("  {pc:016x}: {w:08x}");
    }
    println!("steps: {steps}");
    match stop {
        Some(s) => println!("stop: {s:?}"),
        None if steps == MAX_STEPS => println!("stop: step budget exhausted"),
        None => println!("stop: handoff"),
    }
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
