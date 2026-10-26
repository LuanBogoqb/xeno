//! Visões cifradas das regiões 1 e 3: cada acesso passa pelo security engine.
//!
//! A leitura ou escrita do guest chega em `1` ou `4` bytes, mas o AES trabalha em
//! blocos de 16. O bloco do DRAM físico é lido e gravado de uma vez por
//! `read_phys_into`/`write_phys_from`; antes eram 16 acessos ao barramento por
//! bloco. O `SecurityEngine` mantém o key schedule pronto.

use super::{Bus, BusError};

const PHYS_DRAM: u64 = 0x8000_0000_0000_0000;

impl Bus {
    /// `None` when the address is not in a security-engine view.
    pub(super) fn cipher_read(&mut self, addr: u64, size: usize) -> Option<Result<u64, BusError>> {
        if !self.cipher_view(addr) {
            return None;
        }
        Some(self.cipher_read_inner(addr, size))
    }

    pub(super) fn cipher_write(&mut self, addr: u64, size: usize, val: u64) -> Option<Result<(), BusError>> {
        if !self.cipher_view(addr) {
            return None;
        }
        Some(self.cipher_write_inner(addr, size, val))
    }

    fn cipher_view(&self, addr: u64) -> bool {
        let region = addr >> 40;
        (region == 1 || region == 3) && self.security_cipher && self.security.is_some()
    }

    fn cipher_read_inner(&mut self, addr: u64, size: usize) -> Result<u64, BusError> {
        let off = addr & 0x3FFF_FFFF;
        let mut result = 0u64;
        let mut done = 0usize;
        while done < size {
            let cur = off + done as u64;
            let block_off = cur & !0xF;
            let in_block = (cur & 0xF) as usize;
            let take = (16 - in_block).min(size - done);
            let mut ct = [0u8; 16];
            self.read_phys_into(PHYS_DRAM + block_off, &mut ct)?;
            let pt = self.security.as_ref().unwrap().xex_decrypt_block(&ct, block_off);
            for byte in &pt[in_block..in_block + take] {
                result = (result << 8) | *byte as u64;
            }
            done += take;
        }
        Ok(result)
    }

    fn cipher_write_inner(&mut self, addr: u64, size: usize, val: u64) -> Result<(), BusError> {
        let off = addr & 0x3FFF_FFFF;
        let mut done = 0usize;
        while done < size {
            let cur = off + done as u64;
            let block_off = cur & !0xF;
            let in_block = (cur & 0xF) as usize;
            let take = (16 - in_block).min(size - done);
            let mut ct = [0u8; 16];
            self.read_phys_into(PHYS_DRAM + block_off, &mut ct)?;
            // Decrypt, patch the bytes covered by this store, encrypt back.
            let new_ct = {
                let engine = self.security.as_ref().unwrap();
                let mut pt = engine.xex_decrypt_block(&ct, block_off);
                for j in 0..take {
                    pt[in_block + j] = (val >> (8 * (size - 1 - done - j))) as u8;
                }
                engine.xex_encrypt_block(&pt, block_off)
            };
            self.write_phys_from(PHYS_DRAM + block_off, &new_ct)?;
            done += take;
        }
        Ok(())
    }
}
