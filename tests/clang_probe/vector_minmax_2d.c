#include <arm_neon.h>

float64x2_t max2d(float64x2_t a, float64x2_t b) { return vmaxq_f64(a, b); }
float64x2_t min2d(float64x2_t a, float64x2_t b) { return vminq_f64(a, b); }
