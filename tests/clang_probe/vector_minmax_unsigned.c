#include <arm_neon.h>

uint32x4_t max4u(uint32x4_t a, uint32x4_t b) { return vmaxq_u32(a, b); }
uint32x4_t min4u(uint32x4_t a, uint32x4_t b) { return vminq_u32(a, b); }
