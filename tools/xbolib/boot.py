"""Flash boot chain: CB_A -> CB_B -> CD -> CE, plus CF/CG kernel patching.

Key derivations follow InvoxiPlayGames/xenon-bltool (GPL-2; read for the
algorithm only). The 1BL key is public; no per-console key is needed for this
image.
"""

import hashlib
import os
import struct

from .binary import align, u32
from .crypto import hmac_key, rc4
from .lzx import lzx_decode

KEY_1BL = bytes.fromhex("DD88AD0C9ED669E7B56794FB68563EFA")


def header(b: bytes) -> dict:
    return {
        "magic": b[:2].decode(),
        "version": struct.unpack_from(">H", b, 2)[0],
        "flags": struct.unpack_from(">H", b, 6)[0],
        "entry": struct.unpack_from(">I", b, 8)[0],
        "size": struct.unpack_from(">I", b, 12)[0],
    }


def decrypt_stage(buf: bytes, key: bytes, start: int = 0x20) -> bytes:
    """RC4-decrypt buf[start:aligned size] with the derived key (rest kept as-is)."""
    n = align(header(buf)["size"])
    out = bytearray(buf[:n].ljust(n, b"\0"))
    out[start:n] = rc4(key, bytes(out[start:n]))
    return bytes(out)


def load(d: bytes) -> list[tuple[str, bytes, dict]]:
    """Return [(name, bytes, header)] for every stage in the flash dump."""
    nh = dict(cb_off=u32(d, 8), cf_off=u32(d, 0x64))
    off = nh["cb_off"]
    stages = []

    def take(o: int):
        h = header(d[o:o + 16])
        return d[o:o + align(h["size"])], h

    a_raw, ah = take(off)
    off += align(ah["size"])
    a_key = hmac_key(KEY_1BL, a_raw[0x10:0x20])
    stages.append(("CB_A", decrypt_stage(a_raw, a_key), ah))

    nxt, nh2 = take(off)
    if nh2["flags"] == 0x800 and nh2["size"] <= 0x1000:  # tiny zero-pair stage (RGH3 loader), kept raw
        stages.append(("CB_A2(zeropair)", nxt, nh2))
        off += align(nh2["size"])
        nxt, nh2 = take(off)

    cbb_key = nxt[0x10:0x20]  # RGH image stores CB_B already decrypted: the key field is the key
    stages.append(("CB_B", nxt, nh2))
    off += align(nh2["size"])

    cd_raw, ch = take(off)
    off += align(ch["size"])
    cd_key = hmac_key(cbb_key, cd_raw[0x10:0x20])
    stages.append(("CD", decrypt_stage(cd_raw, cd_key), ch))

    ce_raw, eh = take(off)
    ce_key = hmac_key(cd_key, ce_raw[0x10:0x20])
    stages.append(("CE", decrypt_stage(ce_raw, ce_key), eh))

    cf_raw, fh = take(nh["cf_off"])
    stages.append(("CF", decrypt_stage(cf_raw, hmac_key(KEY_1BL, cf_raw[0x20:0x30]), 0x30), fh))
    return stages


def ce_kernel(ce: bytes) -> bytes:
    """Decompress the base kernel/hypervisor image out of a decrypted CE."""
    unc = struct.unpack_from(">I", ce, 0x28)[0]
    o, chunks, got = 0x30, [], 0
    while got < unc:
        cs, ds = struct.unpack_from(">HH", ce, o)
        chunks.append(ce[o + 4:o + 4 + cs])
        o += 4 + cs
        got += ds
    return lzx_decode(chunks, unc, 17)


def apply_cg(d: bytes, cf: bytes, base: bytes) -> bytes:
    """Decrypt the CG patch next to the CF and apply it to the base kernel."""
    cf_off = u32(d, 0x64)
    cgo = cf_off + align(header(cf)["size"])
    csize = align(struct.unpack_from(">I", d, cgo + 12)[0])
    cg_raw = bytearray()
    p = cgo  # skip near-empty 64 KiB blocks (the 0xC0000 hole on this image)
    while len(cg_raw) < csize:
        end = min((p | 0xFFFF) + 1, p + csize - len(cg_raw))
        seg = d[p:end]
        p = end
        if (p - len(seg)) % 0x10000 == 0 and len(seg) == 0x10000 and len(seg) - seg.count(0xFF) < 0x1000:
            continue
        cg_raw += seg
    cg_raw = bytes(cg_raw)
    # cg_hmac sits right after the CF's 0x100-byte signature.
    cg = decrypt_stage(cg_raw, hmac_key(cf[0x330:0x340], cg_raw[0x10:0x20]))
    osz, nsz = struct.unpack_from(">I", cg, 0x20)[0], struct.unpack_from(">I", cg, 0x38)[0]
    assert hashlib.sha1(base[:osz]).digest() == cg[0x24:0x38], "base kernel hash mismatch"
    out = bytearray(base[:osz]) + bytes(nsz - osz)
    o, end = 0x50, header(cg)["size"]
    while o < end:
        old, new, dsz, csz = struct.unpack_from(">IIHH", cg, o)
        o += 12
        if csz == 0 and dsz == 0 and new == 0 and old == 0:
            break
        if csz == 0:
            out[new:new + dsz] = bytes(dsz)
        elif csz == 1:
            out[new:new + dsz] = out[old:old + dsz]
        else:
            win = bytes((1 << 15) - dsz) + bytes(out[old:old + dsz])
            out[new:new + dsz] = lzx_decode([cg[o:o + csz]], dsz, 15, win)
            o += csz
    assert hashlib.sha1(out).digest() == cg[0x3C:0x50], "patched kernel hash mismatch"
    return bytes(out)


def dump_all(flash_path: str, out_dir: str) -> int:
    """Decrypt every stage and write kernel_base.bin / kernel_17559.bin; prints a summary."""
    d = open(flash_path, "rb").read()
    os.makedirs(out_dir, exist_ok=True)
    for name, b, h in load(d):
        open(os.path.join(out_dir, name.split("(")[0] + ".bin"), "wb").write(b)
        extra = ""
        if name == "CE":
            extra = f" unknown={struct.unpack_from('>I', b, 0x2C)[0]} unc=0x{struct.unpack_from('>I', b, 0x28)[0]:x}"
        if name == "CF":
            extra = (f" base={struct.unpack_from('>H', b, 0x10)[0]} target={struct.unpack_from('>H', b, 0x14)[0]}"
                     f" decrypted={b[0x30] == 0}")
        print(f"{name:16s} build={h['version']:5d} size=0x{h['size']:x}{extra} head20={b[0x20:0x28].hex()}")
    ce = next(b for n, b, _ in load(d) if n == "CE")
    k = ce_kernel(ce)
    open(os.path.join(out_dir, "kernel_base.bin"), "wb").write(k)
    print("kernel_base", len(k), k[:4])
    cf = next(b for n, b, _ in load(d) if n == "CF")
    k2 = apply_cg(d, cf, k)
    open(os.path.join(out_dir, "kernel_17559.bin"), "wb").write(k2)
    print("kernel_17559", len(k2), k2[:4], "sha1", hashlib.sha1(k2).hexdigest())
    return 0
