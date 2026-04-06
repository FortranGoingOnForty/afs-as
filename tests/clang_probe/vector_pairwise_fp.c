#include <arm_neon.h>

float32x4_t pairwise_max(float32x4_t a, float32x4_t b) {
    return vpmaxq_f32(a, b);
}

float32x4_t pairwise_min(float32x4_t a, float32x4_t b) {
    return vpminq_f32(a, b);
}
