//! End-to-end checks against the owner's dumps (`dumps/`, not in git): run with `cargo test -- --ignored`.
use std::path::Path;
use std::process::Command;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap()
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
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let hex = text.lines().find(|l| l.starts_with("dump ")).and_then(|l| l.split(": ").nth(1)).expect("dump line");
    let bytes = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect();
    (text, bytes)
}

#[test]
#[ignore]
fn cd_in_ram_matches_stage_file() {
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
