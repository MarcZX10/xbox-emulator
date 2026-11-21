import struct, sys, bisect, pefile

meta_path, dll_path = sys.argv[1], sys.argv[2]
rvas = [int(x, 16) for x in sys.argv[3:]]
m = open(meta_path, 'rb').read()
assert struct.unpack_from('<I', m, 0)[0] == 0xFAB11BAF
ver = struct.unpack_from('<i', m, 4)[0]
names = ['stringLiteral', 'stringLiteralData', 'string', 'events', 'properties', 'methods', 'parameterDefaultValues', 'fieldDefaultValues', 'fieldAndParameterDefaultValueData', 'fieldMarshaledSizes', 'parameters', 'fields', 'genericParameters', 'genericParameterConstraints', 'genericContainers', 'nestedTypes', 'interfaces', 'vtableMethods', 'interfaceOffsets', 'typeDefinitions', 'images', 'assemblies']
h = {}
for i, n in enumerate(names):
    h[n] = struct.unpack_from('<ii', m, 8 + i * 8)


def s(idx):
    o = h['string'][0] + idx
    e = m.index(b'\0', o)
    return m[o:e].decode('utf-8', 'replace')


MD = 32
TD = 88
ID = 40
assert h['methods'][1] % MD == 0, h['methods']
assert h['typeDefinitions'][1] % TD == 0, h['typeDefinitions']
assert h['images'][1] % ID == 0, h['images']
types = []
for i in range(h['typeDefinitions'][1] // TD):
    o = h['typeDefinitions'][0] + i * TD
    f = struct.unpack_from('<7iI8i8HII', m, o)
    types.append({'name': s(f[0]), 'ns': s(f[1]), 'decl': f[3], 'methodStart': f[9], 'mcount': f[16]})
methods = []
for i in range(h['methods'][1] // MD):
    o = h['methods'][0] + i * MD
    f = struct.unpack_from('<6I4H', m, o)
    methods.append({'name': s(f[0]), 'type': f[1], 'token': f[5]})
images = []
for i in range(h['images'][1] // ID):
    o = h['images'][0] + i * ID
    f = struct.unpack_from('<10I', m, o)
    images.append({'name': s(f[0]), 'typeStart': f[2], 'typeCount': f[3]})


def tname(ti):
    t = types[ti]
    n = t['name']
    if t['decl'] >= 0 and t['decl'] < len(types) and t['decl'] != ti:
        pass
    return (t['ns'] + '.' if t['ns'] else '') + n


pe = pefile.PE(dll_path, fast_load=True)
img = pe.get_memory_mapped_image()
base = pe.OPTIONAL_HEADER.ImageBase
secs = [(x.Name.rstrip(b'\0').decode(), x.VirtualAddress, x.Misc_VirtualSize) for x in pe.sections]
data_like = [x for x in secs if x[0] in ('.data', '.rdata')]


def find_all(b):
    out = []
    i = img.find(b)
    while i >= 0:
        out.append(i)
        i = img.find(b, i + 1)
    return out


ptr_to = {}
for nm, va, sz in data_like:
    for o in range(va, va + sz - 8, 8):
        v = struct.unpack_from('<Q', img, o)[0]
        if base <= v < base + len(img):
            ptr_to.setdefault(v - base, []).append(o)

starts = []
for im in images:
    hits = [x for x in find_all(im['name'].encode() + b'\0') if img[x - 1] == 0]
    mod = None
    for hpos in hits:
        for ref in ptr_to.get(hpos, []):
            cnt = struct.unpack_from('<I', img, ref + 8)[0]
            arr = struct.unpack_from('<Q', img, ref + 16)[0]
            if 0 < cnt < 500000 and base <= arr < base + len(img):
                mod = (cnt, arr - base)
    if not mod:
        continue
    cnt, arr = mod
    for ti in range(im['typeStart'], im['typeStart'] + im['typeCount']):
        t = types[ti]
        for mi in range(t['methodStart'], t['methodStart'] + t['mcount']):
            md = methods[mi]
            rid = md['token'] & 0xFFFFFF
            if 1 <= rid <= cnt:
                p = struct.unpack_from('<Q', img, arr + (rid - 1) * 8)[0]
                if p:
                    starts.append((p - base, im['name'] + ' ' + tname(ti) + '::' + md['name']))
starts.sort()
keys = [x[0] for x in starts]
print('metadata v%d, %d methods with pointers' % (ver, len(starts)), file=sys.stderr)
for r in rvas:
    i = bisect.bisect_right(keys, r) - 1
    if i >= 0:
        print('+0x%x  %s +0x%x' % (r, starts[i][1], r - starts[i][0]))
    else:
        print('+0x%x  ?' % r)
