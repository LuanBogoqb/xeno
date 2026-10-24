"""Dump XEX2 header: optional headers, security info, base-file format, imports."""
import struct, sys

def u32(b, o): return struct.unpack_from(">I", b, o)[0]
def u16(b, o): return struct.unpack_from(">H", b, o)[0]

KEYS = {0x3FF: "base_file_format", 0x10100: "entry_point", 0x10201: "image_base",
        0x103FF: "import_libs", 0x40006: "execution_info", 0x183FF: "original_pe_name",
        0x20104: "tls_info", 0x30000: "default_stack", 0x40201: "default_heap",
        0x80109: "system_flags", 0x18002: "checksum_timestamp", 0x20200: "stack_size"}

def parse(path):
    b = open(path, "rb").read()
    assert b[:4] == b"XEX2", "not XEX2"
    flags, pe_off, _, sec_off, n = struct.unpack_from(">IIIII", b, 4)
    out = {"size": len(b), "module_flags": hex(flags), "pe_offset": pe_off,
           "sec_offset": sec_off, "opt": {}}
    for i in range(n):
        k, v = struct.unpack_from(">II", b, 24 + 8 * i)
        low = k & 0xFF
        if low in (0, 1): data = v
        elif low == 0xFF: data = b[v:v + u32(b, v)]
        else: data = b[v:v + low * 4]
        out["opt"][k] = data
    s = sec_off
    out["image_size"] = u32(b, s + 4)
    out["image_flags"] = hex(u32(b, s + 268))
    out["load_address"] = hex(u32(b, s + 272))
    out["file_key"] = b[s + 336:s + 352].hex()
    out["pages"] = u32(b, s + 384)
    out["b"] = b
    return out

if __name__ == "__main__":
    x = parse(sys.argv[1])
    print({k: v for k, v in x.items() if k not in ("opt", "b")})
    for k, v in x["opt"].items():
        name = KEYS.get(k, "?")
        print(f"opt 0x{k:08X} {name}:", hex(v) if isinstance(v, int) else v[:48].hex() + ("…" if len(v) > 48 else ""), f"[{len(v)}B]" if not isinstance(v, int) else "")
    if 0x3FF in x["opt"]:
        bf = x["opt"][0x3FF]
        print("base_file_format enc/comp:", u16(bf, 4), u16(bf, 6), "(enc 0 none/1 aes; comp 0 none/1 basic/2 lzx/3 delta)")
