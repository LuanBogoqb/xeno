lines=open(r'C:\Users\Luan Bogo\xb-emu\research\hv\analysis\fulldis.txt').read().splitlines()
# parse
ins=[]
for l in lines:
    parts=l.split(':',1)
    if len(parts)<2: continue
    addr=int(parts[0],16)
    rest=parts[1].strip()
    toks=rest.split()
    mn=toks[0] if toks else ''
    op=' '.join(toks[1:]) if len(toks)>1 else ''
    ins.append((addr,mn,op))

# find mtctr rX preceded by li rX, N within 5 ins
res=[]
for i,(a,mn,op) in enumerate(ins):
    if mn=='mtctr':
        reg=op.strip()
        # look back up to 8
        for j in range(max(0,i-8),i):
            a2,mn2,op2=ins[j]
            if mn2=='li' and op2.split(',')[0].strip()==reg:
                # parse N
                try:
                    n=int(op2.split(',')[1].strip(),0)
                except:
                    n=None
                if n is not None and 8<=n<=20:
                    res.append((a,reg,n,ins[j][0]))
                    break
# unique by addr
seen=set()
for a,reg,n,la in res:
    if a in seen: continue
    seen.add(a)
    print(hex(a), 'ctr',reg,'=',n,'(li at',hex(la)+')')
print('count', len(seen))