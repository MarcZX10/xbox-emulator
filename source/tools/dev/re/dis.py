import sys,pefile,capstone
pe=pefile.PE(sys.argv[1],fast_load=True);pe.parse_data_directories(directories=[1,3])
img=pe.get_memory_mapped_image();b=pe.OPTIONAL_HEADER.ImageBase
iat={}
for d in pe.DIRECTORY_ENTRY_IMPORT:
    for i in d.imports: iat[i.address-b]=(i.name.decode() if i.name else str(i.ordinal))
fx={x.struct.BeginAddress:x.struct.EndAddress for x in getattr(pe,'DIRECTORY_ENTRY_EXCEPTION',[])}
md=capstone.Cs(capstone.CS_ARCH_X86,capstone.CS_MODE_64)
for a in sys.argv[2:]:
    s=int(a.split(':')[0],16); e=int(a.split(':')[1],16) if ':' in a else fx.get(s,s+0x100)
    for ins in md.disasm(img[s:e],b+s):
        t=''
        if 'rip' in ins.op_str:
            try:
                disp=int(ins.op_str.split('rip')[1].split(']')[0].replace(' ',''),16); r=ins.address+ins.size+disp-b; t=' ; '+iat.get(r,hex(r))
                bb=img[r:r+70]; z=bb.find(b'\0')
                if z>4 and all(32<=c<127 for c in bb[:z]): t+=' "'+bb[:z].decode()+'"'
            except Exception: pass
        print(hex(ins.address-b),ins.mnemonic,ins.op_str,t)
    print('=====')
