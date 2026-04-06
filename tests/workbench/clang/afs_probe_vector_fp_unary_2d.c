/* Workbench note: probe the float64 SIMD FP unary neighborhood after landing
 * the `.4s` family, to see whether clang emits `fabs.2d`, `fsqrt.2d`, and
 * `fneg.2d` as the next practical sibling set.
 */
#include <arm_neon.h>

float64x2_t abs2f(float64x2_t a) { return vabsq_f64(a); }
float64x2_t sqrt2f(float64x2_t a) { return vsqrtq_f64(a); }
float64x2_t neg2f(float64x2_t a) { return vnegq_f64(a); }
