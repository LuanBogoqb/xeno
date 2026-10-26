import json
ins=json.load(open(r'C:\Users\Luan Bogo\xb-emu\research\hv\analysis\ins.json'))
code_addrs=json.load(open(r'C:\Users\Luan Bogo\xb-emu\research\hv\analysis\code_addrs.json'))
ct=json.load(open(r'C:\Users\Luan Bogo\xb-emu\research\hv\analysis\call_targets.json'))
entries=sorted(set(ct+[0x100]+[v for v in range(0x200,0x1100,0x100)]))
# add addresses where a function might start: after blr? We'll also detect prologue? Keep simple.
# Build map addr->index in sorted code_addrs
code_sorted=sorted(code_addrs)
def next_entry(a):
    for e in entries:
        if e>a: return e
    return len(data) if False else 0x10000
# function ranges: from entry to next entry (exclusive), but trim trailing non-code? We'll just include code addresses in [entry,next_entry)
funcs={}
for i,e in enumerate(entries):
    nxt=entries[i+1] if i+1<len(entries) else 0x10000
    rng=[a for a in code_sorted if e<=a<nxt]
    funcs[e]=rng
# save
out={}
for e,rng in funcs.items():
    out[hex(e)]=[hex(a) for a in rng]
json.dump(out, open(r'C:\Users\Luan Bogo\xb-emu\research\hv\analysis\funcs.json','w'))
print('num funcs', len(funcs))
# print sizes
for e in sorted(funcs):
    print(hex(e), len(funcs[e]))