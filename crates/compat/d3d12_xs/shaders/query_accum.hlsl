#define RS "RootConstants(num32BitConstants=4, b0), UAV(u0), UAV(u1)"
cbuffer C : register(b0) { uint src; uint dst; uint words; uint mode; };
RWByteAddressBuffer p : register(u0);
RWByteAddressBuffer s : register(u1);
[RootSignature(RS)]
[numthreads(1, 1, 1)]
void main() {
    for (uint i = 0; i < words; i++) {
        uint2 a = s.Load2(dst + i * 8);
        uint2 b = p.Load2(src + i * 8);
        uint64_t x = ((uint64_t)a.y << 32) | (uint64_t)a.x;
        uint64_t y = ((uint64_t)b.y << 32) | (uint64_t)b.x;
        uint64_t r = mode == 1 ? (uint64_t)((x | y) != 0) : x + y;
        s.Store2(dst + i * 8, uint2((uint)r, (uint)(r >> 32)));
    }
}
