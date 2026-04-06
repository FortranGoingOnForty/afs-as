/*
 * Workbench probe for the 64-bit-lane NaN-aware pairwise FP family from Apple
 * clang. This is intended to confirm whether clang emits `fmaxnmp.2d` and
 * `fminnmp.2d` so we can land the next practical sibling family as one slice.
 */
#include <arm_neon.h>

float64x2_t pairwise_maxnm_f64(float64x2_t a, float64x2_t b) {
    return vpmaxnmq_f64(a, b);
}

float64x2_t pairwise_minnm_f64(float64x2_t a, float64x2_t b) {
    return vpminnmq_f64(a, b);
}
