"""Big/little-endian field readers and alignment helpers."""

import struct


def u16(b: bytes, o: int) -> int:
    return struct.unpack_from(">H", b, o)[0]


def u16le(b: bytes, o: int) -> int:
    return struct.unpack_from("<H", b, o)[0]


def u32(b: bytes, o: int) -> int:
    return struct.unpack_from(">I", b, o)[0]


def u32le(b: bytes, o: int) -> int:
    return struct.unpack_from("<I", b, o)[0]


def align(n: int, boundary: int = 16) -> int:
    """Round `n` up to `boundary` (0x10 by default, as the boot chain does)."""
    return (n + boundary - 1) & ~(boundary - 1)
