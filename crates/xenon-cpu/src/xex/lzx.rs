//! Xbox 360 LZX, the format used by XEX2: 32 KiB frames, no stream reset.
//!
//! Port of `tools/xbolib/lzx.py`, which decodes the boot chain and real XEX
//! files. Each XEX block carries one 32 KiB frame split into 16-bit-length
//! sub-chunks; `decode` consumes the sub-chunks in order and keeps the Huffman
//! tables and the window across frames, as the console loader does.
//!
//! The optional `prefix` window and the E8 call-target translation are part of
//! the same format and are used by the CG delta, not by XEX extraction.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LzxError {
    Truncated,
    BadCode,
    BadBlockType(u32),
    BadWindow(u32),
    Distance,
}

struct Bits<'a> {
    data: &'a [u8],
    /// Bit position; bits are read from 16-bit little-endian words, MSB first.
    bit: usize,
    odd: bool,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Bits<'a> {
        Bits { data, bit: 0, odd: false }
    }

    fn bits(&mut self, n: u32) -> Result<u32, LzxError> {
        let mut v = 0u32;
        for _ in 0..n {
            let word_index = self.bit >> 4;
            let word = if 2 * word_index + 1 < self.data.len() {
                self.data[2 * word_index] as u32 | ((self.data[2 * word_index + 1] as u32) << 8)
            } else {
                0
            };
            v = (v << 1) | ((word >> (15 - (self.bit & 15))) & 1);
            self.bit += 1;
        }
        Ok(v)
    }

    fn align(&mut self) {
        self.bit = (self.bit + 15) & !15;
    }
}

/// Canonical Huffman decoder built from per-symbol code lengths.
struct Huffman {
    max: u32,
    first_code: [u32; 18],
    first_index: [u32; 18],
    count: [u32; 18],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lens: &[u8]) -> Result<Huffman, LzxError> {
        let max = lens.iter().copied().max().unwrap_or(0) as u32;
        if max == 0 {
            return Err(LzxError::BadCode);
        }
        if max > 16 {
            return Err(LzxError::BadCode);
        }
        let mut count = [0u32; 18];
        for &l in lens {
            count[l as usize] += 1;
        }
        // Length 0 means "unused": it takes part in no code.
        count[0] = 0;
        let mut first_code = [0u32; 18];
        let mut first_index = [0u32; 18];
        let mut symbols = Vec::with_capacity(lens.len());
        for len in 1..=max {
            for (symbol, &l) in lens.iter().enumerate() {
                if l as u32 == len {
                    symbols.push(symbol as u16);
                }
            }
        }
        let mut code = 0u32;
        let mut index = 0u32;
        for len in 1..=max {
            first_code[len as usize] = code;
            first_index[len as usize] = index;
            let n = count[len as usize];
            if code + n > (1u32 << len) {
                return Err(LzxError::BadCode);
            }
            code = (code + n) << 1;
            index += n;
        }
        Ok(Huffman { max, first_code, first_index, count, symbols })
    }

    fn read(&self, b: &mut Bits) -> Result<u16, LzxError> {
        let mut code = 0u32;
        for len in 1..=self.max {
            code = (code << 1) | b.bits(1)?;
            let first = self.first_code[len as usize];
            if code >= first {
                let index = code - first;
                if index < self.count[len as usize] {
                    let at = self.first_index[len as usize] + index;
                    return Ok(self.symbols[at as usize]);
                }
            }
        }
        Err(LzxError::BadCode)
    }
}

fn read_lens(b: &mut Bits, lens: &mut [u8], first: usize, last: usize) -> Result<(), LzxError> {
    let mut pre_lens = [0u8; 20];
    for slot in pre_lens.iter_mut() {
        *slot = b.bits(4)? as u8;
    }
    let pre = Huffman::new(&pre_lens)?;
    let mut x = first;
    while x < last {
        let z = pre.read(b)? as usize;
        match z {
            17 => {
                let y = b.bits(4)? as usize + 4;
                let end = (x + y).min(last);
                lens[x..end].fill(0);
                x += y;
            }
            18 => {
                let y = b.bits(5)? as usize + 20;
                let end = (x + y).min(last);
                lens[x..end].fill(0);
                x += y;
            }
            19 => {
                let y = b.bits(1)? as usize + 4;
                let next = pre.read(b)? as i32;
                let value = ((lens[x] as i32 - next + 17) % 17) as u8;
                let end = (x + y).min(last);
                lens[x..end].fill(value);
                x += y;
            }
            _ => {
                lens[x] = ((lens[x] as i32 - z as i32 + 17) % 17) as u8;
                x += 1;
            }
        }
    }
    Ok(())
}

/// Decodes `chunks` into `total` bytes with the 32 KiB window used by XEX.
pub fn decode(chunks: &[Vec<u8>], total: usize) -> Result<Vec<u8>, LzxError> {
    decode_with(chunks, total, 15, &[])
}

