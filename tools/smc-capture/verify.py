"""Independently unpack and inspect the generated basic-compressed XEX."""
from pathlib import Path
import struct
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from xexinfo import parse, u16, u32
from ppcimports import sections, v2f, TABLES


def verify(path):
    x = parse(path)
    bf = x['opt'][0x3FF]
    assert (u16(bf, 4), u16(bf, 6)) == (0, 1), 'expected unencrypted basic compression'
    image = bytearray()
    cursor = x['pe_offset']
    for offset in range(8, len(bf), 8):
        stored, zeros = struct.unpack_from('>II', bf, offset)
        assert cursor + stored <= len(x['b'])
        image.extend(x['b'][cursor:cursor + stored])
        image.extend(bytes(zeros))
        cursor += stored
    assert cursor == len(x['b']), 'unexpected trailing data'
    assert image[:2] == b'MZ'
    pe = struct.unpack_from('<I', image, 0x3c)[0]
    assert image[pe:pe + 4] == b'PE\0\0'
    assert struct.unpack_from('<H', image, pe + 4)[0] == 0x1f2, 'expected big-endian PowerPC PE'
    base = x['opt'][0x10201]
    assert base == 0x82000000 and x['opt'][0x10100] == 0x82010000
    secs = sections(image)
    blob = x['opt'][0x103FF]
    _, string_size, count = struct.unpack_from('>III', blob)
    names = [n.decode() for n in blob[12:12 + string_size].split(b'\0') if n]
    offset = 12 + string_size
    found = set()
    for _ in range(count):
        length = u32(blob, offset)
        name_index, record_count = struct.unpack_from('>HH', blob, offset + 36)
        library = names[name_index & 255]
        assert u32(blob, offset + 28) == ((2 << 28) | (17559 << 8)), 'unexpected import version'
        for i in range(record_count):
            address = u32(blob, offset + 40 + 4 * i)
            pos = v2f(secs, address - base)
            assert pos is not None, 'unmapped import record'
            value = u32(image, pos)
            if value >> 24 == 0:
                found.add((library, TABLES[library][value & 0xffff]))
        offset += length
    expected = {('xboxkrnl.exe', n) for n in (
        'DbgPrint', 'HalSendSMCMessage', 'NtCreateFile', 'NtWriteFile', 'NtFlushBuffersFile', 'NtClose')}
    expected.add(('xam.xex', 'XamLoaderLaunchTitle'))
    assert found == expected, (found, expected)
    for library, name in sorted(found):
        print(f'{library}: {name}')
    print('OK: PowerPC XEX, expected entry/base and exactly seven expected imports')


if __name__ == '__main__':
    verify(sys.argv[1])
