"""Decrypt (AES) + decompress (LZX) a XEX2 base file to a raw PE memory image."""
import struct, sys
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from xexinfo import parse, u16, u32

RETAIL = bytes.fromhex("20B185A59D28FDC340583FBB0896BF91")
DEVKIT = bytes(16)

def aes(key, iv, data, mode):
    c = Cipher(algorithms.AES(key), mode(iv) if iv is not None else mode())
    d = c.decryptor(); return d.update(data) + d.finalize()

# ---- LZX (Xbox 360 flavour: 32K frames, no reset, chunks = frames) ----
class Bits:
    def __init__(s, d): s.d, s.p, s.n = d, 0, 0   # p = bit position
    def bits(s, n):
        if n == 0: return 0
        v = 0
        for _ in range(n):
            w = s.p >> 4
            word = s.d[2 * w] | (s.d[2 * w + 1] << 8) if 2 * w + 1 < len(s.d) else 0
            v = (v << 1) | ((word >> (15 - (s.p & 15))) & 1); s.p += 1
        return v
    def align(s): s.p = (s.p + 15) & ~15

def mktab(lens):
    codes, code, mx = {}, 0, max(lens) if lens else 0
    for L in range(1, mx + 1):
        for sym, l in enumerate(lens):
            if l == L: codes[(L, code)] = sym; code += 1
        code <<= 1
    return codes, mx

def sym(b, tab):
    codes, mx = tab; c = 0
    for L in range(1, mx + 1):
        c = (c << 1) | b.bits(1)
        if (L, c) in codes: return codes[(L, c)]
    raise ValueError("bad huffman")

def readlens(b, lens, first, last):
    pre = mktab([b.bits(4) for _ in range(20)]); x = first
    while x < last:
        z = sym(b, pre)
        if z == 17: y = b.bits(4) + 4; lens[x:x + y] = [0] * y; x += y
        elif z == 18: y = b.bits(5) + 20; lens[x:x + y] = [0] * y; x += y
        elif z == 19:
            y = b.bits(1) + 4; z = sym(b, pre); z = (lens[x] - z + 17) % 17
            lens[x:x + y] = [z] * y; x += y
        else: lens[x] = (lens[x] - z + 17) % 17; x += 1

def lzx_decode(chunks, total, wbits=15, prefix=b""):
    """prefix: pre-filled window (delta patches); output starts after it."""
    P = len(prefix)
    ebits, j = [], 0
    for i in range(0, 51, 2):
        ebits += [j, j]
        if i and j < 17: j += 1
    base, j = [], 0
    for e in ebits: base.append(j); j += 1 << e
    slots = {15: 30, 16: 32, 17: 34, 18: 36, 19: 38, 20: 42, 21: 50}[wbits]
    nmain = 256 + slots * 8
    out = bytearray(prefix); R = [1, 1, 1]
    main = [0] * nmain; length = [0] * 249
    btype = brem = 0; header = False; mt = lt = at = None; ifsize = 0; istarted = False; res = bytearray(); icur = 0
    for ci, ch in enumerate(chunks):
        b = Bits(ch); target = min((ci + 1) * 0x8000, total)
        if not header:
            if b.bits(1): ifsize = (b.bits(16) << 16) | b.bits(16)
            header = True
        while len(out) - P < target:
            if brem == 0:
                if btype == 3 and brem == 0 and getattr(b, "odd", False): b.p += 8
                btype = b.bits(3); brem = (b.bits(16) << 8) | b.bits(8)
                if btype == 2: at = mktab([b.bits(3) for _ in range(8)])
                if btype in (1, 2):
                    readlens(b, main, 0, 256); readlens(b, main, 256, nmain)
                    mt = mktab(main); readlens(b, length, 0, 249); lt = mktab(length)
                    if main[0xE8]: istarted = True
                elif btype == 3:
                    istarted = True; b.align(); i = b.p >> 3
                    R = [struct.unpack_from("<I", ch, i + 4 * k)[0] for k in range(3)]; b.p += 96
                    b.odd = brem & 1
            if btype == 3:
                n = min(brem, target - (len(out) - P)); i = b.p >> 3
                out += ch[i:i + n]; b.p += 8 * n; brem -= n; continue
            m = sym(b, mt); brem -= 1
            if m < 256: out.append(m); continue
            m -= 256; ml = m & 7
            if ml == 7: ml += sym(b, lt)
            ml += 2; slot = m >> 3
            if slot == 0: off = R[0]
            elif slot == 1: off = R[1]; R[1] = R[0]
            elif slot == 2: off = R[2]; R[2] = R[0]
            else:
                e = ebits[slot]
                if btype == 2 and e >= 3:
                    v = (b.bits(e - 3) << 3) if e > 3 else 0
                    off = base[slot] - 2 + v + sym(b, at)
                else: off = base[slot] - 2 + b.bits(e)
                R[2], R[1] = R[1], R[0]
            R[0] = off
            for _ in range(ml): out.append(out[-off])
            brem -= ml - 1
        b.align()
        fs = (ci * 0x8000, min(target, total))
        fr = bytearray(out[P + fs[0]:P + fs[1]])
        if istarted and ifsize and ci < 32768 and len(fr) > 10:   # E8 call-target translation (mspack)
            i, cur = 0, icur
            while i < len(fr) - 10:
                if fr[i] != 0xE8: i += 1; cur += 1; continue
                i += 1; a = int.from_bytes(fr[i:i + 4], "little", signed=True)
                if a >= -cur and a < ifsize:
                    fr[i:i + 4] = (a - cur if a >= 0 else a + ifsize).to_bytes(4, "little", signed=True)
                i += 4; cur += 5
        icur += len(fr); res += fr
    return bytes(res[:total])

def extract(path, key=None):
    x = parse(path); b = x["b"]; bf = x["opt"][0x3FF]
    enc, comp = u16(bf, 4), u16(bf, 6)
    data = b[x["pe_offset"]:]
    if enc:
        for k in ([key] if key else [RETAIL, DEVKIT]):
            fk = aes(k, None, bytes.fromhex(x["file_key"]), modes.ECB)
            d = aes(fk, bytes(16), data[:len(data) & ~15], modes.CBC)
            if comp != 2 or u32(d, 0) <= len(d): break
        data = d
    if comp == 0: return data
    assert comp == 2, "only LZX supported"
    bsz = u32(bf, 12); chunks, p = [], 0
    while bsz:
        nxt = u32(data, p); q = p + 24; end = p + bsz
        while q < end:
            cs = u16(data, q)
            if cs == 0: break
            chunks.append(data[q + 2:q + 2 + cs]); q += 2 + cs
        p, bsz = end, nxt
    total = x["image_size"]
    return lzx_decode(chunks, total)

if __name__ == "__main__":
    img = extract(sys.argv[1])
    open(sys.argv[2], "wb").write(img)
    print(len(img), img[:4], hex(u32(img, 0x3c)) if len(img) > 0x40 else "")
