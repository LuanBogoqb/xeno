//! Checks that need no console dump, so they run in CI.
//!
//! The chain suite (`chain.rs`) needs the owner's `dumps/` and skips when it is
//! absent; everything here works on a clean checkout.

use std::path::PathBuf;
use std::process::Command;
use xenon_cpu::plugins::{PluginList, report};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xeno-cli-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn without_arguments_the_binary_prints_usage_and_exits_two() {
    let out = Command::new(env!("CARGO_BIN_EXE_runcb"))
        .output()
        .expect("run runcb");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage: runcb"), "{stderr}");
}

#[test]
fn launch_ini_lists_console_plugins_and_reports_missing_files() {
    let dir = scratch("plugins");
    let ini = dir.join("launch.ini");
    std::fs::write(
        &ini,
        "[Settings]\nplugin1 = Hdd:\\Ignored.xex\n[Plugins]\nplugin2 = Hdd:\\Plugins\\Proto.xex\n",
    )
    .unwrap();

    let text = std::fs::read_to_string(&ini).unwrap();
    let mut list = PluginList::from_launch_ini(&text, &dir, &dir);
    list.source = Some(ini.clone());
    assert_eq!(list.specs.len(), 1);
    assert_eq!(list.specs[0].slot, 2);
    assert_eq!(
        list.specs[0].path,
        dir.join("hdd").join("Plugins").join("Proto.xex")
    );

    let lines = report(&list);
    assert!(lines[0].contains("Plugins: 1 from"), "{lines:?}");
    assert!(lines[1].contains("file not found or not XEX2"), "{lines:?}");
    std::fs::remove_dir_all(&dir).ok();
}
