typedef unsigned v4u __attribute__((vector_size(16)));
typedef unsigned short v8hu __attribute__((vector_size(16)));
typedef unsigned char v16bu __attribute__((vector_size(16)));
typedef unsigned long long v2ull __attribute__((vector_size(16)));

unsigned get_lane_u32(v4u a) { return a[2]; }

unsigned short get_lane_u16(v8hu a) { return a[5]; }

unsigned char get_lane_u8(v16bu a) { return a[7]; }

unsigned long long get_lane_u64(v2ull a) { return a[1]; }

v4u set_lane_u32(v4u a, unsigned x) {
    a[1] = x;
    return a;
}

v8hu set_lane_u16(v8hu a, unsigned short x) {
    a[5] = x;
    return a;
}

v16bu set_lane_u8(v16bu a, unsigned char x) {
    a[7] = x;
    return a;
}

v2ull set_lane_u64(v2ull a, unsigned long long x) {
    a[1] = x;
    return a;
}
