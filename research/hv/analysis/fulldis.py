from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]

from capstone import Cs, CS_ARCH_PPC, CS_MODE_64, CS_MODE_BIG_ENDIAN
data=open(ROOT / 'dumps/console/hypervisor_17559.bin','rb').read()
md=Cs(CS_ARCH_PPC, CS_MODE_64 | CS_MODE_BIG_ENDIAN)
lines=[]
for off in range(0,len(data)-3,4):
    r=list(md.disasm(data[off:off+4],off))
    if r:
        i=r[0]
        lines.append(f"{i.address:04x}: {i.mnemonic} {i.op_str}")
    else:
        lines.append(f"{off:04x}: <invalid>")
open(ROOT / 'research/hv/analysis/fulldis.txt','w').write('\n'.join(lines))
print('done', len(lines))