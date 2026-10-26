"""Crypto primitives shared by the XEX2 and boot-chain code.

Nothing here is key material: the AES/RC4 keys always come from the caller
(XEX header, keyvault or the public 1BL key).
"""

import hashlib
import hmac

from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes


def aes_decrypt(key: bytes, iv: bytes | None, data: bytes, mode) -> bytes:
    """AES-decrypt `data` with `key`; `mode` is modes.ECB/CBC and `iv` may be None."""
    c = Cipher(algorithms.AES(key), mode(iv) if iv is not None else mode())
    d = c.decryptor()
    return d.update(data) + d.finalize()


def rc4(key: bytes, data: bytes) -> bytes:
    """Classic RC4; the boot chain uses it up to the aligned stage size."""
    s = list(range(256))
    j = 0
    for i in range(256):
        j = (j + s[i] + key[i % len(key)]) & 255
        s[i], s[j] = s[j], s[i]
    i = j = 0
    out = bytearray()
    for b in data:
        i = (i + 1) & 255
        j = (j + s[i]) & 255
        s[i], s[j] = s[j], s[i]
        out.append(b ^ s[(s[i] + s[j]) & 255])
    return bytes(out)


def hmac_key(key: bytes, *parts: bytes) -> bytes:
    """HMAC-SHA1 over the given parts, truncated to the 16-byte RC4 key."""
    return hmac.new(key, b"".join(parts), hashlib.sha1).digest()[:16]
