#include <arm_neon.h>

unsigned min4u_reduce(uint32x4_t a) { return vminvq_u32(a); }
int min4s_reduce(int32x4_t a) { return vminvq_s32(a); }
