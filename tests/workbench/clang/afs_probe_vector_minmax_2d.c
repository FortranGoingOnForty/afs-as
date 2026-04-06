/* Workbench note: probe the float64 SIMD min/max neighborhood after landing
 * the `.2d` compare-mask family, to see whether clang emits `fmax.2d` and
 * `fmin.2d` or folds these intrinsics to another sibling.
 */
#include <arm_neon.h>

float64x2_t max_f64x2(float64x2_t a, float64x2_t b) { return vmaxq_f64(a, b); }
float64x2_t min_f64x2(float64x2_t a, float64x2_t b) { return vminq_f64(a, b); }
