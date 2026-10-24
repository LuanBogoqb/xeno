"""Walk the boot chain of a flash dump (CB/CD/CE...) and try to decrypt each stage."""
import hmac, hashlib, struct, sys, itertools

def rc4(key, data):
    S = list(range(256)); j = 0
    for i in range(256):
        j = (j + S[i] + key[i % len(key)]) & 255; S[i], S[j] = S[j], S[i]
    i = j = 0; out = bytearray()
    for b in data:
        i = (i + 1) & 255; j = (j + S[i]) & 255; S[i], S[j] = S[j], S[i]
        out.append(b ^ S[(S[i] + S[j]) & 255])
    return bytes(out)

def stages(d):
    off = struct.unpack_from(">I", d, 8)[0]; out = []
    while True:
        magic = d[off:off + 2]
        if not (magic[:1] in (b"C", b"S") and magic[1:].isalpha()): break
        build, qfe, flags, entry, size = struct.unpack_from(">HHHII", d, off + 2)
        out.append(dict(name=magic.decode(), off=off, build=build, qfe=qfe, flags=flags, entry=entry, size=size,
                        salt=d[off + 0x10:off + 0x20]))
        off = (off + size + 0xF) & ~0xF
    return out

def zerofrac(b): return b.count(0) / max(1, len(b))

def try_decrypt(d, st, keys):
    body = d[st["off"] + 0x20:st["off"] + st["size"]]
    best = None
    for kn, k in keys.items():
        for sn, salt in (("own", st["salt"]),):
            rk = hmac.new(k, salt, hashlib.sha1).digest()[:16]
            p = rc4(rk, body)
            if best is None or zerofrac(p) > best[0]: best = (zerofrac(p), kn, sn, rk, p)
    return best

if __name__ == "__main__":
    d = open(sys.argv[1], "rb").read()
    cpu = bytes.fromhex(sys.argv[2])
    keys = {"cpu": cpu}
    if len(sys.argv) > 3: keys["1bl"] = bytes.fromhex(sys.argv[3])
    for s in stages(d):
        print({k: (v.hex() if isinstance(v, bytes) else v) for k, v in s.items()})
        r = try_decrypt(d, s, keys)
        print("   best:", f"zero={r[0]:.2f}", r[1], r[2], "head", r[4][:16].hex())
