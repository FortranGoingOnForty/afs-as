#include <arm_neon.h>

float32x4_t mla4f(float32x4_t a, float32x4_t b, float32x4_t c) { return vfmaq_f32(a, b, c); }
float32x4_t mls4f(float32x4_t a, float32x4_t b, float32x4_t c) { return vfmsq_f32(a, b, c); }
float32x4_t neg4f(float32x4_t a) { return vnegq_f32(a); }
