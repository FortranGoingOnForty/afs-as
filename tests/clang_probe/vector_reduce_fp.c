#include <arm_neon.h>

float add4f_reduce(float32x4_t v) { return vaddvq_f32(v); }
float max4f_reduce(float32x4_t v) { return vmaxvq_f32(v); }
float min4f_reduce(float32x4_t v) { return vminvq_f32(v); }
