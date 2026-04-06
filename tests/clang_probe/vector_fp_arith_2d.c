#include <arm_neon.h>

float64x2_t add2d(float64x2_t a, float64x2_t b) { return vaddq_f64(a, b); }
float64x2_t sub2d(float64x2_t a, float64x2_t b) { return vsubq_f64(a, b); }
float64x2_t mul2d(float64x2_t a, float64x2_t b) { return vmulq_f64(a, b); }
float64x2_t div2d(float64x2_t a, float64x2_t b) { return vdivq_f64(a, b); }
