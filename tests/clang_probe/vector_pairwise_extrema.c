#include <arm_neon.h>

uint32x4_t pairwise_max_u32(uint32x4_t a, uint32x4_t b) {
    return vpmaxq_u32(a, b);
}

uint32x4_t pairwise_min_u32(uint32x4_t a, uint32x4_t b) {
    return vpminq_u32(a, b);
}

int32x4_t pairwise_max_s32(int32x4_t a, int32x4_t b) {
    return vpmaxq_s32(a, b);
}

int32x4_t pairwise_min_s32(int32x4_t a, int32x4_t b) {
    return vpminq_s32(a, b);
}
