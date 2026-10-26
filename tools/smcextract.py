"""Extract SMC firmware from a flat (no ECC/spare) Xbox 360 flash dump.

Header layout and cipher verified against J-Runner Nand.cs decrypt_SMC:
https://github.com/Octal450/J-Runner-with-Extras/blob/release/J-Runner/Nand/Nand.cs
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct


def decrypt(data):
    key = [0x42, 0x75, 0x4E, 0x79]
    result = bytearray()
    for i, value in enumerate(data):
        result.append(value ^ (key[i & 3] & 255))
        product = value * 251
        key[(i + 1) & 3] += product
        key[(i + 2) & 3] += product >> 8
    return bytes(result)


def extract(image):
    if len(image) < 0x80 or image[:2] != b'\xff\x4f':
        raise ValueError('expected flat FF4F flash image (no ECC/spare)')
    size, offset = struct.unpack_from('>II', image, 0x78)
    if not (0x103 <= size <= 0x4000 and 0x80 <= offset <= len(image) - size):
        raise ValueError('SMC header points outside the image or has an invalid size')
    raw = image[offset:offset + size]
    plain = decrypt(raw)
    return raw, plain, dict(offset=offset, size=size,
        version=f'{plain[0x101]}.{plain[0x102]:02d}',
        sha256_encrypted=hashlib.sha256(raw).hexdigest(),
        sha256_decrypted=hashlib.sha256(plain).hexdigest(),
        source_sha256=hashlib.sha256(image).hexdigest(),
        note='Firmware from flash, not a live SMC response. RGH patches may be present.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('flash', type=Path)
    parser.add_argument('output', type=Path, help='new output directory; existing paths are refused')
    args = parser.parse_args()
    raw, plain, info = extract(args.flash.read_bytes())
    args.output.mkdir(parents=True, exist_ok=False)
    (args.output / 'SMC_en.bin').write_bytes(raw)
    (args.output / 'SMC_dec.bin').write_bytes(plain)
    (args.output / 'manifest.json').write_text(json.dumps(info, indent=2) + '\n', encoding='utf-8')
    print(f'SMC firmware {info["version"]}: {info["size"]} bytes -> {args.output}')


if __name__ == '__main__':
    main()
