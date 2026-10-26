"""Resolve XEX import records to names (Xenia export tables) and list call xrefs."""

import sys

from xbolib.xex import imports

if __name__ == "__main__":
    _x, _img, _base, _secs, res = imports(sys.argv[1])
    for addr in sorted(res):
        print(hex(addr), *res[addr])
