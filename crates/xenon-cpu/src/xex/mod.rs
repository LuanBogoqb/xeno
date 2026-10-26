//! XEX2: header, extraction, image layout and import thunks.
//!
//! An XEX is a header, a security header and a base file. The base file is
//! either stored or LZX-compressed and, for retail files, AES-128 encrypted
//! with a key derived from the file key in the security header. `extract`
//! returns that base file (a PE image); `map` places it at its run-time layout
//! (flat when the PE section table is not usable, as in the packed plugins);
//! `patch_imports` resolves the ordinal slots and writes the `lis/addi/mtctr/
//! bctr` stubs over the thunks, as the console loader does.
//!
//! Calling the entry point and mapping into guest RAM depend on the kernel
//! (`xam.xex` and the export tables), so they stay outside this module.

pub mod lzx;

use aes::Aes128;
use cipher::generic_array::GenericArray;
use cipher::{BlockDecrypt, KeyInit};

/// XEX2 container keys: retail and the devkit zero key. These are format
/// constants that every XEX tool ships; they are not console keys.
const RETAIL_KEY: [u8; 16] = [0x20, 0xB1, 0x85, 0xA5, 0x9D, 0x28, 0xFD, 0xC3, 0x40, 0x58, 0x3F, 0xBB, 0x08, 0x96, 0xBF, 0x91];
const DEVKIT_KEY: [u8; 16] = [0; 16];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XexError {
    TooShort,
    BadMagic,
    BadOffset,
    NoBaseFileFormat,
    NoImage,
    TooLarge,
    UnsupportedEncryption(u16),
    UnsupportedCompression(u16),
    BadKey,
    Lzx(lzx::LzxError),
}

impl std::fmt::Display for XexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XexError::TooShort => write!(f, "file too short"),
            XexError::BadMagic => write!(f, "not XEX2"),
            XexError::BadOffset => write!(f, "header offset outside the file"),
            XexError::NoBaseFileFormat => write!(f, "no base file format header"),
            XexError::NoImage => write!(f, "no image size in the security header"),
            XexError::TooLarge => write!(f, "image larger than the declared size"),
            XexError::UnsupportedEncryption(k) => write!(f, "encryption type {k}"),
            XexError::UnsupportedCompression(k) => write!(f, "compression type {k}"),
            XexError::BadKey => write!(f, "file key does not decrypt the base file"),
            XexError::Lzx(e) => write!(f, "LZX: {e:?}"),
        }
    }
}

/// One import entry in the order it appears in the import table. An entry is
/// either an ordinal slot (4 bytes, patched with the address) or a call thunk
/// (16 bytes, patched with a `lis/addi/mtctr/bctr` stub); which one it is only
/// comes from the extracted image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub library: String,
    pub address: u64,
}

/// How the loader resolves an entry: an ordinal slot holds the function
/// address; a call thunk holds the address of the module's import stub, which
/// can differ from the function itself (the console build points some thunks
/// 64 KiB above the function).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Slot,
    Thunk,
}

/// One import entry paired with the ordinal it refers to and the kind read
/// from the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassifiedImport<'a> {
    pub library: &'a str,
    pub address: u64,
    pub ordinal: u16,
    pub kind: ImportKind,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {    pub slots: usize,
    pub thunks: usize,
    /// Entries the resolver could not answer, in table order.
    pub unresolved: Vec<(String, u16)>,
    /// Entries whose address is outside the mapped image.
    pub out_of_range: usize,
}

pub struct Xex {
    file: Vec<u8>,    pe_offset: usize,
    module_flags: u32,
    encryption: u16,
    compression: u16,
    block_size: u32,
    image_size: usize,
    image_base: Option<u64>,
    entry_point: Option<u64>,
    load_address: Option<u32>,
    file_key: [u8; 16],
    import_libraries: Vec<String>,
    imports: Vec<Import>,
}

