/* Workbench note: probe the float64 SIMD reciprocal/refinement neighborhood
 * after landing `.2d` unary, convert, and rounding support, to see whether
 * clang emits `frecpe.2d`, `frecps.2d`, `frsqrte.2d`, and `frsqrts.2d`.
 */
#include <arm_neon.h>

float64x2_t recip_est2(float64x2_t a) { return vrecpeq_f64(a); }
float64x2_t recip_step2(float64x2_t a, float64x2_t b) { return vrecpsq_f64(a, b); }
float64x2_t rsqrt_est2(float64x2_t a) { return vrsqrteq_f64(a); }
float64x2_t rsqrt_step2(float64x2_t a, float64x2_t b) { return vrsqrtsq_f64(a, b); }
