// Workbench note: probe the float64 NaN-aware SIMD min/max neighborhood after
// landing the plain `.2d` min/max family, to see whether clang emits direct
// `fmaxnm.2d` / `fminnm.2d` forms or lowers through another sibling.
#include <arm_neon.h>

float64x2_t maxnm2d(float64x2_t a, float64x2_t b) { return vmaxnmq_f64(a, b); }
float64x2_t minnm2d(float64x2_t a, float64x2_t b) { return vminnmq_f64(a, b); }