impl std::fmt::Debug for Xex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Xex")
            .field("file_size", &self.file.len())
            .field("image_size", &self.image_size)
            .field("image_base", &self.image_base)
            .field("entry_point", &self.entry_point)
            .field("encryption", &self.encryption)
            .field("compression", &self.compression)
            .finish()
    }
}

impl Xex {
    pub fn parse(file: Vec<u8>) -> Result<Xex, XexError> {
        if file.len() < 24 {
            return Err(XexError::TooShort);
        }
        if &file[..4] != b"XEX2" {
            return Err(XexError::BadMagic);
        }
        let module_flags = be32(&file, 4)?;
        // Offset 8 is the base file (PE data) offset; 12 is reserved.
        let pe_offset = be32(&file, 8)? as usize;
        let security_offset = be32(&file, 16)? as usize;
        let count = be32(&file, 20)? as usize;

        let mut encryption = 0;
        let mut compression = 0;
        let mut block_size = 0;
        let mut image_base = None;
        let mut entry_point = None;
        let mut import_blob: Option<Vec<u8>> = None;
        for i in 0..count {
            let at = 24 + 8 * i;
            let key = be32(&file, at)?;
            let value = be32(&file, at + 4)?;
            let low = (key & 0xFF) as usize;
            // 0 and 1 carry the value inline; the rest point to data whose length
            // comes from the key (`low * 4`) or from a leading u32 (`0xFF`).
            let blob: Vec<u8> = match low {
                0 | 1 => value.to_be_bytes().to_vec(),
                0xFF => {
                    let len = be32(&file, value as usize)? as usize;
                    slice(&file, value as usize, len)?.to_vec()
                }
                n => slice(&file, value as usize, n * 4)?.to_vec(),
            };
            match key {
                0x3FF => {
                    // base_file_format: size, encryption, compression, chunk, block.
                    if blob.len() < 8 {
                        return Err(XexError::NoBaseFileFormat);
                    }
                    encryption = be16(&blob, 4)?;
                    compression = be16(&blob, 6)?;
                    if blob.len() >= 16 {
                        block_size = be32(&blob, 12)?;
                    }
                }
                0x10100 => entry_point = Some(value as u64),
                0x10201 => image_base = Some(value as u64),
                0x103FF => import_blob = Some(blob),
                _ => {}
            }
        }

        let image_size = be32(&file, security_offset + 4)? as usize;
        let load_address = if security_offset + 276 <= file.len() {
            Some(be32(&file, security_offset + 272)?)
        } else {
            None
        };
        let file_key: [u8; 16] = slice(&file, security_offset + 336, 16)?
            .try_into()
            .map_err(|_| XexError::TooShort)?;

        let (import_libraries, imports) = match import_blob {
            Some(blob) => read_imports(&blob)?,
            None => (Vec::new(), Vec::new()),
        };

        Ok(Xex {
            file,
            pe_offset,
            module_flags,
            encryption,
            compression,
            block_size,
            image_size,
            image_base,
            entry_point,
            load_address,
            file_key,
            import_libraries,
            imports,
        })
    }

    pub fn file_size(&self) -> usize { self.file.len() }
    pub fn pe_offset(&self) -> usize { self.pe_offset }
    pub fn module_flags(&self) -> u32 { self.module_flags }
    pub fn encryption(&self) -> u16 { self.encryption }
    pub fn compression(&self) -> u16 { self.compression }
    pub fn image_size(&self) -> usize { self.image_size }
    pub fn image_base(&self) -> Option<u64> { self.image_base }
    pub fn entry_point(&self) -> Option<u64> { self.entry_point }
    pub fn load_address(&self) -> Option<u32> { self.load_address }
    pub fn import_libraries(&self) -> &[String] { &self.import_libraries }
    pub fn imports(&self) -> &[Import] { &self.imports }

