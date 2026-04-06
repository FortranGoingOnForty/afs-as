// Workbench note: probe the plain float64 SIMD arithmetic neighborhood after
// landing the `.2d` min/max families, to see whether clang emits direct
// `fadd.2d` / `fsub.2d` / `fmul.2d` / `fdiv.2d` forms as a clean sibling slice.
#include <arm_neon.h>

float64x2_t add2d(float64x2_t a, float64x2_t b) { return vaddq_f64(a, b); }
float64x2_t sub2d(float64x2_t a, float64x2_t b) { return vsubq_f64(a, b); }
float64x2_t mul2d(float64x2_t a, float64x2_t b) { return vmulq_f64(a, b); }
float64x2_t div2d(float64x2_t a, float64x2_t b) { return vdivq_f64(a, b); }