/// Same as `decode`, with an explicit window size and a pre-filled window
/// (the delta form used by CG).
pub fn decode_with(
    chunks: &[Vec<u8>],
    total: usize,
    wbits: u32,
    prefix: &[u8],
) -> Result<Vec<u8>, LzxError> {
    let mut ebits = [0u32; 52];
    let mut base = [0u32; 52];
    let mut j = 0u32;
    for (k, i) in (0..51).step_by(2).enumerate() {
        ebits[2 * k] = j;
        ebits[2 * k + 1] = j;
        if i > 0 && j < 17 {
            j += 1;
        }
    }
    let mut sum = 0u32;
    for (slot, base) in base.iter_mut().enumerate() {
        *base = sum;
        sum += 1 << ebits[slot];
    }
    let slots = match wbits {
        15 => 30,
        16 => 32,
        17 => 34,
        18 => 36,
        19 => 38,
        20 => 42,
        21 => 50,
        other => return Err(LzxError::BadWindow(other)),
    };
    let nmain = 256 + slots * 8;

    let p0 = prefix.len();
    let mut out = Vec::with_capacity(p0 + total);
    out.extend_from_slice(prefix);
    let mut r = [1u32, 1, 1];
    let mut main = vec![0u8; nmain];
    let mut length = vec![0u8; 249];
    let mut btype = 0u32;
    let mut brem = 0u32;
    let mut header = false;
    let mut ifsize = 0u32;
    let mut mt: Option<Huffman> = None;
    let mut lt: Option<Huffman> = None;
    let mut at: Option<Huffman> = None;
    let mut istarted = false;
    let mut icur = 0i32;

    for (ci, chunk) in chunks.iter().enumerate() {
        let mut b = Bits::new(chunk);
        let target = ((ci + 1) * 0x8000).min(total);
        if !header {
            if b.bits(1)? == 1 {
                ifsize = (b.bits(16)? << 16) | b.bits(16)?;
            }
            header = true;
        }
        while out.len() - p0 < target {
            if brem == 0 {
                if btype == 3 && b.odd {
                    b.bit += 8;
                }
                btype = b.bits(3)?;
                brem = (b.bits(16)? << 8) | b.bits(8)?;
                match btype {
                    1 | 2 => {
                        if btype == 2 {
                            let mut aligned = [0u8; 8];
                            for slot in aligned.iter_mut() {
                                *slot = b.bits(3)? as u8;
                            }
                            at = Some(Huffman::new(&aligned)?);
                        }
                        read_lens(&mut b, &mut main, 0, 256)?;
                        read_lens(&mut b, &mut main, 256, nmain)?;
                        mt = Some(Huffman::new(&main)?);
                        read_lens(&mut b, &mut length, 0, 249)?;
                        lt = Some(Huffman::new(&length)?);
                        if main[0xE8] != 0 {
                            istarted = true;
                        }
                    }
                    3 => {
                        istarted = true;
                        b.align();
                        let start = b.bit >> 3;
                        for (k, reg) in r.iter_mut().enumerate() {
                            let at = start + 4 * k;
                            let Some(bytes) = chunk.get(at..at + 4) else {
                                return Err(LzxError::Truncated);
                            };
                            *reg = u32::from_le_bytes(bytes.try_into().unwrap());
                        }
                        b.bit += 96;
                        b.odd = brem & 1 != 0;
                    }
                    other => return Err(LzxError::BadBlockType(other)),
                }
            }
            if btype == 3 {
                let n = (brem as usize).min(target - (out.len() - p0));
                let start = b.bit >> 3;
                let Some(bytes) = chunk.get(start..start + n) else {
                    return Err(LzxError::Truncated);
                };
                out.extend_from_slice(bytes);
                b.bit += 8 * n;
                brem -= n as u32;
                continue;
            }
            let mt = mt.as_ref().ok_or(LzxError::BadCode)?;
            let symbol = mt.read(&mut b)? as u32;
            brem -= 1;
            if symbol < 256 {

                out.push(symbol as u8);
                continue;
            }
            let symbol = symbol - 256;
            let mut ml = symbol & 7;
            if ml == 7 {
                let lt = lt.as_ref().ok_or(LzxError::BadCode)?;
                ml += lt.read(&mut b)? as u32;
            }
            ml += 2;
            let slot = (symbol >> 3) as usize;
            if slot >= ebits.len() {
                return Err(LzxError::BadCode);
            }
            let off = match slot {
                0 => r[0],
                1 => {
                    let off = r[1];
                    r[1] = r[0];
                    off
                }
                2 => {
                    let off = r[2];
                    r[2] = r[0];
                    off
                }
                _ => {
                    let e = ebits[slot];
                    let off = if btype == 2 && e >= 3 {
                        let extra = if e > 3 { b.bits(e - 3)? << 3 } else { 0 };
                        let aligned = at.as_ref().ok_or(LzxError::BadCode)?;
                        base[slot] - 2 + extra + aligned.read(&mut b)? as u32
                    } else {
                        base[slot] - 2 + b.bits(e)?
                    };
                    r[2] = r[1];
                    r[1] = r[0];
                    off
                }
            };
            r[0] = off;

            let off = off as usize;
            if off == 0 || off > out.len() {
                return Err(LzxError::Distance);
            }
            for _ in 0..ml {
                let byte = out[out.len() - off];
                out.push(byte);
            }
            brem -= ml - 1;
        }
        b.align();
        let start = p0 + ci * 0x8000;
        let end = (p0 + target).min(out.len());
        if istarted && ifsize != 0 && ci < 32768 && end - start > 10 {
            translate_calls(&mut out[start..end], ifsize, &mut icur);
        }
        icur += (end - start) as i32;
    }
    out.truncate(p0 + total);
    Ok(out)
}

