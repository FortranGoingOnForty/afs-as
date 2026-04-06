#include <arm_neon.h>

float32x4_t recip_est(float32x4_t a) { return vrecpeq_f32(a); }
float32x4_t recip_step(float32x4_t a, float32x4_t b) { return vrecpsq_f32(a, b); }
float32x4_t rsqrt_est(float32x4_t a) { return vrsqrteq_f32(a); }
float32x4_t rsqrt_step(float32x4_t a, float32x4_t b) { return vrsqrtsq_f32(a, b); }
