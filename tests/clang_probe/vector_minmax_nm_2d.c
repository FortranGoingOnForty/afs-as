#include <arm_neon.h>

float64x2_t maxnm2d(float64x2_t a, float64x2_t b) { return vmaxnmq_f64(a, b); }
float64x2_t minnm2d(float64x2_t a, float64x2_t b) { return vminnmq_f64(a, b); }
