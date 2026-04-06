#include <arm_neon.h>

uint32x4_t pairwise_add_u32(uint32x4_t a, uint32x4_t b) {
    return vpaddq_u32(a, b);
}

int32x4_t pairwise_add_s32(int32x4_t a, int32x4_t b) {
    return vpaddq_s32(a, b);
}