    pub fn compression_name(&self) -> &'static str {
        match self.compression {
            0 => "stored",
            1 => "basic",
            2 => "LZX",
            _ => "unknown",
        }
    }

    /// Decompresses (and decrypts, when needed) the base file into a PE image.
    pub fn extract(&self) -> Result<Vec<u8>, XexError> {
        if self.image_size == 0 {
            return Err(XexError::NoImage);
        }
        let data = match self.encryption {
            0 => {
                let len = self.file.len().checked_sub(self.pe_offset).ok_or(XexError::BadOffset)?;
                slice(&self.file, self.pe_offset, len)?.to_vec()
            }
            _ => self.decrypt_container()?,
        };
        match self.compression {
            0 => {
                if data.len() < self.image_size {
                    return Err(XexError::TooShort);
                }
                Ok(data[..self.image_size].to_vec())
            }
            2 => {
                let chunks = self.lzx_chunks(&data)?;
                lzx::decode(&chunks, self.image_size).map_err(XexError::Lzx)
            }
            other => Err(XexError::UnsupportedCompression(other)),
        }
    }

    fn decrypt_container(&self) -> Result<Vec<u8>, XexError> {
        let len = self.file.len().checked_sub(self.pe_offset).ok_or(XexError::BadOffset)?;
        let data = slice(&self.file, self.pe_offset, len)?;
        for key in [RETAIL_KEY, DEVKIT_KEY] {
            let file_key = ecb_decrypt(&key, &self.file_key);
            let plain = cbc_decrypt(&file_key, data);
            // The devkit key only makes sense when the first block looks like
            // a block header; retail always passes and stops here.
            if self.compression != 2 || be32(&plain, 0).map(|n| n as usize <= plain.len()).unwrap_or(false) {
                return Ok(plain);
            }
        }
        Err(XexError::BadKey)
    }

    fn lzx_chunks(&self, data: &[u8]) -> Result<Vec<Vec<u8>>, XexError> {
        let block = self.block_size as usize;
        if block == 0 {
            return Err(XexError::BadOffset);
        }
        let mut chunks = Vec::new();
        let mut p = 0usize;
        let mut size = block;
        while size != 0 {
            let next = be32(data, p)? as usize;
            let end = (p + size).min(data.len());
            let mut q = p + 24;
            while q < end {
                let len = be16(data, q)? as usize;
                if len == 0 {
                    break;
                }
                let Some(chunk) = data.get(q + 2..q + 2 + len) else {
                    return Err(XexError::TooShort);
                };
                chunks.push(chunk.to_vec());
                q += 2 + len;
            }
            p += size;
            size = next;
        }
        Ok(chunks)
    }

    /// Places the extracted image at its run-time layout. A packed XEX (like
    /// the running Proto) has no usable PE section table: the loader maps the
    /// base file flat at the image base. A normal build uses the section table
    /// and keeps the PE headers before the first section.
    pub fn map(&self, image: &[u8]) -> Result<Vec<u8>, XexError> {
        let mut out = vec![0u8; self.image_size];
        let sections = self.sections(image);
        if let Some(sections) = sections {
            let first_raw = sections
                .iter()
                .map(|s| s.1)
                .min()
                .unwrap_or(0)
                .min(image.len())
                .min(self.image_size);
            out[..first_raw].copy_from_slice(&image[..first_raw]);
            for (va, raw, size) in sections {
                let n = size.min(self.image_size - va).min(image.len() - raw);
                out[va..va + n].copy_from_slice(&image[raw..raw + n]);
            }
        } else {
            if image.len() > self.image_size {
                return Err(XexError::TooLarge);
            }
            out[..image.len()].copy_from_slice(image);
        }
        Ok(out)
    }

    fn sections(&self, image: &[u8]) -> Option<Vec<(usize, usize, usize)>> {
        if image.len() < 0x40 || &image[..2] != b"MZ" {
            return None;
        }
        let e = le32(image, 0x3C)? as usize;
        let count = le16(image, e + 6)? as usize;
        let opt = le16(image, e + 20)? as usize;
        if count == 0 || count > 16 {
            return None;
        }
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let at = e + 24 + opt + 40 * i;
            let vs = le32(image, at + 8)? as usize;
            let va = le32(image, at + 12)? as usize;
            let raw_size = le32(image, at + 16)? as usize;
            let raw = le32(image, at + 20)? as usize;
            if vs == 0 || va >= self.image_size || raw_size == 0 {
                return None;
            }
            if raw.checked_add(raw_size)? > image.len() {
                return None;
            }
            if va + raw_size.min(vs) > self.image_size {
                return None;
            }
            out.push((va, raw, raw_size.min(vs)));
        }
        Some(out)
    }

    /// Counts how many entries are ordinal slots and how many are call thunks,
    /// reading the classification from the image before any patch is applied.
    pub fn classify_imports(&self, image: &[u8]) -> (usize, usize) {
        let mut slots = 0;
        let mut thunks = 0;
        for entry in self.classify_entries(image) {
            match entry.kind {
                ImportKind::Slot => slots += 1,
                ImportKind::Thunk => thunks += 1,
            }
        }
        (slots, thunks)
    }

    /// Walks the import table and pairs each entry with the ordinal it refers
    /// to (a thunk inherits the ordinal of the slot before it), reading the
    /// entry kind from the unpatched image.
    pub fn classify_entries<'a>(&'a self, image: &[u8]) -> Vec<ClassifiedImport<'a>> {
        let base = self.image_base.unwrap_or(0);
        let mut out = Vec::new();
        let mut last: Option<u16> = None;
        for import in &self.imports {
            let Some(offset) = import.address.checked_sub(base).map(|o| o as usize) else {
                continue;
            };
            let Some(word) = read_u32(image, offset) else {
                continue;
            };
            if word >> 24 == 0 {
                let ordinal = (word & 0xFFFF) as u16;
                last = Some(ordinal);
                out.push(ClassifiedImport {
                    library: &import.library,
                    address: import.address,
                    ordinal,
                    kind: ImportKind::Slot,
                });
            } else if let Some(ordinal) = last {
                out.push(ClassifiedImport {
                    library: &import.library,
                    address: import.address,
                    ordinal,
                    kind: ImportKind::Thunk,
                });
            }
        }
        out
    }

    /// Resolves the ordinal slots and call thunks, as the console loader does.
    /// Slots get the resolved address; thunks get `lis r11, hi; addi r11, r11,
    /// lo; mtctr r11; bctr`. Unresolved entries are left untouched and reported.
    pub fn patch_imports<F>(&self, image: &mut [u8], mut resolve: F) -> ImportReport
    where
        F: FnMut(&str, u16, ImportKind) -> Option<u64>,
    {
        let base = self.image_base.unwrap_or(0);
        let mut report = ImportReport::default();
        let mut last: Option<(String, u16)> = None;
        for import in &self.imports {
            let Some(offset) = import.address.checked_sub(base).map(|o| o as usize) else {
                report.out_of_range += 1;
                continue;
            };
            let Some(word) = read_u32(image, offset) else {
                report.out_of_range += 1;
                continue;
            };
            if word >> 24 == 0 {
                // Ordinal slot: the low 16 bits are the ordinal.
                let ordinal = (word & 0xFFFF) as u16;
                last = Some((import.library.clone(), ordinal));
                match resolve(&import.library, ordinal, ImportKind::Slot) {
                    Some(address) => {
                        write_u32(image, offset, address as u32);
                        report.slots += 1;
                    }
                    None => {
                        let pair = (import.library.clone(), ordinal);
                        if !report.unresolved.contains(&pair) {
                            report.unresolved.push(pair);
                        }
                    }
                }
            } else {
                let Some((library, ordinal)) = &last else {
                    report.out_of_range += 1;
                    continue;
                };
                // Keep the library of the slot this thunk belongs to.
                let library = if import.library.is_empty() { library.clone() } else { import.library.clone() };
                match resolve(&library, *ordinal, ImportKind::Thunk) {
                    Some(address) => {
                        write_thunk(image, offset, address);
                        report.thunks += 1;
                    }
                    None => {
                        let pair = (library, *ordinal);
                        if !report.unresolved.contains(&pair) {
                            report.unresolved.push(pair);
                        }
                    }
                }
            }
        }
        report
    }
}

