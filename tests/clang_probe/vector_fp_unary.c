#include <arm_neon.h>

float32x4_t abs4f(float32x4_t a) { return vabsq_f32(a); }
float32x4_t negabs4f(float32x4_t a) { return -vabsq_f32(a); }
float32x4_t sqrt4f(float32x4_t a) { return vsqrtq_f32(a); }
