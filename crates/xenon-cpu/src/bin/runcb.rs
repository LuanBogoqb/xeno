use std::collections::VecDeque;
use xenon_cpu::cpu::Cpu;
use xenon_cpu::dev::flash::FlashWindow;
use xenon_cpu::dev::soc::SocRegs;
use xenon_cpu::map::{initial_regs, CB_A_LOAD, FLASH_BASE, FLASH_SIZE, SOC_BASE, SOC_SIZE, SRAM_BASE, SRAM_SIZE};
use xenon_cpu::mem::Bus;

const MAX_STEPS: u64 = 5_000_000;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: runcb <CB_A.bin> <FLASH.bin>");
    let flash = std::fs::read(args.next().expect("usage: runcb <CB_A.bin> <FLASH.bin>")).expect("read flash");
    let image = std::fs::read(&path).expect("read CB_A");
    let entry = u32::from_be_bytes(image[8..12].try_into().unwrap()) as u64;

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
        None => println!("stop: step budget exhausted"),
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
