import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from collections import Counter
import cbscan


class I:
    def __init__(self, m): self.mnemonic = m


def test_missing_lists_only_unknown_offsets():
    ins = [(0, I("addi")), (4, I("sc")), (8, I("dcbf")), (12, None)]
    assert missing_of(ins, {4, 8}) == Counter({"sc": 1, "dcbf": 1})
    assert missing_of(ins, {12}) == Counter({".long": 1})
    assert missing_of(ins, set()) == Counter()


def missing_of(ins, unknown):
    return cbscan.missing(ins, unknown)


if __name__ == "__main__":
    test_missing_lists_only_unknown_offsets()
    print("ok")
