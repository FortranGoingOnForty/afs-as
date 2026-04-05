#include <arm_neon.h>

unsigned sum4u(uint32x4_t a) { return vaddvq_u32(a); }
unsigned max4u_reduce(uint32x4_t a) { return vmaxvq_u32(a); }
int max4s_reduce(int32x4_t a) { return vmaxvq_s32(a); }
