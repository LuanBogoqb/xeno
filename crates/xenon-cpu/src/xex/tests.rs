use super::*;

/// Header (0x400), base file at 0x400 and security header at 0x1000.
fn build_xex(compression: u16, image: &[u8]) -> Vec<u8> {
    let mut file = vec![0u8; 0x1200];
    file[..4].copy_from_slice(b"XEX2");
    file[4..8].copy_from_slice(&0xEu32.to_be_bytes());
    file[8..12].copy_from_slice(&0x400u32.to_be_bytes()); // base file offset
    file[12..16].copy_from_slice(&0u32.to_be_bytes()); // reserved
    file[16..20].copy_from_slice(&0x1000u32.to_be_bytes());
    file[20..24].copy_from_slice(&3u32.to_be_bytes());
    // base_file_format at 0x200: size, version, encryption, compression,
    // chunk size, block size.
    file[0x200..0x204].copy_from_slice(&16u32.to_be_bytes());
    file[0x204..0x206].copy_from_slice(&0u16.to_be_bytes()); // encryption
    file[0x206..0x208].copy_from_slice(&compression.to_be_bytes());
    file[0x208..0x20C].copy_from_slice(&0x8000u32.to_be_bytes()); // chunk size
    file[0x20C..0x210].copy_from_slice(&0u32.to_be_bytes()); // block size
    for (i, (key, value)) in [
        (0x0000_03FFu32, 0x200u32),
        (0x0001_0100, 0x9185_0100),
        (0x0001_0201, 0x9185_0000),
    ]
    .iter()
    .enumerate()
    {
        let at = 24 + 8 * i;
        file[at..at + 4].copy_from_slice(&key.to_be_bytes());
        file[at + 4..at + 8].copy_from_slice(&value.to_be_bytes());
    }
    // Security header: image size at +4 and load address at +272.
    file[0x1004..0x1008].copy_from_slice(&(image.len() as u32).to_be_bytes());
    file[0x1110..0x1114].copy_from_slice(&0x9185_0000u32.to_be_bytes());
    file[0x400..0x400 + image.len()].copy_from_slice(image);
    file
}

fn parse(image: &[u8], compression: u16) -> Xex {
    Xex::parse(build_xex(compression, image)).unwrap()
}

#[test]
fn parses_the_header_and_a_stored_image() {
    let image = vec![0xAAu8; 0x200];
    let x = parse(&image, 0);
    assert_eq!(x.image_size(), 0x200);
    assert_eq!(x.entry_point(), Some(0x9185_0100));
    assert_eq!(x.image_base(), Some(0x9185_0000));
    assert_eq!(x.compression_name(), "stored");
    assert_eq!(x.extract().unwrap(), image);
}

#[test]
fn rejects_what_is_not_xex() {
    assert_eq!(Xex::parse(b"nope".to_vec()).unwrap_err(), XexError::TooShort);
    let mut file = build_xex(0, &[0; 16]);
    file[..4].copy_from_slice(b"XBE2");
    assert_eq!(Xex::parse(file).unwrap_err(), XexError::BadMagic);
}

#[test]
fn maps_a_flat_image_when_the_section_table_is_unusable() {
    let image = vec![0x11u8; 0x200];
    let x = parse(&image, 0);
    assert_eq!(x.map(&x.extract().unwrap()).unwrap(), image);
}

