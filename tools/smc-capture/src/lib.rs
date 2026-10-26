#![no_std]

use core::fmt::{self, Write};

/// Fixed-size text record; no allocation or console C runtime is needed.
pub struct Record {
    bytes: [u8; 256],
    len: usize,
}

impl Record {
    pub fn reply(sample: u32, status: u32, bytes: &[u8; 16]) -> Self {
        let mut out = Self {
            bytes: [0; 256],
            len: 0,
        };
        write!(out, "sample={sample} hal_result={status:08x} rx=").unwrap();
        for b in bytes {
            write!(out, "{b:02x}").unwrap();
        }
        writeln!(out, " command_matches={}", bytes[0] == 0x12).unwrap();
        out
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl Write for Record {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self.len.checked_add(value.len()).ok_or(fmt::Error)?;
        let dest = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        dest.copy_from_slice(value.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_reply_keeps_byte_order_and_return_code() {
        let bytes = [0x12, 0x41, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 255];
        assert_eq!(Record::reply(1, 0xC0000001, &bytes).as_bytes(),
            b"sample=1 hal_result=c0000001 rx=124102030405060708090a0b0c0d0eff command_matches=true\n");
    }

    #[test]
    fn wrong_command_is_preserved_and_marked() {
        let out = Record::reply(2, 0, &[0; 16]);
        assert!(out.as_bytes().ends_with(b"command_matches=false\n"));
    }
}
