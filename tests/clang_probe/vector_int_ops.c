typedef int v4i __attribute__((vector_size(16)));
typedef unsigned char v16u8 __attribute__((vector_size(16)));

v4i add4i(v4i a, v4i b) { return a + b; }

v4i sub4i(v4i a, v4i b) { return a - b; }

v16u8 and16(v16u8 a, v16u8 b) { return a & b; }

v16u8 or16(v16u8 a, v16u8 b) { return a | b; }

v16u8 xor16(v16u8 a, v16u8 b) { return a ^ b; }
