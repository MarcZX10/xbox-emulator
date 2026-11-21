RWByteAddressBuffer o : register(u0);
[numthreads(64, 1, 1)]
void main(uint3 t : SV_DispatchThreadID)
{
    uint x = t.x * 3 + 1;
    o.Store(t.x * 4, x * 2 + 0);
}
