/* Workbench note: probing whether Apple clang emits the float64 SIMD compare-
 * with-zero family as the next practical `.2d` sibling after fused math.
 * If so, this would likely expose `fcmge.2d`, `fcmgt.2d`, `fcmle.2d`, or
 * `fcmlt.2d` zero-immediate forms rather than register-register compares.
 */
#include <arm_neon.h>

uint64x2_t ge_zero_f64(float64x2_t a) { return vcgezq_f64(a); }
uint64x2_t gt_zero_f64(float64x2_t a) { return vcgtzq_f64(a); }
uint64x2_t le_zero_f64(float64x2_t a) { return vclezq_f64(a); }
uint64x2_t lt_zero_f64(float64x2_t a) { return vcltzq_f64(a); }
