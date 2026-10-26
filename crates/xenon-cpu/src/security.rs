//! Xbox 360 security-engine model.
use aes::Aes128;
use cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use cipher::generic_array::GenericArray;
use std::collections::HashMap;

const UINT36_MASK: u128 = (1u128 << 36) - 1;
const UINT128_MASK: u128 = u128::MAX;

fn generate_gf2_table(iv: u16, poly: u16) -> [u16; 256] {
    let mut tab = [0u16; 256];
    for i in 0..256u16 {
        let mut crc = iv as u32;
        let mut c = (i as u32) << 8;
        for _ in 0..8 {
            if (crc ^ c) & 0x8000 != 0 {
                crc = (crc << 1) ^ (poly as u32);
            } else {
                crc <<= 1;
            }
            c <<= 1;
        }
        tab[i as usize] = (crc & 0xFFFF) as u16;
    }
    tab
}

static mut GF2_TAB_INIT: bool = false;
static mut GF2_TAB: [u16; 256] = [0u16; 256];

fn gf2_tab() -> &'static [u16; 256] {
    unsafe {
        if !GF2_TAB_INIT {
            GF2_TAB = generate_gf2_table(0, 0x87);
            GF2_TAB_INIT = true;
        }
        &GF2_TAB
    }
}

fn get_tweak_0(white_key: &[u8; 16], address: u64) -> [u8; 16] {
    let key = u128::from_be_bytes(*white_key);
    let addr = (address >> 4) as u64;
    let mut n: u128 = key.wrapping_shl(36);
    let mut of: u64 = (key >> 92) as u64;
    let mut bitlen = 0usize;
    let mut tmp = addr;
    while tmp > 0 { bitlen += 1; tmp >>= 1; }
    for i in 0..bitlen {
        if (addr >> i) & 1 != 0 {
            n ^= key.wrapping_shl(i as u32);
            if i > 0 {
                of ^= (key >> (128 - i)) as u64;
            }
        }
    }
    let tab = gf2_tab();
    let mut o = of & (UINT36_MASK as u64);
    let mut idx = 0usize;
    while o != 0 {
        n ^= (tab[(o & 0xFF) as usize] as u128) << (idx * 8);
        o >>= 8;
        idx += 1;
    }
    (n & UINT128_MASK).to_be_bytes()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct KeySlot {
    pub id: u8,
    pub value: u64,
    pub locked: bool,
}

#[derive(Clone)]
pub struct SecurityEngine {
    master_key: u64,
    slots: [KeySlot; 8],
    tags: HashMap<u64, u64>,
    enabled: bool,
    white_key: [u8; 16],
    aes_key: [u8; 16],
    hash_key: [u8; 16],
}

impl Default for SecurityEngine {
    fn default() -> Self {
        Self::new(0)
    }
}

impl SecurityEngine {
    pub fn new(master_key: u64) -> Self {
        let mut s = Self {
            master_key,
            slots: [KeySlot { id: 0, value: 0, locked: false }; 8],
            tags: HashMap::new(),
            enabled: false,
            white_key: [0x55; 16],
            aes_key: [0x55; 16],
            hash_key: [0x55; 16],
        };
        for (i, slot) in s.slots.iter_mut().enumerate() {
            slot.id = i as u8;
        }
        s
    }

    pub fn with_keys(master_key: u64, white_key: [u8; 16], aes_key: [u8; 16], hash_key: [u8; 16]) -> Self {
        let mut s = Self::new(master_key);
        s.white_key = white_key;
        s.aes_key = aes_key;
        s.hash_key = hash_key;
        s
    }

    pub fn enable(&mut self) { self.enabled = true; }
    pub fn is_enabled(&self) -> bool { self.enabled }

    pub fn load_key(&mut self, slot: usize, value: u64, lock: bool) -> bool {
        if slot >= self.slots.len() { return false; }
        if self.slots[slot].locked { return false; }
        self.slots[slot].value = value ^ self.master_key;
        self.slots[slot].locked = lock;
        true
    }

    pub fn read_key(&self, slot: usize) -> Option<u64> {
        if slot >= self.slots.len() { return None; }
        if self.slots[slot].locked { None } else { Some(self.slots[slot].value ^ self.master_key) }
    }

    pub fn white_key(&self) -> &[u8; 16] { &self.white_key }
    pub fn aes_key(&self) -> &[u8; 16] { &self.aes_key }
    pub fn hash_key(&self) -> &[u8; 16] { &self.hash_key }

    fn region_key(&self, region: u8) -> u64 {
        self.slots.get(region as usize).map(|s| s.value ^ self.master_key).unwrap_or(self.master_key)
    }

    pub fn xex_encrypt_block(&self, plaintext: &[u8; 16], address: u64) -> [u8; 16] {
        let t = get_tweak_0(&self.white_key, address);
        let x: [u8; 16] = core::array::from_fn(|i| plaintext[i] ^ t[i]);
        let cipher = Aes128::new(self.aes_key.as_slice().into());
        let mut block = GenericArray::from(x);
        cipher.decrypt_block(&mut block);
        core::array::from_fn(|i| block[i] ^ t[i])
    }

    pub fn xex_decrypt_block(&self, ciphertext: &[u8; 16], address: u64) -> [u8; 16] {
        let t = get_tweak_0(&self.white_key, address);
        let x: [u8; 16] = core::array::from_fn(|i| ciphertext[i] ^ t[i]);
        let cipher = Aes128::new(self.aes_key.as_slice().into());
        let mut block = GenericArray::from(x);
        cipher.encrypt_block(&mut block);
        core::array::from_fn(|i| block[i] ^ t[i])
    }

    pub fn xex_encrypt(&self, data: &[u8], base_address: u64) -> Vec<u8> {
        assert!(data.len() % 16 == 0, "data must be multiple of 16");
        let mut out = Vec::with_capacity(data.len());
        for i in 0..(data.len() / 16) {
            let addr = base_address + (i as u64) * 16;
            let block: [u8; 16] = data[i * 16..(i + 1) * 16].try_into().unwrap();
            let enc = self.xex_encrypt_block(&block, addr);
            out.extend_from_slice(&enc);
        }
        out
    }

    pub fn xex_decrypt(&self, data: &[u8], base_address: u64) -> Vec<u8> {
        assert!(data.len() % 16 == 0, "data must be multiple of 16");
        let mut out = Vec::with_capacity(data.len());
        for i in 0..(data.len() / 16) {
            let addr = base_address + (i as u64) * 16;
            let block: [u8; 16] = data[i * 16..(i + 1) * 16].try_into().unwrap();
            let dec = self.xex_decrypt_block(&block, addr);
            out.extend_from_slice(&dec);
        }
        out
    }

    pub fn encrypt_page(&self, _region: u8, base: u64, data: &mut [u8]) {
        if !self.enabled { return; }
        if data.len() % 16 != 0 { return; }
        let enc = self.xex_encrypt(data, base);
        data.copy_from_slice(&enc);
    }

    pub fn decrypt_page(&self, _region: u8, base: u64, data: &mut [u8]) {
        if !self.enabled { return; }
        if data.len() % 16 != 0 { return; }
        let dec = self.xex_decrypt(data, base);
        data.copy_from_slice(&dec);
    }

    pub fn tag_page(&self, addr: u64, data: &[u8]) -> u64 {
        // AES-CBC-MAC (retail) using hash_key as the AES key. Replaces the
        // previous XOR/FNV placeholder with a real block-cipher MAC. Still a
        // model: the exact Xenon integrity tag (algorithm, IV, padding) is not
        // validated against hardware and remains unverified.
        use aes::Aes128;
        use cipher::{BlockEncrypt, KeyInit};
        use cipher::generic_array::GenericArray;
        let cipher = Aes128::new(self.hash_key.as_slice().into());
        let mut msg = Vec::with_capacity(8 + data.len() + 16);
        msg.extend_from_slice(&addr.to_be_bytes());
        msg.extend_from_slice(data);
        msg.push(0x80);
        while msg.len() % 16 != 0 {
            msg.push(0);
        }
        let mut iv = [0u8; 16];
        for chunk in msg.chunks(16) {
            for i in 0..16 {
                iv[i] ^= chunk[i];
            }
            let mut block = GenericArray::from(iv);
            cipher.encrypt_block(&mut block);
            iv = block.into();
        }
        u64::from_be_bytes(iv[..8].try_into().unwrap())
    }

    pub fn store_tag(&mut self, page: u64, tag: u64) {
        self.tags.insert(page & !0xFFF, tag);
    }

    pub fn verify_tag(&self, page: u64, data: &[u8]) -> bool {
        if !self.enabled { return true; }
        match self.tags.get(&(page & !0xFFF)) {
            Some(t) => *t == self.tag_page(page, data),
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn key_isolation_and_lock() {
        let mut e = SecurityEngine::new(0x1234);
        assert!(e.load_key(0, 0xABCD, true));
        assert_eq!(e.read_key(0), None);
        assert!(!e.load_key(0, 0, false));
        assert!(e.load_key(1, 0x5678, false));
        assert_eq!(e.read_key(1), Some(0x5678));
    }
    #[test]
    fn gooby_tweak_matches() {
        let wk = [0x55u8; 16];
        let t = get_tweak_0(&wk, 0x3B80);
        assert_eq!(t, [0x55,0x55,0x55,0x55,0x55,0x55,0x55,0x55,0x55,0x55,0x57,0xe0,0x00,0x00,0x6e,0xfb]);
    }
    #[test]
    fn gooby_block_matches() {
        let e = SecurityEngine::with_keys(0, [0x55;16], [0x55;16], [0x55;16]);
        let pt = [0u8;16];
        let ct = e.xex_encrypt_block(&pt, 0x3B80);
        assert_eq!(ct, [0x8c,0x51,0x72,0x17,0x7e,0x69,0x99,0x54,0xe2,0xf8,0xe1,0x13,0x7c,0x69,0xb0,0xbf]);
    }
    #[test]
    fn xex_roundtrip() {
        let e = SecurityEngine::new(0);
        let data: Vec<u8> = (0..64).map(|i| (i * 7 + 3) as u8).collect();
        let enc = e.xex_encrypt(&data, 0x3B80);
        let dec = e.xex_decrypt(&enc, 0x3B80);
        assert_eq!(dec, data);
    }
    #[test]
    fn xex_not_identity() {
        let e = SecurityEngine::new(0);
        let data = vec![0u8; 16];
        let enc = e.xex_encrypt(&data, 0x3B80);
        assert_ne!(enc, data);
    }
    #[test]
    fn disabled_engine_is_passthrough() {
        let e = SecurityEngine::new(0);
        let mut data = vec![7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22];
        let orig = data.clone();
        e.encrypt_page(2, 0, &mut data);
        assert_eq!(data, orig);
    }
    #[test]
    fn encryption_roundtrips_and_tag_detects_change() {
        let mut e = SecurityEngine::new(0x999);
        e.enable();
        let mut data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let orig = data.clone();
        e.encrypt_page(1, 0x1000, &mut data);
        assert_ne!(data, orig);
        let mut dec = data.clone();
        e.decrypt_page(1, 0x1000, &mut dec);
        assert_eq!(dec, orig);
        let tag = e.tag_page(0x1000, &orig);
        e.store_tag(0x1000, tag);
        assert!(e.verify_tag(0x1000, &orig));
        let mut bad = orig.clone();
        bad[0] ^= 1;
        assert!(!e.verify_tag(0x1000, &bad));
    }
    #[test]
    #[ignore]
    fn captured_pair_decryption_with_all_55() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
        let data = std::fs::read(root.join("dumps/console/sec_pair_0x3b80.bin")).unwrap();
        let n = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;
        let ct = &data[12 + n..12 + 2 * n];
        let e = SecurityEngine::with_keys(0, [0x55; 16], [0x55; 16], [0x55; 16]);
        let dec = e.xex_decrypt(ct, 0x3B80);
        let kernel = std::fs::read(root.join("dumps/stages/kernel_17559.bin")).unwrap();
        let expected: Vec<u8> = [0x3B80usize, 0x3F80, 0x4380, 0x4780]
            .iter().flat_map(|o| kernel[*o..*o + 0x400].to_vec()).collect();
        assert_eq!(dec, expected, "all-0x55 decryption should match kernel if HV was modified");
    }
}
