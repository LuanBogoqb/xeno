"""Decrypt the boot chain (CB_A, CB_B, CD, CE, CF) of a retail/RGH3 flash dump.

Keys follow InvoxiPlayGames/xenon-bltool (GPL-2, read for the algorithm only);
see research/flash/README.md for the per-stage derivation and validation.
"""

import sys

from xbolib.boot import dump_all

if __name__ == "__main__":
    raise SystemExit(dump_all(sys.argv[1], sys.argv[2]))
