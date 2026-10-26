"""XEX2: header parsing, AES+LZX extraction, imports and strings."""

import re
import struct
from pathlib import Path

from cryptography.hazmat.primitives.ciphers import modes

from .binary import u16, u32
from .crypto import aes_decrypt
from .lzx import lzx_decode

# XEX2 media keys: retail and the devkit zero key.
RETAIL = bytes.fromhex("20B185A59D28FDC340583FBB0896BF91")
DEVKIT = bytes(16)

OPTIONAL_HEADERS = {
    0x3FF: "base_file_format",
    0x10100: "entry_point",
    0x10201: "image_base",
    0x103FF: "import_libs",
    0x40006: "execution_info",
    0x183FF: "original_pe_name",
    0x20104: "tls_info",
    0x30000: "default_stack",
    0x40201: "default_heap",
    0x80109: "system_flags",
    0x18002: "checksum_timestamp",
    0x20200: "stack_size",
}

_DATA = Path(__file__).resolve().parents[1] / "data"
_table_cache: dict[str, dict[int, str]] = {}


def export_table(library: str) -> dict[int, str]:
    """Ordinal -> name table for `library` (`xboxkrnl.exe`, `xam.xex`); {} if unknown."""
    if library in _table_cache:
        return _table_cache[library]
    files = {"xboxkrnl.exe": "xboxkrnl_table.inc", "xam.xex": "xam_table.inc"}
    out: dict[int, str] = {}
    fn = files.get(library)
    if fn and (_DATA / fn).exists():
        for m in re.finditer(r"XE_EXPORT\(\w+,\s*(0x[0-9A-Fa-f]+),\s*(\w+)", (_DATA / fn).read_text()):
            out[int(m.group(1), 16)] = m.group(2)
    _table_cache[library] = out
    return out


def parse(path) -> dict:
    """Parse the XEX2 header and optional headers; the raw file stays in `b`."""
    b = Path(path).read_bytes()
    assert b[:4] == b"XEX2", "not XEX2"
    flags, pe_off, _, sec_off, n = struct.unpack_from(">IIIII", b, 4)
    out = {
        "size": len(b),
        "module_flags": hex(flags),
        "pe_offset": pe_off,
        "sec_offset": sec_off,
        "opt": {},
    }
    for i in range(n):
        k, v = struct.unpack_from(">II", b, 24 + 8 * i)
        low = k & 0xFF
        if low in (0, 1):
            data = v
        elif low == 0xFF:
            data = b[v:v + u32(b, v)]
        else:
            data = b[v:v + low * 4]
        out["opt"][k] = data
    s = sec_off
    out["image_size"] = u32(b, s + 4)
    out["image_flags"] = hex(u32(b, s + 268))
    out["load_address"] = hex(u32(b, s + 272))
    out["file_key"] = b[s + 336:s + 352].hex()
    out["pages"] = u32(b, s + 384)
    out["b"] = b
    return out


def sections(img: bytes) -> list[tuple[str, int, int, int, int]]:
    """PE sections as (name, virtual address, virtual size, raw offset, raw size)."""
    e = struct.unpack_from("<I", img, 0x3C)[0]
    n = struct.unpack_from("<H", img, e + 6)[0]
    opt = struct.unpack_from("<H", img, e + 20)[0]
    out = []
    for i in range(n):
        o = e + 24 + opt + 40 * i
        nm = img[o:o + 8].rstrip(b"\0").decode()
        vs, va, rs, ro = struct.unpack_from("<IIII", img, o + 8)
        out.append((nm, va, vs, ro, rs))
    return out


def image_offset(secs, rva: int) -> int | None:
    for _nm, va, vs, ro, rs in secs:
        if va <= rva < va + max(vs, rs):
            return ro + rva - va
    return None


def extract(path, key: bytes | None = None) -> bytes:
    """Decrypt (AES) and decompress (LZX) the base file into a raw PE image."""
    x = parse(path)
    b = x["b"]
    bf = x["opt"][0x3FF]
    enc, comp = u16(bf, 4), u16(bf, 6)
    data = b[x["pe_offset"]:]
    if enc:
        for k in ([key] if key else [RETAIL, DEVKIT]):
            fk = aes_decrypt(k, None, bytes.fromhex(x["file_key"]), modes.ECB)
            d = aes_decrypt(fk, bytes(16), data[:len(data) & ~15], modes.CBC)
            if comp != 2 or u32(d, 0) <= len(d):
                break
        data = d
    if comp == 0:
        return data
    assert comp == 2, "only LZX supported"
    bsz = u32(bf, 12)
    chunks, p = [], 0
    while bsz:
        nxt = u32(data, p)
        q = p + 24
        end = p + bsz
        while q < end:
            cs = u16(data, q)
            if cs == 0:
                break
            chunks.append(data[q + 2:q + 2 + cs])
            q += 2 + cs
        p, bsz = end, nxt
    return lzx_decode(chunks, x["image_size"])


def imports(path) -> tuple[dict, bytes, int, list, dict]:
    """Resolve import records: (header, image, image base, sections, slot -> record)."""
    x = parse(path)
    img = extract(path)
    base = int(x["opt"][0x10201])
    secs = sections(img)
    blob = x["opt"][0x103FF]
    _size, strsz, cnt = struct.unpack_from(">III", blob, 0)

    names, o = [], 12
    while o < 12 + strsz:
        e = blob.index(b"\0", o)
        if e > o:
            names.append(blob[o:e].decode())
        o = e + 1

    o = 12 + strsz
    res: dict[int, tuple[str, int, str, str]] = {}
    last = ("", 0, "")
    for _ in range(cnt):
        rsz = u32(blob, o)
        ni, nrec = struct.unpack_from(">HH", blob, o + 36)
        lib = names[ni & 0xFF]
        for r in range(nrec):
            addr = u32(blob, o + 40 + 4 * r)
            f = image_offset(secs, addr - base)
            val = u32(img, f) if f is not None else 0
            if val >> 24 == 0:  # ordinal slot; the next record is its call thunk
                last = (lib, val & 0xFFFF, export_table(lib).get(val & 0xFFFF, f"ord_{val & 0xFFFF:#x}"))
                res[addr] = last + ("slot",)
            else:
                res[addr] = last + ("thunk",)
        o += rsz
    return x, img, base, secs, res


def strings(img: bytes, secs, base: int, minlen: int = 5) -> list[tuple[int, str]]:
    """ASCII and UTF-16LE strings in .rdata/.data with their virtual addresses."""
    out = []
    for nm, va, _vs, ro, rs in secs:
        if nm not in (".rdata", ".data"):
            continue
        blob = img[ro:ro + rs]
        for m in re.finditer(rb"[\x20-\x7e]{%d,}" % minlen, blob):
            out.append((base + va + m.start(), m.group().decode()))
        for m in re.finditer(rb"(?:[\x20-\x7e]\x00){%d,}" % minlen, blob):
            out.append((base + va + m.start(), "u16:" + m.group().decode("utf-16le")))
    return out
