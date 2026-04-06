#include <arm_neon.h>

float32x4_t maxnm4f(float32x4_t a, float32x4_t b) { return vmaxnmq_f32(a, b); }
float32x4_t minnm4f(float32x4_t a, float32x4_t b) { return vminnmq_f32(a, b); }
