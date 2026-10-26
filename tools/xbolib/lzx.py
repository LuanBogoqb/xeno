"""Xbox 360 LZX: 32 KiB frames, no reset, one chunk list per frame.

`lzx_decode` also handles the delta form used by CG (`prefix` = the bytes of the
old window the patch relies on) and the E8 call-target translation that mspack
performs when the stream sets the Intel flag.
"""

import struct


class Bits:
    """Bit reader over 16-bit little-endian words, MSB first."""

    def __init__(self, d: bytes):
        self.d = d
        self.p = 0  # bit position
        self.odd = False

    def bits(self, n: int) -> int:
        if n == 0:
            return 0
        v = 0
        for _ in range(n):
            w = self.p >> 4
            word = self.d[2 * w] | (self.d[2 * w + 1] << 8) if 2 * w + 1 < len(self.d) else 0
            v = (v << 1) | ((word >> (15 - (self.p & 15))) & 1)
            self.p += 1
        return v

    def align(self) -> None:
        self.p = (self.p + 15) & ~15


def make_table(lens: list[int]) -> tuple[dict, int]:
    codes, code, mx = {}, 0, max(lens) if lens else 0
    for length in range(1, mx + 1):
        for symbol, l in enumerate(lens):
            if l == length:
                codes[(length, code)] = symbol
                code += 1
        code <<= 1
    return codes, mx


def read_symbol(b: Bits, table: tuple[dict, int]) -> int:
    codes, mx = table
    c = 0
    for length in range(1, mx + 1):
        c = (c << 1) | b.bits(1)
        if (length, c) in codes:
            return codes[(length, c)]
    raise ValueError("bad huffman code")


def read_lens(b: Bits, lens: list[int], first: int, last: int) -> None:
    pre = make_table([b.bits(4) for _ in range(20)])
    x = first
    while x < last:
        z = read_symbol(b, pre)
        if z == 17:
            y = b.bits(4) + 4
            lens[x:x + y] = [0] * y
            x += y
        elif z == 18:
            y = b.bits(5) + 20
            lens[x:x + y] = [0] * y
            x += y
        elif z == 19:
            y = b.bits(1) + 4
            z = read_symbol(b, pre)
            z = (lens[x] - z + 17) % 17
            lens[x:x + y] = [z] * y
            x += y
        else:
            lens[x] = (lens[x] - z + 17) % 17
            x += 1


def lzx_decode(chunks: list[bytes], total: int, wbits: int = 15, prefix: bytes = b"") -> bytes:
    """Decode `chunks` into `total` bytes; `prefix` is a pre-filled window (CG delta)."""
    p0 = len(prefix)
    ebits, j = [], 0
    for i in range(0, 51, 2):
        ebits += [j, j]
        if i and j < 17:
            j += 1
    base, j = [], 0
    for e in ebits:
        base.append(j)
        j += 1 << e
    slots = {15: 30, 16: 32, 17: 34, 18: 36, 19: 38, 20: 42, 21: 50}[wbits]
    nmain = 256 + slots * 8

    out = bytearray(prefix)
    r = [1, 1, 1]
    main = [0] * nmain
    length = [0] * 249
    btype = brem = 0
    header = False
    mt = lt = at = None
    ifsize = 0
    istarted = False
    res = bytearray()
    icur = 0

    for ci, ch in enumerate(chunks):
        b = Bits(ch)
        target = min((ci + 1) * 0x8000, total)
        if not header:
            if b.bits(1):
                ifsize = (b.bits(16) << 16) | b.bits(16)
            header = True
        while len(out) - p0 < target:
            if brem == 0:
                if btype == 3 and brem == 0 and b.odd:
                    b.p += 8
                btype = b.bits(3)
                brem = (b.bits(16) << 8) | b.bits(8)
                if btype == 2:
                    at = make_table([b.bits(3) for _ in range(8)])
                if btype in (1, 2):
                    read_lens(b, main, 0, 256)
                    read_lens(b, main, 256, nmain)
                    mt = make_table(main)
                    read_lens(b, length, 0, 249)
                    lt = make_table(length)
                    if main[0xE8]:
                        istarted = True
                elif btype == 3:
                    istarted = True
                    b.align()
                    i = b.p >> 3
                    r = [struct.unpack_from("<I", ch, i + 4 * k)[0] for k in range(3)]
                    b.p += 96
                    b.odd = bool(brem & 1)
            if btype == 3:
                n = min(brem, target - (len(out) - p0))
                i = b.p >> 3
                out += ch[i:i + n]
                b.p += 8 * n
                brem -= n
                continue
            m = read_symbol(b, mt)
            brem -= 1
            if m < 256:
                out.append(m)
                continue
            m -= 256
            ml = m & 7
            if ml == 7:
                ml += read_symbol(b, lt)
            ml += 2
            slot = m >> 3
            if slot == 0:
                off = r[0]
            elif slot == 1:
                off = r[1]
                r[1] = r[0]
            elif slot == 2:
                off = r[2]
                r[2] = r[0]
            else:
                e = ebits[slot]
                if btype == 2 and e >= 3:
                    v = (b.bits(e - 3) << 3) if e > 3 else 0
                    off = base[slot] - 2 + v + read_symbol(b, at)
                else:
                    off = base[slot] - 2 + b.bits(e)
                r[2], r[1] = r[1], r[0]
            r[0] = off
            for _ in range(ml):
                out.append(out[-off])
            brem -= ml - 1
        b.align()
        fs = (ci * 0x8000, min(target, total))
        frame = bytearray(out[p0 + fs[0]:p0 + fs[1]])
        if istarted and ifsize and ci < 32768 and len(frame) > 10:
            # E8 call-target translation (mspack behaviour).
            i, cur = 0, icur
            while i < len(frame) - 10:
                if frame[i] != 0xE8:
                    i += 1
                    cur += 1
                    continue
                i += 1
                a = int.from_bytes(frame[i:i + 4], "little", signed=True)
                if a >= -cur and a < ifsize:
                    frame[i:i + 4] = (a - cur if a >= 0 else a + ifsize).to_bytes(4, "little", signed=True)
                i += 4
                cur += 5
        icur += len(frame)
        res += frame
    return bytes(res[:total])