#[test]
fn maps_sections_to_their_virtual_addresses() {
    let mut image = vec![0u8; 0x400];
    image[..2].copy_from_slice(b"MZ");
    image[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    image[0x80..0x84].copy_from_slice(b"PE\0\0");
    image[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
    image[0x94..0x96].copy_from_slice(&0xE0u16.to_le_bytes());
    let section = 0x80 + 24 + 0xE0;
    image[section..section + 8].copy_from_slice(b".text\0\0\0");
    image[section + 8..section + 12].copy_from_slice(&0x100u32.to_le_bytes());
    image[section + 12..section + 16].copy_from_slice(&0x200u32.to_le_bytes());
    image[section + 16..section + 20].copy_from_slice(&0x100u32.to_le_bytes());
    image[section + 20..section + 24].copy_from_slice(&0x300u32.to_le_bytes());
    for (i, byte) in image[0x300..0x400].iter_mut().enumerate() {
        *byte = i as u8;
    }
    let x = parse(&image, 0);
    let mapped = x.map(&image).unwrap();
    assert_eq!(mapped.len(), 0x400);
    assert_eq!(&mapped[0..2], b"MZ");
    assert_eq!(&mapped[0x200..0x300], &image[0x300..0x400]);
}

/// A slot (ordinal 0x44E at 0x100) and a call thunk (at 0x200).
fn xex_with_imports() -> (Xex, Vec<u8>) {
    let mut image = vec![0u8; 0x400];
    image[..2].copy_from_slice(b"MZ");
    image[0x100..0x104].copy_from_slice(&0x0000_044Eu32.to_be_bytes());
    for (i, word) in [0x0100_044Eu32, 0x0200_044E, 0x7D69_03A6, 0x4E80_0420]
        .iter()
        .enumerate()
    {
        let at = 0x200 + 4 * i;
        image[at..at + 4].copy_from_slice(&word.to_be_bytes());
    }

    let mut blob = Vec::new();
    blob.extend_from_slice(&0u32.to_be_bytes()); // total size, patched below
    blob.extend_from_slice(&10u32.to_be_bytes()); // names size
    blob.extend_from_slice(&1u32.to_be_bytes()); // record count
    blob.extend_from_slice(b"xam.xex\0\0\0");
    let record = blob.len();
    blob.extend_from_slice(&48u32.to_be_bytes()); // record size
    blob.resize(record + 40, 0); // record header, flags and padding
    blob[record + 36..record + 38].copy_from_slice(&0u16.to_be_bytes()); // name index
    blob[record + 38..record + 40].copy_from_slice(&2u16.to_be_bytes()); // entries
    blob.extend_from_slice(&0x9185_0100u32.to_be_bytes());
    blob.extend_from_slice(&0x9185_0200u32.to_be_bytes());
    let blob_len = blob.len() as u32;
    blob[..4].copy_from_slice(&blob_len.to_be_bytes());

    let mut file = build_xex(0, &image);
    file[20..24].copy_from_slice(&4u32.to_be_bytes());
    let at = 24 + 8 * 3;
    let blob_at = file.len() as u32;
    file[at..at + 4].copy_from_slice(&0x0001_03FFu32.to_be_bytes());
    file[at + 4..at + 8].copy_from_slice(&blob_at.to_be_bytes());
    file.extend_from_slice(&blob);
    (Xex::parse(file).unwrap(), image)
}

#[test]
fn patches_slots_and_thunks() {
    let (x, image) = xex_with_imports();
    assert_eq!(x.imports().len(), 2);
    let mut mapped = x.map(&image).unwrap();
    let report = x.patch_imports(&mut mapped, |library, ordinal, kind| {
        assert_eq!(library, "xam.xex");
        assert_eq!(ordinal, 0x44E);
        Some(match kind {
            ImportKind::Slot => 0x8171_C928,
            ImportKind::Thunk => 0x8172_C928,
        })
    });
    assert_eq!(report.slots, 1);
    assert_eq!(report.thunks, 1);
    assert!(report.unresolved.is_empty());
    assert_eq!(read_u32(&mapped, 0x100), Some(0x8171_C928));
    assert_eq!(read_u32(&mapped, 0x200), Some(0x3D60_8172));
    assert_eq!(read_u32(&mapped, 0x204), Some(0x396B_C928));
    assert_eq!(read_u32(&mapped, 0x208), Some(0x7D69_03A6));
    assert_eq!(read_u32(&mapped, 0x20C), Some(0x4E80_0420));
}

#[test]
fn leaves_unresolved_entries_untouched() {
    let (x, image) = xex_with_imports();
    let mut mapped = x.map(&image).unwrap();
    let before = mapped.clone();
    let report = x.patch_imports(&mut mapped, |_, _, _| None);
    assert_eq!(report.slots, 0);
    assert_eq!(report.thunks, 0);
    assert_eq!(report.unresolved, vec![("xam.xex".to_string(), 0x44E)]);
    assert_eq!(mapped, before);
}

/// Checks against the owner's dumps: run with `cargo test -- --ignored`.
mod real {
    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
    }

    fn read(path: PathBuf) -> Option<Vec<u8>> {
        match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(_) => {
                eprintln!("skipping: {} not present", path.display());
                None
            }
        }
    }

    fn console_build() -> Option<(Xex, Vec<u8>, Vec<u8>)> {
        let xex = read(root().join("dumps/console/Proto_console.xex"))?;
        let live = read(root().join("dumps/console/CAP00/proto.mem"))?;
        let x = Xex::parse(xex).unwrap();
        let image = x.map(&x.extract().unwrap()).unwrap();
        Some((x, image, live))
    }

    #[test]
    #[ignore]
    fn console_build_extracts_like_the_live_capture() {
        let Some((x, image, live)) = console_build() else { return };
        assert_eq!(x.file_size(), 151_552);
        assert_eq!(x.image_size(), 0x5F000);
        assert_eq!(x.image_base(), Some(0x9185_0000));
        assert_eq!(x.entry_point(), Some(0x9187_3910));
        assert_eq!(x.encryption(), 0);
        assert_eq!(x.compression(), 2);
        assert_eq!(x.imports().len(), 357);
        assert_eq!(image.len(), live.len());

        // The plugin unpacks itself at run time: .text and .rdata differ from
        // the file. .pdata is unwind metadata and is not touched; neither is
        // the entry stub. Both must match the capture byte for byte.
        assert_eq!(&image[0x7400..0x9200], &live[0x7400..0x9200], ".pdata");
        assert_eq!(&image[0x23910..0x23920], &live[0x23910..0x23920], "entry stub");
        let (slots, thunks) = x.classify_imports(&image);
        assert_eq!((slots, thunks), (181, 176));
    }

    #[test]
    #[ignore]
    fn console_build_import_patches_match_the_live_capture() {
        let Some((x, image, live)) = console_build() else { return };
        let base = x.image_base().unwrap();
        // The capture already has every ordinal resolved; use it as the
        // resolver so only our patch layout is under test.
        let mut slots: HashMap<(String, u16), u64> = HashMap::new();
        let mut thunks: HashMap<(String, u16), u64> = HashMap::new();
        for entry in x.classify_entries(&image) {
            let at = (entry.address - base) as usize;
            let value = if entry.kind == ImportKind::Slot {
                read_u32(&live, at).unwrap() as u64
            } else {
                let hi = read_u32(&live, at).unwrap() & 0xFFFF;
                let lo = read_u32(&live, at + 4).unwrap() & 0xFFFF;
                ((hi << 16) | lo) as u64
            };
            let key = (entry.library.to_string(), entry.ordinal);
            if entry.kind == ImportKind::Slot {
                slots.insert(key, value);
            } else {
                thunks.insert(key, value);
            }
        }
        let mut patched = image.clone();
        let report = x.patch_imports(&mut patched, |library, ordinal, kind| {
            let key = (library.to_string(), ordinal);
            match kind {
                ImportKind::Slot => slots.get(&key).copied(),
                ImportKind::Thunk => thunks.get(&key).copied(),
            }
        });
        assert_eq!(report.slots, 181);
        assert_eq!(report.thunks, 176);
        assert!(report.unresolved.is_empty(), "{:?}", report.unresolved);
        for import in x.imports() {
            let at = (import.address - base) as usize;
            let word = read_u32(&image, at).unwrap();
            let len = if word >> 24 == 0 { 4 } else { 16 };
            assert_eq!(&patched[at..at + len], &live[at..at + len], "import at {at:#x}");
        }
    }

    #[test]
    #[ignore]
    fn encrypted_sample_build_extracts_and_maps_sections() {
        let Some(bytes) = read(root().join("samples/Proto.xex")) else { return };
        let x = Xex::parse(bytes).unwrap();
        assert_eq!(x.file_size(), 247_808);
        assert_eq!(x.image_size(), 499_712);
        assert_eq!(x.entry_point(), Some(0x9188_3800));
        assert_eq!(x.encryption(), 1);
        assert_eq!(x.compression(), 2);
        assert_eq!(x.imports().len(), 271);
        let image = x.extract().unwrap();
        // mflr r12; stw r12,-8(r1): the standard prologue at the entry point.
        assert_eq!(&image[0x33800..0x33808], &[0x7d, 0x88, 0x02, 0xa6, 0x91, 0x81, 0xff, 0xf8]);
        let mapped = x.map(&image).unwrap();
        // .text is at 0x6000 in memory and at 0x5c00 in the file.
        assert_eq!(&mapped[0x6000..0x6010], &image[0x5c00..0x5c10]);
    }
}