import sys,pefile,numpy as np,bisect,struct
pe=pefile.PE(sys.argv[1],fast_load=True);pe.parse_data_directories(directories=[3])
img=pe.get_memory_mapped_image();b=pe.OPTIONAL_HEADER.ImageBase
t=[s for s in pe.sections if s.Name.startswith(b'.text')][0]
a=np.frombuffer(img[t.VirtualAddress:t.VirtualAddress+t.Misc_VirtualSize],dtype=np.uint8)
n=len(a)-4
d=(a[:n].astype(np.int64)|(a[1:n+1].astype(np.int64)<<8)|(a[2:n+2].astype(np.int64)<<16)|(a[3:n+3].astype(np.int64)<<24))
d=np.where(d>=2**31,d-2**32,d);pos=np.arange(n)+t.VirtualAddress;e=pos+4+d
funcs=sorted((x.struct.BeginAddress,x.struct.EndAddress) for x in pe.DIRECTORY_ENTRY_EXCEPTION);keys=[f[0] for f in funcs]
def fn(r):
    i=bisect.bisect_right(keys,r)-1
    while i>=0 and funcs[i][1]<=r: i-=1
    return funcs[i][0] if i>=0 else None
def callers(tg):
    out=[]
    for p in pos[e==tg]:
        p=int(p)
        if img[p-1] in (0xe8,0xe9): out.append((fn(p),p-1,'call' if img[p-1]==0xe8 else 'jmp'))
        else: out.append((fn(p),p,'ref'))
    q=struct.pack('<Q',b+tg);i=img.find(q)
    while i>=0: out.append((None,i,'data')); i=img.find(q,i+1)
    return out
depth=int(sys.argv[2]); seen=set()
def walk(tg,lv):
    if lv>depth or tg in seen: return
    seen.add(tg)
    for f,p,k in callers(tg):
        print('  '*lv+'%s 0x%x <- %s at 0x%x'%(k,tg,hex(f) if f else '-',p))
        if f and k!='data': walk(f,lv+1)
for x in sys.argv[3:]: walk(int(x,16),0)
