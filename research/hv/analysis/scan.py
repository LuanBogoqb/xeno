from capstone import Cs, CS_ARCH_PPC, CS_MODE_64, CS_MODE_BIG_ENDIAN
data=open(r'C:\Users\Luan Bogo\xb-emu\dumps\console\hypervisor_17559.bin','rb').read()
md=Cs(CS_ARCH_PPC, CS_MODE_64 | CS_MODE_BIG_ENDIAN)
SIZE=len(data)

visited=set()
code=set()
all_ins={}
worklist=[0x100]+[v for v in range(0x200,0x1100,0x100)]
processed=set()
call_targets=set()
functions=set()

def decode_one(addr):
    if addr<0 or addr+4>SIZE or addr%4: return None
    r=list(md.disasm(data[addr:addr+4],addr))
    if not r: return None
    return r[0]

while worklist:
    start=worklist.pop()
    if start in processed: continue
    addr=start
    while True:
        if addr in processed or addr in code:
            break
        if addr<0 or addr+4>SIZE or addr%4:
            break
        ins=decode_one(addr)
        if ins is None:
            break
        code.add(addr)
        all_ins[addr]=ins
        mn=ins.mnemonic
        op=ins.op_str
        # parse target for branches
        def parse_tgt(s):
            parts=s.replace(',',' ').split()
            for p in reversed(parts):
                try:
                    return int(p,0)
                except: pass
            try:
                return int(s,0)
            except:
                return None
        tgt=parse_tgt(op) if (mn.startswith('b') or mn.startswith('bc')) else None
        is_term=False
        if mn in ('bl','bla'):
            if tgt is not None:
                call_targets.add(tgt); functions.add(tgt)
            if tgt is not None and 0<=tgt<SIZE and tgt not in code:
                worklist.append(tgt)
        elif mn in ('b','ba','bctr','bctar'):
            if tgt is not None and 0<=tgt<SIZE and tgt not in code:
                worklist.append(tgt)
            is_term=True
        elif mn.startswith('bc') or mn.startswith('bd') or mn.startswith('bge') or mn.startswith('bgt') or mn.startswith('ble') or mn.startswith('blt') or mn.startswith('bne') or mn.startswith('beq'):
            # conditional branch
            if tgt is not None and 0<=tgt<SIZE and tgt not in code:
                worklist.append(tgt)
            # fallthrough
        # terminators
        if mn in ('blr','blrl','bclr','bcctr','bctr','rfid','rfi','sc','attn','bctar'):
            is_term=True
        if is_term:
            break
        addr+=4
    processed.add(start)

import json
json.dump(sorted(call_targets), open(r'C:\Users\Luan Bogo\xb-emu\research\hv\analysis\call_targets.json','w'))
json.dump(sorted(code), open(r'C:\Users\Luan Bogo\xb-emu\research\hv\analysis\code_addrs.json','w'))
json.dump({hex(k):[v.mnemonic,v.op_str] for k,v in all_ins.items()}, open(r'C:\Users\Luan Bogo\xb-emu\research\hv\analysis\ins.json','w'))
print("code ins count", len(code))
print("call targets count", len(call_targets))