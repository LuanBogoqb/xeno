"""Verify generated XEX headers and the complete import allowlist independently."""
from pathlib import Path
import struct
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from xexinfo import parse, u16, u32


def verify(path):
    x = parse(path)
    bf = x['opt'][0x3ff]
    assert (u16(bf, 4), u16(bf, 6)) == (0, 1)
    image = bytearray()
    cursor = x['pe_offset']
    for i in range(8, len(bf), 8):
        stored, zeros = struct.unpack_from('>II', bf, i)
        assert cursor + stored <= len(x['b'])
        image += x['b'][cursor:cursor + stored] + bytes(zeros)
        cursor += stored
    assert cursor == len(x['b']) and image[:2] == b'MZ'
    pe = struct.unpack_from('<I', image, 0x3c)[0]
    assert image[pe:pe+4] == b'PE\0\0'
    assert struct.unpack_from('<H', image, pe+4)[0] == 0x1f2
    section_count = struct.unpack_from('<H', image, pe+6)[0]
    optional_size = struct.unpack_from('<H', image, pe+20)[0]
    sections = []
    for i in range(section_count):
        o = pe + 24 + optional_size + i * 40
        vs, va, rs, raw = struct.unpack_from('<IIII', image, o+8)
        sections.append((va, max(vs, rs), raw))
    assert x['opt'][0x10100] == 0x82010000 and x['opt'][0x10201] == 0x82000000
    def word(address):
        rva = address - 0x82000000
        for va, size, raw in sections:
            if va <= rva < va + size:
                return u32(image, raw + rva - va)
        raise AssertionError('Unmapped import')
    blob = x['opt'][0x103ff]
    _, strsize, count = struct.unpack_from('>III', blob)
    names = [s.decode() for s in blob[12:12+strsize].split(b'\0') if s]
    found = set()
    offset = 12 + strsize
    for _ in range(count):
        size = u32(blob, offset)
        index, records = struct.unpack_from('>HH', blob, offset+36)
        assert u32(blob, offset+28) == 0x20449700  # 2.0.17559.0
        for j in range(records):
            value = word(u32(blob, offset+40+4*j))
            if value >> 24 == 0:
                found.add((names[index & 255], value & 0xffff))
        offset += size
    # Ordinals independently resolved against tools/data/xboxkrnl_table.inc and xam_table.inc.
    from re import finditer
    tables = {}
    for lib, filename in [('xboxkrnl.exe','xboxkrnl_table.inc'),('xam.xex','xam_table.inc')]:
        text = (Path(__file__).resolve().parents[1] / 'data' / filename).read_text()
        tables[lib] = {int(m[1],16):m[2] for m in finditer(r'XE_EXPORT\(\w+,\s*(0x[0-9A-Fa-f]+),\s*(\w+)',text)}
    named = {(lib, tables[lib][ordinal]) for lib, ordinal in found}
    expected = {('xboxkrnl.exe', n) for n in ['DbgPrint','NtClose','NtCreateFile','NtWriteFile',
        'NtFlushBuffersFile','XexGetModuleHandle','MmIsAddressValid','MmQueryAddressProtect',
        'MmQueryStatistics','HalSendSMCMessage','XboxKrnlVersion','XboxKrnlBaseVersion','XboxHardwareInfo']}
    expected.add(('xam.xex','XamLoaderLaunchTitle'))
    assert named == expected, (named, expected)
    print(f'OK: PPC XEX, entry/base and {len(found)} expected imports verified')


if __name__ == '__main__':
    verify(sys.argv[1])
