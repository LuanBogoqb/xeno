//! Kernel plugins, loaded the way Dashlaunch does it.
//!
//! The console reads `launch.ini` and boots the entries of `[Plugins]`
//! (`plugin1` to `plugin5`) as kernel modules. This module reads that
//! configuration, maps the console drive to the local dump tree and reads the
//! XEX header of each file, so the runner can report what a boot would load.
//!
//! What runs today: configuration, path mapping and header parsing. Mapping the
//! image into guest RAM and calling its entry point needs XEX extraction and the
//! kernel export table, which is M6.

use std::path::{Path, PathBuf};

/// Dashlaunch keeps five plugin slots.
pub const MAX_SLOTS: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSpec {
    /// Slot number, as in the `pluginN` key.
    pub slot: u8,
    /// Text from the ini, kept for the report.
    pub raw: String,
    /// Where the file is on this machine.
    pub path: PathBuf,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PluginList {
    pub specs: Vec<PluginSpec>,
    /// Where the list came from: path of the ini, or `None` when given by flag.
    pub source: Option<PathBuf>,
}

impl PluginList {
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// Reads the `[Plugins]` section of a `launch.ini`. Console paths are
    /// resolved with `drive_root`, which stands in for the console drives.
    pub fn from_launch_ini(text: &str, ini_dir: &Path, drive_root: &Path) -> PluginList {
        let mut specs: Vec<PluginSpec> = Vec::new();
        let mut section = String::new();
        for line in text.lines() {
            let line = strip_comment(line).trim();
            if line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.trim().to_ascii_lowercase();
                continue;
            }
            if section != "plugins" {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let Some(slot) = key.strip_prefix("plugin").and_then(|n| n.parse::<u8>().ok()) else {
                continue;
            };
            if slot == 0 || slot > MAX_SLOTS {
                continue;
            }
            let raw = value.trim().trim_matches('"').trim().to_string();
            if raw.is_empty() {
                continue;
            }
            let path = resolve_console_path(&raw, ini_dir, drive_root);
            // The last key in the file wins, as it does in the console loader.
            specs.retain(|s| s.slot != slot);
            specs.push(PluginSpec { slot, raw, path });
        }
        specs.sort_by_key(|s| s.slot);
        PluginList { specs, source: None }
    }

    /// Adds a plugin given directly on the command line, in the next free slot.
    pub fn push_path(&mut self, raw: &str, ini_dir: &Path, drive_root: &Path) {
        if self.specs.len() >= MAX_SLOTS as usize {
            return;
        }
        let slot = (1..=MAX_SLOTS)
            .find(|n| !self.specs.iter().any(|s| s.slot == *n))
            .unwrap_or(MAX_SLOTS);
        let path = resolve_console_path(raw, ini_dir, drive_root);
        self.specs.push(PluginSpec { slot, raw: raw.to_string(), path });
        self.specs.sort_by_key(|s| s.slot);
    }
}

/// Console drives Dashlaunch uses: everything else is taken as a local path.
const VIRTUAL_DRIVES: [&str; 5] = ["hdd", "hdd1", "usb0", "usb1", "flash"];

/// Maps a Dashlaunch path (`Hdd:\Plugins\x.xex`) to the dump tree
/// (`<drive_root>/hdd/Plugins/x.xex`). A path that is not on a console drive is
/// used as given; a relative one is taken from the directory of the ini.
pub fn resolve_console_path(raw: &str, ini_dir: &Path, drive_root: &Path) -> PathBuf {
    let trimmed = raw.trim().trim_matches('"').trim();
    if let Some((drive, rest)) = trimmed.split_once(':') {
        let drive = drive.trim().to_ascii_lowercase();
        if (rest.starts_with('\\') || rest.starts_with('/'))
            && VIRTUAL_DRIVES.contains(&drive.as_str())
        {
            let rest = rest.trim_start_matches(['\\', '/']).replace('\\', "/");
            return drive_root.join(drive).join(rest);
        }
    }
    let literal = Path::new(trimmed);
    if literal.is_absolute() {
        return literal.to_path_buf();
    }
    ini_dir.join(trimmed.replace('\\', "/"))
}

fn strip_comment(line: &str) -> &str {
    let mut end = line.len();
    for (i, c) in line.char_indices() {
        if c == ';' || c == '#' {
            end = i;
            break;
        }
    }
    &line[..end]
}

/// Lines the runner prints for `--launch-ini`, `--plugins-dir` and `--plugin`.
pub fn report(list: &PluginList) -> Vec<String> {
    let mut out = Vec::new();
    if list.is_empty() && list.source.is_none() {
        return out;
    }
    match &list.source {
        Some(p) => out.push(format!("Plugins: {} from {}", list.specs.len(), p.display())),
        None => out.push(format!(
            "Plugins: {} from the command line",
            list.specs.len()
        )),
    }
    for s in &list.specs {
        let info = std::fs::read(&s.path).ok().and_then(|b| read_xex(&b).ok());
        match info {
            Some(x) => {
                let base = x.image_base.or(x.load_address).unwrap_or(0);
                out.push(format!(
                    "  plugin{}  {}  XEX2 encryption={} compression={} entry={:#x} base={base:#x} imports=[{}]",
                    s.slot,
                    s.path.display(),
                    x.encryption,
                    x.compression_name(),
                    x.entry_point.unwrap_or(0),
                    x.import_libraries.join(" ")
                ));
            }
            None => out.push(format!(
                "  plugin{}  {}  (file not found or not XEX2)",
                s.slot,
                s.path.display()
            )),
        }
    }
    if !list.is_empty() {
        out.push(
            "Plugin loader: M6; the list and the headers are read, the images are not mapped or executed"
                .to_string(),
        );
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XexInfo {
    pub size: usize,
    pub header_size: u32,
    /// 0 = unencrypted, 1 = retail key, 2 = devkit key.
    pub encryption: u16,
    /// 0 = stored, 1 = basic, 2 = LZX.
    pub compression: u16,
    pub entry_point: Option<u32>,
    pub image_base: Option<u32>,
    /// `load_address` from the security header.
    pub load_address: Option<u32>,
    pub import_libraries: Vec<String>,
}

impl XexInfo {
    pub fn compression_name(&self) -> &'static str {
        match self.compression {
            0 => "stored",
            1 => "basic",
            2 => "LZX",
            _ => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XexError {
    TooShort,
    BadMagic,
    BadOffset,
}

/// Reads the XEX2 header and the optional headers the loader needs.
pub fn read_xex(bytes: &[u8]) -> Result<XexInfo, XexError> {
    if bytes.len() < 24 {
        return Err(XexError::TooShort);
    }
    if &bytes[..4] != b"XEX2" {
        return Err(XexError::BadMagic);
    }
    let be32 = |o: usize| -> Result<u32, XexError> {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
            .ok_or(XexError::BadOffset)
    };
    let header_size = be32(8)?;
    let security_offset = be32(16)? as usize;
    let count = be32(20)? as usize;

    let mut encryption = 0;
    let mut compression = 0;
    let mut entry_point = None;
    let mut image_base = None;
    let mut import_libraries = Vec::new();

    for i in 0..count {
        let base = 24 + 8 * i;
        let key = be32(base)?;
        let value = be32(base + 4)?;
        let low = (key & 0xFF) as usize;
        // 0 and 1 carry the value inline; the rest point to data whose length
        // comes from the key (`low * 4`) or from a leading u32 (`0xFF`).
        let blob: Vec<u8> = match low {
            0 | 1 => value.to_be_bytes().to_vec(),
            0xFF => {
                let len = be32(value as usize)? as usize;
                bytes
                    .get(value as usize..value as usize + len)
                    .ok_or(XexError::BadOffset)?
                    .to_vec()
            }
            n => bytes
                .get(value as usize..value as usize + n * 4)
                .ok_or(XexError::BadOffset)?
                .to_vec(),
        };
        match key {
            0x3FF => {
                // base_file_format: u32 version, u16 encryption, u16 compression.
                if blob.len() >= 8 {
                    encryption = u16::from_be_bytes([blob[4], blob[5]]);
                    compression = u16::from_be_bytes([blob[6], blob[7]]);
                }
            }
            0x10100 => entry_point = Some(value),
            0x10201 => image_base = Some(value),
            0x103FF => import_libraries = read_import_names(&blob),
            _ => {}
        }
    }

    let load_address = if security_offset + 276 <= bytes.len() {
        Some(u32::from_be_bytes(
            bytes[security_offset + 272..security_offset + 276].try_into().unwrap(),
        ))
    } else {
        None
    };

    Ok(XexInfo {
        size: bytes.len(),
        header_size,
        encryption,
        compression,
        entry_point,
        image_base,
        load_address,
        import_libraries,
    })
}

/// Import libraries blob: u32 size, u32 names size, u32 count, then the names.
fn read_import_names(blob: &[u8]) -> Vec<String> {
    if blob.len() < 12 {
        return Vec::new();
    }
    let names_size = u32::from_be_bytes(blob[4..8].try_into().unwrap()) as usize;
    let count = u32::from_be_bytes(blob[8..12].try_into().unwrap()) as usize;
    let end = (12 + names_size).min(blob.len());
    let mut out = Vec::new();
    let mut o = 12;
    while o < end && out.len() < count {
        let Some(len) = blob[o..end].iter().position(|b| *b == 0) else {
            break;
        };
        if len > 0 {
            out.push(String::from_utf8_lossy(&blob[o..o + len]).to_string());
        }
        o += len + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ini(text: &str) -> PluginList {
        PluginList::from_launch_ini(text, Path::new("dumps/hdd"), Path::new("dumps"))
    }

    #[test]
    fn reads_plugin_slots_in_order() {
        let list = ini(
            "[Plugins]\nplugin2 = Hdd:\\Plugins\\Proto.xex\nplugin1=Hdd:\\Plugins\\GoldSpoof.xex\n",
        );
        assert_eq!(list.specs.len(), 2);
        assert_eq!(list.specs[0].slot, 1);
        assert_eq!(list.specs[1].slot, 2);
        assert_eq!(
            list.specs[0].path,
            Path::new("dumps/hdd/Plugins/GoldSpoof.xex")
        );
    }

    #[test]
    fn ignores_other_sections_and_comments() {
        let list = ini(
            "[Settings]\nplugin1 = Hdd:\\Plugins\\Nope.xex\n; plugin1 = Hdd:\\Plugins\\Commented.xex\n[Plugins]\nplugin1 = Hdd:\\Plugins\\Yes.xex ; a comment\n",
        );
        assert_eq!(list.specs.len(), 1);
        assert_eq!(list.specs[0].raw, "Hdd:\\Plugins\\Yes.xex");
    }

    #[test]
    fn strips_quotes_and_accepts_spacing() {
        let list = ini("[Plugins]\n  Plugin1  =  \"Hdd:\\Plugins\\A.xex\"  \n");
        assert_eq!(list.specs[0].raw, "Hdd:\\Plugins\\A.xex");
    }

    #[test]
    fn last_value_wins_and_beyond_five_is_ignored() {
        let list = ini(
            "[Plugins]\nplugin1 = Hdd:\\A.xex\nplugin1 = Hdd:\\B.xex\nplugin6 = Hdd:\\C.xex\nplugin0 = Hdd:\\D.xex\n",
        );
        assert_eq!(list.specs.len(), 1);
        assert!(list.specs[0].path.ends_with("B.xex"));
    }    #[test]
    fn maps_usb_and_relative_paths() {
        let usb = resolve_console_path("Usb0:\\Plugins\\A.xex", Path::new("ini"), Path::new("dumps"));
        assert_eq!(usb, Path::new("dumps/usb0/Plugins/A.xex"));
        let rel = resolve_console_path("Plugins\\B.xex", Path::new("dumps/hdd"), Path::new("dumps"));
        assert_eq!(rel, Path::new("dumps/hdd/Plugins/B.xex"));
    }

    #[test]
    fn local_path_outside_a_console_drive_is_kept() {
        #[cfg(windows)]
        let raw = "C:\\tools\\Plugins\\A.xex";
        #[cfg(not(windows))]
        let raw = "/opt/Plugins/A.xex";
        let local = resolve_console_path(raw, Path::new("ini"), Path::new("dumps"));
        assert_eq!(local, Path::new(raw));
    }

    #[test]
    fn flag_paths_fill_free_slots() {
        let mut list = ini("[Plugins]\nplugin1 = Hdd:\\A.xex\n");
        list.push_path("dumps/Plugins/B.xex", Path::new("."), Path::new("dumps"));
        assert_eq!(list.specs.len(), 2);
        assert_eq!(list.specs[1].slot, 2);
        for _ in 0..10 {
            list.push_path("dumps/Plugins/C.xex", Path::new("."), Path::new("dumps"));
        }
        assert_eq!(list.specs.len(), MAX_SLOTS as usize);
    }

    fn synthetic_xex(entry: u32, base: u32, encryption: u16, compression: u16) -> Vec<u8> {
        // header: magic, flags, size, reserved, security offset, header count
        let mut b = vec![0u8; 0x4000];
        b[..4].copy_from_slice(b"XEX2");
        b[4..8].copy_from_slice(&1u32.to_be_bytes());
        b[8..12].copy_from_slice(&0x1000u32.to_be_bytes());
        b[16..20].copy_from_slice(&0x2000u32.to_be_bytes());
        b[20..24].copy_from_slice(&3u32.to_be_bytes());
        // three optional headers: base file format, entry point, image base
        for (i, (key, val)) in [(0x0000_03FFu32, 0x1000u32), (0x0001_0100, entry), (0x0001_0201, base)]
            .iter()
            .enumerate()
        {
            let o = 24 + 8 * i;
            b[o..o + 4].copy_from_slice(&key.to_be_bytes());
            b[o + 4..o + 8].copy_from_slice(&val.to_be_bytes());
        }
        // base_file_format at 0x1000: total size, then encryption and compression.
        // The size includes its own four bytes, as the console stores it.
        let bff = [
            (encryption >> 8) as u8,
            encryption as u8,
            (compression >> 8) as u8,
            compression as u8,
        ];
        b[0x1000..0x1004].copy_from_slice(&8u32.to_be_bytes());
        b[0x1004..0x1008].copy_from_slice(&bff);
        // security header at 0x2000
        b[0x2004..0x2008].copy_from_slice(&0x20_0000u32.to_be_bytes());
        b[0x2110..0x2114].copy_from_slice(&0x9185_0000u32.to_be_bytes());
        b
    }

    #[test]
    fn reads_entry_base_and_security() {
        let x = read_xex(&synthetic_xex(0x9188_3800, 0x9185_0000, 1, 2)).unwrap();
        assert_eq!(x.entry_point, Some(0x9188_3800));
        assert_eq!(x.image_base, Some(0x9185_0000));
        assert_eq!(x.load_address, Some(0x9185_0000));
        assert_eq!(x.encryption, 1);
        assert_eq!(x.compression, 2);
        assert_eq!(x.compression_name(), "LZX");
    }

    #[test]
    fn rejects_what_is_not_xex() {
        assert_eq!(read_xex(b"nope"), Err(XexError::TooShort));
        let mut b = synthetic_xex(0, 0, 0, 0);
        b[..4].copy_from_slice(b"XBE2");
        assert_eq!(read_xex(&b), Err(XexError::BadMagic));
    }

    #[test]
    fn reads_import_library_names() {
        let mut blob = Vec::new();
        blob.extend_from_slice(&0u32.to_be_bytes());
        blob.extend_from_slice(&26u32.to_be_bytes());
        blob.extend_from_slice(&2u32.to_be_bytes());
        blob.extend_from_slice(b"xboxkrnl.exe\0xam.xex\0\0\0");
        blob.resize(12 + 26, 0);
        assert_eq!(read_import_names(&blob), vec!["xboxkrnl.exe", "xam.xex"]);
    }

    #[test]
    fn report_lists_slots_and_headers() {
        let dir = std::env::temp_dir().join(format!("xenon-plugins-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let xex = dir.join("GoldSpoof.xex");
        std::fs::write(&xex, synthetic_xex(0x9110_23c0, 0x9110_0000, 1, 2)).unwrap();
        let mut list = PluginList::default();
        list.push_path(xex.to_str().unwrap(), Path::new("."), Path::new("dumps"));
        let lines = report(&list);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "Plugins: 1 from the command line");
        assert!(lines[1].contains("plugin1"));
        assert!(lines[1].contains("encryption=1 compression=LZX"));
        assert!(lines[1].contains("entry=0x911023c0"));
        assert!(lines[2].starts_with("Plugin loader: M6"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn report_flags_a_missing_file() {
        let mut list = PluginList::default();
        list.push_path("dumps/Plugins/nope.xex", Path::new("."), Path::new("dumps"));
        let lines = report(&list);
        assert!(lines[1].contains("file not found or not XEX2"));
    }

    #[test]
    fn empty_list_prints_nothing() {
        assert!(report(&PluginList::default()).is_empty());
    }
}
