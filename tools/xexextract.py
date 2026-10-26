"""Decrypt (AES) + decompress (LZX) a XEX2 base file to a raw PE memory image."""

import sys

from xbolib.binary import u32
from xbolib.xex import extract

if __name__ == "__main__":
    img = extract(sys.argv[1])
    open(sys.argv[2], "wb").write(img)
    print(len(img), img[:4], hex(u32(img, 0x3C)) if len(img) > 0x40 else "")
