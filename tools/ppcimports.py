"""Resolve XEX import records to names (Xenia export tables) and list call xrefs."""
import re, struct, sys, os
from xexinfo import parse, u32, u16
from xexextract import extract

HERE = os.path.dirname(__file__)

def table(fn):
    t = {}
    for m in re.finditer(r"XE_EXPORT\(\w+,\s*(0x[0-9A-Fa-f]+),\s*(\w+)", open(os.path.join(HERE, "data", fn)).read()):
        t[int(m.group(1), 16)] = m.group(2)
    return t

TABLES = {"xboxkrnl.exe": table("xboxkrnl_table.inc"), "xam.xex": table("xam_table.inc")}

def sections(img):
    e = struct.unpack_from("<I", img, 0x3C)[0]
    n = struct.unpack_from("<H", img, e + 6)[0]; opt = struct.unpack_from("<H", img, e + 20)[0]
    out = []
    for i in range(n):
        o = e + 24 + opt + 40 * i
        nm = img[o:o + 8].rstrip(b"\0").decode()
        vs, va, rs, ro = struct.unpack_from("<IIII", img, o + 8)
        out.append((nm, va, vs, ro, rs))
    return out

def v2f(secs, rva):
    for nm, va, vs, ro, rs in secs:
        if va <= rva < va + max(vs, rs): return ro + rva - va
    return None

def imports(path):
    x = parse(path); img = extract(path); base = int(x["opt"][0x10201]); secs = sections(img)
    blob = x["opt"][0x103FF]
    size, strsz, cnt = struct.unpack_from(">III", blob, 0)
    names, o = [], 12
    while o < 12 + strsz:
        e = blob.index(b"\0", o); 
        if e > o: names.append(blob[o:e].decode())
        o = e + 1
    o = 12 + strsz; res = {}   # slot addr -> (lib, ordinal, name, kind)
    for _ in range(cnt):
        rsz = u32(blob, o); ni, nrec = struct.unpack_from(">HH", blob, o + 36)
        lib = names[ni & 0xFF]
        for r in range(nrec):
            addr = u32(blob, o + 40 + 4 * r)
            f = v2f(secs, addr - base)
            val = u32(img, f) if f is not None else 0
            if val >> 24 == 0:   # ordinal slot; the following record is its call thunk
                last = (lib, val & 0xFFFF, TABLES.get(lib, {}).get(val & 0xFFFF, f"ord_{val & 0xFFFF:#x}"))
                res[addr] = last + ("slot",)
            else: res[addr] = last + ("thunk",)
        o += rsz
    return x, img, base, secs, res

def strings(img, secs, base, minlen=5):
    out = []
    for nm, va, vs, ro, rs in secs:
        if nm not in (".rdata", ".data"): continue
        blob = img[ro:ro + rs]
        for m in re.finditer(rb"[\x20-\x7e]{%d,}" % minlen, blob): out.append((base + va + m.start(), m.group().decode()))
        for m in re.finditer(rb"(?:[\x20-\x7e]\x00){%d,}" % minlen, blob):
            out.append((base + va + m.start(), "u16:" + m.group().decode("utf-16le")))
    return out

if __name__ == "__main__":
    x, img, base, secs, res = imports(sys.argv[1])
    for a in sorted(res): print(hex(a), *res[a])
