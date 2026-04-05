#include <arm_neon.h>

float maxnm4f_reduce(float32x4_t v) { return vmaxnmvq_f32(v); }
float minnm4f_reduce(float32x4_t v) { return vminnmvq_f32(v); }
