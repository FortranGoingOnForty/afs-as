/* Workbench note: probing the float64 SIMD fused family after landing `.2d`
 * unary, convert, round, recip, compare, min/max, and arithmetic coverage.
 * If clang emits clean direct forms here, this is the next practical sibling
 * slice: `fmla.2d` and `fmls.2d`.
 */
#include <arm_neon.h>

float64x2_t mla2d(float64x2_t a, float64x2_t b, float64x2_t c) {
    return vfmaq_f64(a, b, c);
}

float64x2_t mls2d(float64x2_t a, float64x2_t b, float64x2_t c) {
    return vfmsq_f64(a, b, c);
}
