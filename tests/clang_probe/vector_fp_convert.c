#include <arm_neon.h>

float32x4_t to_float_s32(int32x4_t a) { return vcvtq_f32_s32(a); }
float32x4_t to_float_u32(uint32x4_t a) { return vcvtq_f32_u32(a); }
int32x4_t to_int_s32(float32x4_t a) { return vcvtq_s32_f32(a); }
uint32x4_t to_uint_u32(float32x4_t a) { return vcvtq_u32_f32(a); }
