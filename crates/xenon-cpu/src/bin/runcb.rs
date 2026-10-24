use std::collections::VecDeque;
use xenon_cpu::cpu::Cpu;
use xenon_cpu::map::{initial_regs, CB_A_LOAD, SRAM_BASE, SRAM_SIZE};
use xenon_cpu::mem::Bus;

const MAX_STEPS: u64 = 50_000_000;

fn main() {
    let path = std::env::args().nth(1).expect("usage: runcb <CB_A.bin>");
    let image = std::fs::read(&path).expect("read CB_A");
    let entry = u32::from_be_bytes(image[8..12].try_into().unwrap()) as u64;

    let mut bus = Bus::new();
    bus.map_ram(SRAM_BASE, SRAM_SIZE);
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
    for (i, r) in cpu.gpr.iter().enumerate() {
        print!("r{i:<2}={r:016x}{}", if i % 4 == 3 { "\n" } else { " " });
    }
    println!("lr={:016x} ctr={:016x} cr={:08x}", cpu.lr, cpu.ctr, cpu.cr);
}
