/*
 * Workbench note: this probe exposed the 64-bit-lane pairwise FP add family
 * from Apple clang, which led to adding `faddp.2d` and promoting the case into
 * `tests/clang_probe/vector_pairwise_add_fp_2d.c`.
 */
#include <arm_neon.h>

float64x2_t pairwise_add_f64(float64x2_t a, float64x2_t b) {
    return vpaddq_f64(a, b);
}
