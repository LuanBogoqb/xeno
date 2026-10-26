from capstone import Cs, CS_ARCH_PPC, CS_MODE_64, CS_MODE_BIG_ENDIAN
data=open(r'C:\Users\Luan Bogo\xb-emu\dumps\console\hypervisor_17559.bin','rb').read()
md=Cs(CS_ARCH_PPC, CS_MODE_64 | CS_MODE_BIG_ENDIAN)
md.detail=True
ins=list(md.disasm(data,0))
out=[]
for i in ins:
    out.append(f"{i.address:04x}: {i.mnemonic} {i.op_str}")
open(r'C:\Users\Luan Bogo\xb-emu\research\hv\analysis\full_disasm.txt','w').write('\n'.join(out))
print("count", len(ins))