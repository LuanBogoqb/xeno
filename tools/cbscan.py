"""Static scan of a decrypted CB stage: opcode histogram, constants built in registers, entry register state."""
import os, re, struct, sys
from collections import Counter
sys.path.insert(0, os.environ.get("PYLIB", ""))
from capstone import Cs, CS_ARCH_PPC, CS_MODE_64, CS_MODE_BIG_ENDIAN

M64 = (1 << 64) - 1
NODEST = ("st", "cmp", "mt", "b", "tw", "td", "sync", "isync", "dcb", "icb", "nop")


def disasm(d):
    entry = struct.unpack_from(">I", d, 8)[0]
    md = Cs(CS_ARCH_PPC, CS_MODE_64 | CS_MODE_BIG_ENDIAN)
    out = []
    for off in range(entry, len(d) - 3, 4):
        out.append((off, next(md.disasm(d[off:off + 4], off), None)))
    return entry, out


def constants(ins):
    """Linear (control-flow blind) tracking of constants built with li/lis/addi/ori/oris/sldi/mr."""
    known, consts = {}, {}
    for off, i in ins:
        if not i:
            continue
        o = i.op_str.replace(" ", "").split(",")
        m, v = i.mnemonic, None
        try:
            if m == "li": v = int(o[1], 0) & M64
            elif m == "lis": v = (int(o[1], 0) << 16) & M64
            elif m == "addi" and o[1] in known: v = (known[o[1]] + int(o[2], 0)) & M64
            elif m == "ori" and o[1] in known: v = known[o[1]] | (int(o[2], 0) & 0xFFFF)
            elif m == "oris" and o[1] in known: v = known[o[1]] | ((int(o[2], 0) & 0xFFFF) << 16)
            elif m == "sldi" and o[1] in known: v = (known[o[1]] << int(o[2], 0)) & M64
            elif m == "mr" and o[1] in known: v = known[o[1]]
        except ValueError:
            v = None
        if not o or not o[0].startswith("r") or m.startswith(NODEST):
            continue
        if v is None:
            known.pop(o[0], None)
        else:
            known[o[0]] = v
            if v >= 0x10000:
                consts.setdefault(v, off)
    return consts


def entry_regs(ins, n=64):
    written, readfirst = set(), []
    for off, i in ins[:n]:
        if not i:
            continue
        regs = re.findall(r"\br\d+\b", i.op_str)
        if not regs:
            continue
        nodest = i.mnemonic.startswith(NODEST)
        src = regs if nodest else regs[1:]
        for r in src:
            if r not in written and r not in readfirst:
                readfirst.append(r)
        if not nodest:
            written.add(regs[0])
        if i.mnemonic in ("lbzu", "lwzu", "ldu", "stdu", "stwu", "stbu"):
            written.add(regs[-1])
    return readfirst


def missing(ins, unknown):
    """Mnemonic histogram of the words the interpreter does not decode (offsets from `opscan`)."""
    return Counter(i.mnemonic if i else ".long" for off, i in ins if off in unknown)


def main(path, unknown_path=None):
    d = open(path, "rb").read()
    entry, ins = disasm(d)
    print(f"entry=0x{entry:x} size=0x{len(d):x} words={len(ins)}")
    print("\n## (a) mnemonics")
    for m, n in Counter(i.mnemonic if i else ".long" for _, i in ins).most_common():
        print(f"{n:6d} {m}")
    if unknown_path:
        unknown = {int(l.split()[0], 16) for l in open(unknown_path) if l.strip()}
        print("\n## (a2) mnemonics the interpreter does not decode (cargo run --bin opscan)")
        for m, n in missing(ins, unknown).most_common():
            print(f"{n:6d} {m}")
    print("\n## (b) constants built in registers, value >= 0x10000")
    for v, off in sorted(constants(ins).items()):
        print(f"0x{v:016x}  first at 0x{off:x}")
    print("\n## (c) absolute branches (ba/bla)")
    for off, i in ins:
        if i and i.mnemonic in ("ba", "bla"):
            print(f"0x{off:x}: {i.mnemonic} {i.op_str}")
    print("\n## (d) registers read before written (first 64 linear words, heuristic)")
    print(entry_regs(ins))
    print("\n## (e) mtspr/mfspr/sc/rfid/mtmsr uses")
    for off, i in ins:
        if i and i.mnemonic in ("mtspr", "mfspr", "sc", "rfid", "mtmsr", "mtmsrd", "mfmsr", "slbia", "tlbie", "sync", "isync", "dcbf", "icbi", "eieio"):
            print(f"0x{off:x}: {i.mnemonic} {i.op_str}")


if __name__ == "__main__":
    main(*sys.argv[1:3])
