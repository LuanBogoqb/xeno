"""Decrypt the boot chain (CB_A, CB_B, CD, CE, CF) of a retail/RGH3 flash dump.
Key derivations follow InvoxiPlayGames/xenon-bltool (GPL-2, read for the algorithm only)."""
import hmac, hashlib, struct, sys, os
from flashinfo import rc4
from xexextract import lzx_decode

KEY_1BL = bytes.fromhex("DD88AD0C9ED669E7B56794FB68563EFA")
H = lambda k, *ds: hmac.new(k, b"".join(ds), hashlib.sha1).digest()[:16]
al = lambda n: (n + 15) & ~15

def hdr(b): return dict(magic=b[:2].decode(), version=struct.unpack_from(">H", b, 2)[0], flags=struct.unpack_from(">H", b, 6)[0],
                        entry=struct.unpack_from(">I", b, 8)[0], size=struct.unpack_from(">I", b, 12)[0])

def dec(buf, key, start=0x20):
    """RC4-decrypt buf[start:aligned size] in place with the derived key."""
    n = al(hdr(buf)["size"]); out = bytearray(buf[:n].ljust(n, b"\0"))
    out[start:n] = rc4(key, bytes(out[start:n])); return bytes(out)

def load(d):
    nh = dict(cb_off=struct.unpack_from(">I", d, 8)[0], cf_off=struct.unpack_from(">I", d, 0x64)[0])
    off = nh["cb_off"]; stages = []
    def take(o): h = hdr(d[o:o + 16]); return d[o:o + al(h["size"])], h
    a_raw, ah = take(off); off += al(ah["size"])
    a = dec(a_raw, H(KEY_1BL, a_raw[0x10:0x20]))                       # CB_A: key from 1BL
    stages.append(("CB_A", a, ah))
    cur_key = H(KEY_1BL, a_raw[0x10:0x20])
    nxt, nh2 = take(off)
    if nh2["flags"] == 0x800 and nh2["size"] <= 0x1000:                 # tiny zero-pair stage (RGH3 loader), kept raw
        stages.append(("CB_A2(zeropair)", nxt, nh2)); off += al(nh2["size"]); nxt, nh2 = take(off)
    cbb_key = nxt[0x10:0x20]                                           # RGH image stores CB_B already decrypted: key field is the key
    stages.append(("CB_B", nxt, nh2)); off += al(nh2["size"])
    cd_raw, ch = take(off); off += al(ch["size"])
    cd_key = H(cbb_key, cd_raw[0x10:0x20]); cd = dec(cd_raw, cd_key)
    stages.append(("CD", cd, ch))
    ce_raw, eh = take(off)
    ce_key = H(cd_key, ce_raw[0x10:0x20]); ce = dec(ce_raw, ce_key)
    stages.append(("CE", ce, eh))
    cf_raw, fh = take(nh["cf_off"])
    cf = dec(cf_raw, H(KEY_1BL, cf_raw[0x20:0x30]), 0x30)
    stages.append(("CF", cf, fh))
    return stages

def ce_kernel(ce):
    """Decompress the base kernel/hypervisor image out of a decrypted CE."""
    unc = struct.unpack_from(">I", ce, 0x28)[0]; o = 0x30; chunks = []; got = 0
    while got < unc:
        cs, ds = struct.unpack_from(">HH", ce, o); chunks.append(ce[o + 4:o + 4 + cs]); o += 4 + cs; got += ds
    return lzx_decode(chunks, unc, 17)

def apply_cg(d, cf, base):
    """Decrypt the CG next to the CF in its patch slot and apply the LZX-delta patch to the base kernel."""
    cf_off = struct.unpack_from(">I", d, 0x64)[0]; cgo = cf_off + al(hdr(cf)["size"])
    csize = al(struct.unpack_from(">I", d, cgo + 12)[0])
    cg_raw = bytearray(); p = cgo                                       # skip near-empty 64 KiB blocks (0xC0000 on this image)
    while len(cg_raw) < csize:
        end = min((p | 0xFFFF) + 1, p + csize - len(cg_raw)); seg = d[p:end]; p = end
        if (p - len(seg)) % 0x10000 == 0 and len(seg) == 0x10000 and len(seg) - seg.count(0xFF) < 0x1000: continue
        cg_raw += seg
    cg_raw = bytes(cg_raw)
    cg = dec(cg_raw, H(cf[0x330:0x340], cg_raw[0x10:0x20]))         # cg_hmac sits right after CF's 0x100-byte signature
    osz, nsz = struct.unpack_from(">I", cg, 0x20)[0], struct.unpack_from(">I", cg, 0x38)[0]
    assert hashlib.sha1(base[:osz]).digest() == cg[0x24:0x38], "base kernel hash mismatch"
    out = bytearray(base[:osz]) + bytes(nsz - osz); o = 0x50; end = hdr(cg)["size"]
    while o < end:
        old, new, dsz, csz = struct.unpack_from(">IIHH", cg, o); o += 12
        if csz == 0 and dsz == 0 and new == 0 and old == 0: break
        if csz == 0: out[new:new + dsz] = bytes(dsz)
        elif csz == 1: out[new:new + dsz] = out[old:old + dsz]
        else:
            win = bytes((1 << 15) - dsz) + bytes(out[old:old + dsz])
            out[new:new + dsz] = lzx_decode([cg[o:o + csz]], dsz, 15, win); o += csz
    assert hashlib.sha1(out).digest() == cg[0x3C:0x50], "patched kernel hash mismatch"
    return bytes(out)

if __name__ == "__main__":
    d = open(sys.argv[1], "rb").read(); out = sys.argv[2]; os.makedirs(out, exist_ok=True)
    for name, b, h in load(d):
        open(os.path.join(out, name.split("(")[0] + ".bin"), "wb").write(b)
        extra = ""
        if name == "CE": extra = f" unknown={struct.unpack_from('>I', b, 0x2C)[0]} unc=0x{struct.unpack_from('>I', b, 0x28)[0]:x}"
        if name == "CF": extra = f" base={struct.unpack_from('>H', b, 0x10)[0]} target={struct.unpack_from('>H', b, 0x14)[0]} decrypted={b[0x30] == 0}"
        print(f"{name:16s} build={h['version']:5d} size=0x{h['size']:x}{extra} head20={b[0x20:0x28].hex()}")
    ce = [b for n, b, _ in load(d) if n == "CE"][0]
    k = ce_kernel(ce); open(os.path.join(out, "kernel_base.bin"), "wb").write(k)
    print("kernel_base", len(k), k[:4])
    cf = [b for n, b, _ in load(d) if n == "CF"][0]
    k2 = apply_cg(d, cf, k); open(os.path.join(out, "kernel_17559.bin"), "wb").write(k2)
    print("kernel_17559", len(k2), k2[:4], "sha1", hashlib.sha1(k2).hexdigest())
