//! Lists the words of a stage (from its entry) that the interpreter does not decode: `<offset hex> <word hex>` per line.
//! Decoding is tested by stepping one word on a scratch bus, so this can't drift from `cpu.rs`.
use xenon_cpu::cpu::{Cpu, Stop};
use xenon_cpu::mem::Bus;

fn main() {
    let path = std::env::args().nth(1).expect("usage: opscan <stage.mem>");
    let d = std::fs::read(path).expect("read stage");
    let entry = u32::from_be_bytes(d[8..12].try_into().unwrap()) as usize;
    for off in (entry..d.len() - 3).step_by(4) {
        let word = u32::from_be_bytes(d[off..off + 4].try_into().unwrap());
        let mut bus = Bus::new();
        bus.map_ram(0x1000, 4);
        bus.load(0x1000, &word.to_be_bytes());
        let mut cpu = Cpu::new(0x1000);
        if let Err(Stop::UnknownOpcode { .. }) = cpu.step(&mut bus) {
            println!("{off:x} {word:08x}");
        }
    }
}
