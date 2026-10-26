#!/usr/bin/env python3
"""Parse a SecCapture (tools/seccapture.cpp) capture and sanity-check it against
the retail HV image.

Usage:  python tools/parse_seccap.py [capture.bin]

Handles both record formats:
  v<=18: 12-byte header (tag, offset, len)
  v>=19: 16-byte header (tag, window, offset, len)

tag & 0xFFF is the region: 0 = physical/ciphertext, 1 = HV address space,
2 = SoC MMIO.  In the HV space, block k lives at
0x80000100_00000000 + k*0x200000000 + physical_offset.
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
KERNEL = ROOT / "dumps" / "stages" / "kernel_17559.bin"

# Where the interesting things are in the retail HV image.
HV_LOCATIONS = {
    0x15FD0: "syscall table (entry 0xD replaced by stage 3 with 0x354)",
    0x16004: "syscall table entry 0xD (HvxPostOutput)",
    0x100F0: "live DRAM key table (hash, white, aes) - zeroed in the retail image",
    0x16360: "SE register save area r2+0x16360",
    0x29B04: "HvpImageSignatureVerification patch site",
    0x30000: "last HV segment start",
}


def parse(d: bytes):
    magic, version, build, nchunks = struct.unpack_from(">4I", d, 0)
    hdr_len = 16 if version >= 19 else 12
    records = []
    off = 16
    while off + hdr_len <= len(d):
        tag = struct.unpack_from(">I", d, off)[0]
        win = struct.unpack_from(">I", d, off + 4)[0] if hdr_len == 16 else 0
        a = struct.unpack_from(">I", d, off + hdr_len - 8)[0]
        ln = struct.unpack_from(">I", d, off + hdr_len - 4)[0]
        off += hdr_len
        if off + ln > len(d):
            records.append((tag & 0xFFF, win, a, ln, None))
            off = len(d)
            break
        records.append((tag & 0xFFF, win, a, ln, d[off:off + ln]))
        off += ln
    return magic, version, build, nchunks, records, off


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "dumps/console/seccap_keys.bin"
    d = path.read_bytes()
    magic, version, build, nchunks, records, end = parse(d)
    print(f"file      {path} ({len(d)} bytes)")
    print(f"header    magic={magic:08x} version={version} build={build} chunks={nchunks}")
    print(f"parsed    {len(records)} record(s), {end} bytes consumed")
    if magic != 0x53454343:
        print("!! not a SecCapture file")

    hv = KERNEL.read_bytes() if KERNEL.exists() else b""

    for i, (region, win, off, ln, data) in enumerate(records):
        if data is None:
            print(f"[{i}] region={region} win={win} off=0x{off:05x} len={ln}  ** truncated **")
            continue
        nz = sum(1 for b in data if b)
        print(f"[{i}] region={region} win={win} off=0x{off:05x} len={ln:4d} nz={nz:4d} {data[:16].hex(' ')}")
        if region == 1 and hv and off + ln <= len(hv):
            expect = hv[off:off + ln]
            if data == expect:
                print("     == retail HV image (identical)")
            else:
                diffs = [j for j in range(ln) if data[j] != expect[j]]
                first = diffs[0]
                print(f"     != retail HV image ({len(diffs)} byte(s) differ, first at +0x{first:x}: "
                      f"{data[first]:02x} vs {expect[first]:02x})")

    print()
    print("HV landmarks present in the capture:")
    for (region, win, off, ln, data) in records:
        if region != 1 or data is None:
            continue
        for addr, label in HV_LOCATIONS.items():
            if off <= addr < off + ln:
                rel = addr - off
                if rel + 16 <= len(data):
                    val = struct.unpack_from(">2Q", data, rel)
                    print(f"  0x{addr:05x} (win {win}) {label}")
                    print(f"      {data[rel:rel+16].hex(' ')}  {val[0]:016x} {val[1]:016x}")

    # The key save area, if captured.
    for (region, win, off, ln, data) in records:
        if region != 1 or data is None or not (off <= 0x16360 < off + ln):
            continue
        rel = 0x16360 - off
        area = data[rel:rel + 0x40]
        print()
        print(f"key save area 0x80000102_00016360 (win {win}):")
        for row in range(0, 0x40, 16):
            print(f"  +{row:02x}  {area[row:row+16].hex(' ')}")
        chunks = [area[i * 16:(i + 1) * 16] for i in range(3)]
        for i, c in enumerate(chunks):
            print(f"  chunk{i}: {c.hex()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