pub(crate) fn read_imports(blob: &[u8]) -> Result<(Vec<String>, Vec<Import>), XexError> {
    if blob.len() < 12 {
        return Ok((Vec::new(), Vec::new()));
    }
    let names_size = be32(blob, 4)? as usize;
    let count = be32(blob, 8)? as usize;
    let names_end = (12 + names_size).min(blob.len());
    let mut libraries = Vec::new();
    let mut o = 12;
    while o < names_end {
        let Some(len) = blob[o..names_end].iter().position(|b| *b == 0) else {
            break;
        };
        if len > 0 {
            libraries.push(String::from_utf8_lossy(&blob[o..o + len]).to_string());
        }
        o += len + 1;
    }
    let mut imports = Vec::new();
    let mut o = 12 + names_size;
    for _ in 0..count {
        if o + 40 > blob.len() {
            break;
        }
        let size = be32(blob, o)? as usize;
        if size < 40 {
            break;
        }
        let name = be16(blob, o + 36)? as usize;
        let n = be16(blob, o + 38)? as usize;
        let Some(library) = libraries.get(name & 0xFF) else {
            break;
        };
        for i in 0..n {
            let at = o + 40 + 4 * i;
            let Some(address) = blob.get(at..at + 4) else {
                break;
            };
            imports.push(Import {
                library: library.clone(),
                address: u32::from_be_bytes(address.try_into().unwrap()) as u64,
            });
        }
        o += size;
    }
    Ok((libraries, imports))
}

