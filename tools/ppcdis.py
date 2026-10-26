"""Annotated PPC disassembly of a XEX's code; calls to import thunks are named."""

import os
import sys

sys.path.insert(0, os.environ.get("PYLIB", ""))

from capstone import Cs, CS_ARCH_PPC, CS_MODE_32, CS_MODE_BIG_ENDIAN

from xbolib.xex import imports, strings


def main(path, out=sys.stdout):
    _x, img, base, secs, res = imports(path)
    thunks = {a: r[2] for a, r in res.items() if r[3] == "thunk"}
    strs = dict(strings(img, secs, base))
    md = Cs(CS_ARCH_PPC, CS_MODE_32 | CS_MODE_BIG_ENDIAN)
    for nm, va, vs, ro, rs in secs:
        if not nm.endswith("text"):
            continue
        print(f"; ---- {nm} {base + va:#x}", file=out)
        code = img[ro:ro + vs]
        for off in range(0, len(code) - 3, 4):  # one word at a time: capstone stops at VMX128
            i = next(md.disasm(code[off:off + 4], base + va + off), None)
            if i is None:
                print(f"{base + va + off:#x}: .long   {code[off:off + 4].hex()}", file=out)
                continue
            note = ""
            if i.mnemonic in ("bl", "b") and i.op_str.startswith("0x"):
                t = int(i.op_str, 16)
                if t in thunks:
                    note = f" ; -> {thunks[t]}"
            print(f"{i.address:#x}: {i.mnemonic:8s}{i.op_str}{note}", file=out)


if __name__ == "__main__":
    main(sys.argv[1])
