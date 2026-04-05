#include <arm_neon.h>

float32x4_t pairwise_maxnm(float32x4_t a, float32x4_t b) {
    return vpmaxnmq_f32(a, b);
}

float32x4_t pairwise_minnm(float32x4_t a, float32x4_t b) {
    return vpminnmq_f32(a, b);
}
