// Probe result: this exposed the practical SIMD FP unary neighborhood after
// the fused slice. It led to adding `fabs.4s` and `fsqrt.4s`, while also
// keeping `fneg.4s` exercised in the same tracked probe.
#include <arm_neon.h>

float32x4_t abs4f(float32x4_t a) { return vabsq_f32(a); }
float32x4_t negabs4f(float32x4_t a) { return -vabsq_f32(a); }
