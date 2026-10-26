"""Dump XEX2 header: optional headers, security info, base-file format."""

import sys

from xbolib.binary import u16
from xbolib.xex import OPTIONAL_HEADERS, parse


def main(path, out=sys.stdout):
    x = parse(path)
    print({k: v for k, v in x.items() if k not in ("opt", "b")}, file=out)
    for k, v in x["opt"].items():
        name = OPTIONAL_HEADERS.get(k, "?")
        shown = hex(v) if isinstance(v, int) else v[:48].hex() + ("..." if len(v) > 48 else "")
        print(f"opt 0x{k:08X} {name}:", shown, f"[{len(v)}B]" if not isinstance(v, int) else "", file=out)
    if 0x3FF in x["opt"]:
        bf = x["opt"][0x3FF]
        print("base_file_format enc/comp:", u16(bf, 4), u16(bf, 6),
              "(enc 0 none/1 aes; comp 0 none/1 basic/2 lzx/3 delta)", file=out)


if __name__ == "__main__":
    main(sys.argv[1])