/// E8 call-target translation, as mspack does when the stream sets the Intel flag.
fn translate_calls(frame: &mut [u8], ifsize: u32, icur: &mut i32) {
    let mut i = 0usize;
    let mut cur = *icur;
    while i + 10 < frame.len() {
        if frame[i] != 0xE8 {
            i += 1;
            cur += 1;
            continue;
        }
        i += 1;
        let a = i32::from_le_bytes(frame[i..i + 4].try_into().unwrap());
        if a >= -cur && (a as i64) < ifsize as i64 {
            let value = if a >= 0 { a - cur } else { a + ifsize as i32 };
            frame[i..i + 4].copy_from_slice(&value.to_le_bytes());
        }
        i += 4;
        cur += 5;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One uncompressed block (type 3) is the only stream the format lets us
    /// build by hand: bits are MSB-first over 16-bit little-endian words.
    struct Writer {
        bits: Vec<bool>,
    }

    impl Writer {
        fn new() -> Writer {
            Writer { bits: Vec::new() }
        }
        fn put(&mut self, value: u32, n: u32) {
            for i in (0..n).rev() {
                self.bits.push((value >> i) & 1 != 0);
            }
        }
        fn align(&mut self) {
            while self.bits.len() % 16 != 0 {
                self.bits.push(false);
            }
        }
        fn bytes(&mut self) -> Vec<u8> {
            self.align();
            let mut out = Vec::new();
            for pair in self.bits.chunks(16) {
                let mut word = 0u16;
                for (i, bit) in pair.iter().enumerate() {
                    if *bit {
                        word |= 1 << (15 - i);
                    }
                }
                out.extend_from_slice(&word.to_le_bytes());
            }
            out
        }
    }

    fn stored_block(data: &[u8], with_header: bool) -> Vec<u8> {
        let mut w = Writer::new();
        if with_header {
            w.put(0, 1); // no Intel filesize header
        }
        w.put(3, 3); // block type: uncompressed
        w.put(data.len() as u32, 24);
        w.align();
        let mut out = w.bytes();
        // The reader aligns, then takes R0, R1 and R2 as little-endian u32.
        for _ in 0..3 {
            out.extend_from_slice(&1u32.to_le_bytes());
        }
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn decodes_an_uncompressed_block() {
        let data: Vec<u8> = (0..1000).map(|i| (i * 7) as u8).collect();
        let chunk = stored_block(&data, true);
        assert_eq!(decode(&[chunk], data.len()).unwrap(), data);
    }

    #[test]
    fn keeps_the_window_and_state_across_frames() {
        let first: Vec<u8> = (0..0x8000).map(|i| (i % 251) as u8).collect();
        let second: Vec<u8> = (0..0x8000).map(|i| (255 - i % 251) as u8).collect();
        let chunks = vec![stored_block(&first, true), stored_block(&second, false)];
        let image = decode(&chunks, 0x8000 * 2).unwrap();
        assert_eq!(&image[..0x8000], &first[..]);
        assert_eq!(&image[0x8000..], &second[..]);
    }

    #[test]
    fn rejects_a_bad_window_and_unknown_block() {
        assert_eq!(decode_with(&[], 16, 14, &[]), Err(LzxError::BadWindow(14)));
        let mut w = Writer::new();
        w.put(0, 1);
        w.put(7, 3);
        w.put(16, 24);
        assert_eq!(decode(&[w.bytes()], 16), Err(LzxError::BadBlockType(7)));
    }

    #[test]
    fn truncated_chunk_does_not_panic() {
        let mut w = Writer::new();
        w.put(0, 1);
        w.put(3, 3);
        w.put(64, 24);
        w.align();
        let mut chunk = w.bytes();
        for _ in 0..3 {
            chunk.extend_from_slice(&1u32.to_le_bytes());
        }
        assert_eq!(decode(&[chunk], 64), Err(LzxError::Truncated));
    }
}
