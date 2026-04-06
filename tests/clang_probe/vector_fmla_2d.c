#include <arm_neon.h>

float64x2_t mla2d(float64x2_t a, float64x2_t b, float64x2_t c) { return vfmaq_f64(a, b, c); }
float64x2_t mls2d(float64x2_t a, float64x2_t b, float64x2_t c) { return vfmsq_f64(a, b, c); }