fn write_thunk(image: &mut [u8], at: usize, address: u64) {
    let hi = ((address >> 16) & 0xFFFF) as u32;
    let lo = (address & 0xFFFF) as u32;
    write_u32(image, at, 0x3D60_0000 | hi); // lis  r11, hi
    write_u32(image, at + 4, 0x396B_0000 | lo); // addi r11, r11, lo
    write_u32(image, at + 8, 0x7D69_03A6); // mtctr r11
    write_u32(image, at + 12, 0x4E80_0420); // bctr
}

fn read_u32(image: &[u8], at: usize) -> Option<u32> {
    image.get(at..at + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()))
}

fn write_u32(image: &mut [u8], at: usize, value: u32) {
    if let Some(slot) = image.get_mut(at..at + 4) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
}

fn slice(data: &[u8], at: usize, len: usize) -> Result<&[u8], XexError> {
    data.get(at..at + len).ok_or(XexError::BadOffset)
}

fn be16(data: &[u8], at: usize) -> Result<u16, XexError> {
    data.get(at..at + 2)
        .map(|b| u16::from_be_bytes(b.try_into().unwrap()))
        .ok_or(XexError::BadOffset)
}

fn be32(data: &[u8], at: usize) -> Result<u32, XexError> {
    data.get(at..at + 4)
        .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
        .ok_or(XexError::BadOffset)
}

fn le16(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2).map(|b| u16::from_le_bytes(b.try_into().unwrap()))
}

fn le32(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

#[cfg(test)]
mod tests;

fn ecb_decrypt(key: &[u8; 16], block: &[u8; 16]) -> [u8; 16] {
    let cipher = Aes128::new(key.as_slice().into());
    let mut data = GenericArray::from(*block);
    cipher.decrypt_block(&mut data);
    data.into()
}

fn cbc_decrypt(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(key.as_slice().into());
    let mut out = Vec::with_capacity(data.len() & !15);
    let mut previous = [0u8; 16];
    for block in data.chunks_exact(16) {
        let mut current = GenericArray::from(*<&[u8; 16]>::try_from(block).unwrap());
        cipher.decrypt_block(&mut current);
        let plain: [u8; 16] = core::array::from_fn(|i| current[i] ^ previous[i]);
        out.extend_from_slice(&plain);
        previous = *<&[u8; 16]>::try_from(block).unwrap();
    }
    out
}

