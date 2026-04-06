#include <arm_neon.h>

float64x2_t absdiff2d(float64x2_t a, float64x2_t b) { return vabdq_f64(a, b); }
