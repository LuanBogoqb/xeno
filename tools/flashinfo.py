"""Walk the boot chain of a flash dump (CB/CD/CE...) and try to decrypt each stage.

Exploratory companion to bootchain.py: it does not know the per-stage key
derivation, it just tries the keys you give it and reports which stage ends up
most zero-filled.
"""

import struct
import sys

from xbolib.crypto import hmac_key, rc4


def stages(d: bytes) -> list[dict]:
    off = struct.unpack_from(">I", d, 8)[0]
    out = []
    while True:
        magic = d[off:off + 2]
        if not (magic[:1] in (b"C", b"S") and magic[1:].isalpha()):
            break
        build, qfe, flags, entry, size = struct.unpack_from(">HHHII", d, off + 2)
        out.append(dict(name=magic.decode(), off=off, build=build, qfe=qfe, flags=flags,
                        entry=entry, size=size, salt=d[off + 0x10:off + 0x20]))
        off = (off + size + 0xF) & ~0xF
    return out


def zerofrac(b: bytes) -> float:
    return b.count(0) / max(1, len(b))


def try_decrypt(d: bytes, st: dict, keys: dict) -> tuple:
    body = d[st["off"] + 0x20:st["off"] + st["size"]]
    best = None
    for kn, k in keys.items():
        rk = hmac_key(k, st["salt"])
        p = rc4(rk, body)
        if best is None or zerofrac(p) > best[0]:
            best = (zerofrac(p), kn, rk, p)
    return best


if __name__ == "__main__":
    d = open(sys.argv[1], "rb").read()
    keys = {"cpu": bytes.fromhex(sys.argv[2])}
    if len(sys.argv) > 3:
        keys["1bl"] = bytes.fromhex(sys.argv[3])
    for s in stages(d):
        print({k: (v.hex() if isinstance(v, bytes) else v) for k, v in s.items()})
        z, kn, _rk, p = try_decrypt(d, s, keys)
        print("   best:", f"zero={z:.2f}", kn, "head", p[:16].hex())
